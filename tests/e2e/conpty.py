"""Minimal Windows ConPTY driver with a streaming UTF-8 decoder.

pywinpty's intermediate string reads can replace a glyph split at a 4 KiB
boundary. ReadFile returns bytes here, so animated redraws remain lossless.
Only used by the black-box terminal tests; no product-specific hooks.
"""
import codecs
import ctypes as c
from ctypes import wintypes as w
import subprocess


class Coord(c.Structure):
    _fields_ = [('x', c.c_short), ('y', c.c_short)]


class StartupInfo(c.Structure):
    _fields_ = [('cb', w.DWORD), ('reserved', w.LPWSTR), ('desktop', w.LPWSTR),
                ('title', w.LPWSTR), ('x', w.DWORD), ('y', w.DWORD),
                ('xsize', w.DWORD), ('ysize', w.DWORD), ('xchars', w.DWORD),
                ('ychars', w.DWORD), ('fill', w.DWORD), ('flags', w.DWORD),
                ('show', w.WORD), ('reserved_size', w.WORD),
                ('reserved_bytes', c.POINTER(c.c_byte)), ('stdin', w.HANDLE),
                ('stdout', w.HANDLE), ('stderr', w.HANDLE)]


class StartupInfoEx(c.Structure):
    _fields_ = [('info', StartupInfo), ('attributes', c.c_void_p)]


class ProcessInfo(c.Structure):
    _fields_ = [('process', w.HANDLE), ('thread', w.HANDLE),
                ('pid', w.DWORD), ('tid', w.DWORD)]


kernel = c.WinDLL('kernel32', use_last_error=True)


def api(name, result, *args):
    fn = getattr(kernel, name)
    fn.restype, fn.argtypes = result, args
    return fn


create_pipe = api('CreatePipe', w.BOOL, c.POINTER(w.HANDLE), c.POINTER(w.HANDLE), c.c_void_p, w.DWORD)
close_handle = api('CloseHandle', w.BOOL, w.HANDLE)
create_console = api('CreatePseudoConsole', c.c_long, Coord, w.HANDLE, w.HANDLE, w.DWORD, c.POINTER(w.HANDLE))
close_console = api('ClosePseudoConsole', None, w.HANDLE)
init_attributes = api('InitializeProcThreadAttributeList', w.BOOL, c.c_void_p, w.DWORD, w.DWORD, c.POINTER(c.c_size_t))
update_attribute = api('UpdateProcThreadAttribute', w.BOOL, c.c_void_p, w.DWORD, c.c_size_t, c.c_void_p, c.c_size_t, c.c_void_p, c.c_void_p)
delete_attributes = api('DeleteProcThreadAttributeList', None, c.c_void_p)
create_process = api('CreateProcessW', w.BOOL, w.LPCWSTR, w.LPWSTR, c.c_void_p, c.c_void_p, w.BOOL,
                     w.DWORD, c.c_void_p, w.LPCWSTR, c.POINTER(StartupInfoEx), c.POINTER(ProcessInfo))
read_file = api('ReadFile', w.BOOL, w.HANDLE, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), c.c_void_p)
write_file = api('WriteFile', w.BOOL, w.HANDLE, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), c.c_void_p)
wait_process = api('WaitForSingleObject', w.DWORD, w.HANDLE, w.DWORD)
terminate_process = api('TerminateProcess', w.BOOL, w.HANDLE, w.UINT)


def checked(ok):
    if not ok:
        raise c.WinError(c.get_last_error())


class ConPtyProcess:
    def __init__(self, binary, cwd, env, columns=120, rows=32):
        self.input = self.output = self.console = self.process = None
        self.decoder = codecs.getincrementaldecoder('utf-8')()
        input_read, input_write, output_read, output_write = (w.HANDLE() for _ in range(4))
        attributes = None
        initialized = False
        try:
            checked(create_pipe(c.byref(input_read), c.byref(input_write), None, 0))
            self.input = input_write
            checked(create_pipe(c.byref(output_read), c.byref(output_write), None, 0))
            self.output = output_read
            console = w.HANDLE()
            result = create_console(Coord(columns, rows), input_read, output_write, 0, c.byref(console))
            if result < 0:
                raise OSError(f'CreatePseudoConsole failed: HRESULT {result:#x}')
            self.console = console
            size = c.c_size_t()
            init_attributes(None, 1, 0, c.byref(size))
            attributes = c.create_string_buffer(size.value)
            checked(init_attributes(attributes, 1, 0, c.byref(size)))
            initialized = True
            checked(update_attribute(attributes, 0, 0x00020016, console, c.sizeof(console), None, None))
            startup = StartupInfoEx()
            startup.info.cb = c.sizeof(startup)
            # Explicit null std handles let ConPTY supply them even when this
            # test runner's stdout/stderr are redirected to log files.
            startup.info.flags = 0x00000100  # STARTF_USESTDHANDLES
            startup.attributes = c.cast(attributes, c.c_void_p)
            process = ProcessInfo()
            command = c.create_unicode_buffer(subprocess.list2cmdline([str(binary)]))
            environment = c.create_unicode_buffer('\0'.join(f'{key}={value}' for key, value in
                sorted(env.items(), key=lambda pair: pair[0].upper())) + '\0')
            # Extended startup info + Unicode environment. ConPTY owns the
            # console, so this never opens a visible console window.
            checked(create_process(None, command, None, None, False, 0x00080000 | 0x00000400,
                environment, str(cwd), c.byref(startup), c.byref(process)))
            self.process = process.process
            close_handle(process.thread)
        except BaseException:
            self.terminate()
            self.close()
            raise
        finally:
            if initialized:
                delete_attributes(attributes)
            for handle in (input_read, output_write):
                if handle:
                    close_handle(handle)

    def read(self):
        buffer = c.create_string_buffer(65536)
        count = w.DWORD()
        while True:
            if not read_file(self.output, buffer, len(buffer), c.byref(count), None):
                error = c.get_last_error()
                if error in (6, 109, 232):
                    return self.decoder.decode(b'', final=True)
                raise c.WinError(error)
            if not count.value:
                return self.decoder.decode(b'', final=True)
            text = self.decoder.decode(buffer.raw[:count.value])
            if text:
                return text

    def write(self, text):
        data = text.encode('utf-8')
        while data:
            count = w.DWORD()
            checked(write_file(self.input, data, len(data), c.byref(count), None))
            if not count.value:
                raise OSError('ConPTY input pipe closed')
            data = data[count.value:]

    def isalive(self):
        return self.process is not None and wait_process(self.process, 0) == 258

    def terminate(self, force=True):
        if self.isalive():
            checked(terminate_process(self.process, 1))
            wait_process(self.process, 5000)

    def close(self, force=True):
        if self.console:
            close_console(self.console)
            self.console = None
        for name in ('input', 'output', 'process'):
            handle = getattr(self, name)
            if handle:
                close_handle(handle)
                setattr(self, name, None)
