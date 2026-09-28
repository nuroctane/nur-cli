"""Terminal startup regression tests against the real Nur binary.

The model-list response is held behind a gate. Typing and submitting a draft
must work before that gate opens; the first request must use the resolved model
afterward. This reproduces the pre-TUI network stall without timing the network.
Requires pyte to reconstruct terminal repaint sequences. Windows additionally
requires pywinpty; Unix uses the standard-library pty module.
All writable state is isolated. Run: python tests/e2e/run_startup.py --bin PATH
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import pyte
from http.server import ThreadingHTTPServer

from fake_provider import State, make_handler
from run_e2e import isolated_env, find_binary

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'target' / 'startup-e2e'


class BuilderLease:
    """Hold the same OS lease as Nur's index builder, without a test-only hook."""
    def __init__(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        self.file = path.open('w+b')
        self.file.write(b'0')
        self.file.flush()
        self.file.seek(0)
        if os.name == 'nt':
            import msvcrt
            msvcrt.locking(self.file.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(self.file, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def release(self):
        if self.file.closed: return
        if os.name == 'nt':
            import msvcrt
            self.file.seek(0)
            msvcrt.locking(self.file.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl
            fcntl.flock(self.file, fcntl.LOCK_UN)
        self.file.close()


class Terminal:
    def __init__(self, binary, cwd, env):
        self.output = ''
        self.screen = pyte.Screen(120, 32)
        self.stream = pyte.Stream(self.screen)
        self.chunks = queue.Queue()
        if os.name == 'nt':
            from winpty import PtyProcess
            self.proc = PtyProcess.spawn([str(binary)], cwd=str(cwd), env=env, dimensions=(32, 120))
            self.read = lambda: self.proc.read(65536)
            self.write = self.proc.write
        else:
            import pty
            import fcntl
            import termios
            import struct
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 120, 0, 0))
            self.fd = master
            self.proc = subprocess.Popen([str(binary)], cwd=cwd, env=env, stdin=slave, stdout=slave, stderr=slave)
            os.close(slave)
            self.read = lambda: os.read(master, 65536).decode('utf-8', errors='replace')
            self.write = lambda s: os.write(master, s.encode())
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        try:
            while True:
                chunk = self.read()
                if not chunk:
                    return
                self.chunks.put(chunk)
        except (OSError, EOFError):
            pass

    def feed(self, timeout):
        try:
            chunk = self.chunks.get(timeout=timeout)
        except queue.Empty:
            return False
        self.output += chunk
        self.stream.feed(chunk)
        return True

    def until(self, text, timeout=10, found=None):
        found = found or (lambda: text in '\n'.join(self.screen.display))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if found():
                return
            self.feed(.05)
        raise AssertionError(f'Screen never contained {text!r}: {ascii(chr(10).join(self.screen.display))}')

    def draft_ends_with(self, text):
        # Each frame leaves the terminal cursor on the draft's caret, so the
        # draft is what precedes it. Rows between `║` borders are the palette.
        y, x = self.screen.cursor.y, self.screen.cursor.x
        row = self.screen.buffer[y]
        before = ''.join(row[i].data for i in range(x))
        return '║' not in self.screen.display[y] and before.endswith(text)

    def type_text(self, text):
        # A bulk PTY write is a paste burst, not individual typing. Nur's paste
        # path also checks the OS image clipboard, which is outside our isolated
        # home. Wait for each key to reach the draft before sending the next so
        # this typing test neither reads the user's clipboard nor depends on its
        # contents. Seeing the text anywhere is not enough: the command palette
        # lists `/quit` once `/q` is typed, which let `uit` and Enter arrive as
        # one burst (Enter then became a pasted newline). A typed space changes
        # no visible cell under NO_COLOR; only the caret moves.
        typed = ''
        for char in text:
            self.write(char)
            typed += char
            self.until(typed, timeout=5, found=lambda: self.draft_ends_with(typed))

    def close(self):
        if os.name == 'nt':
            self.proc.terminate(force=True)
            self.proc.close(force=True)
        else:
            self.proc.terminate()
            self.proc.wait(timeout=5)
            os.close(self.fd)


def blocked_models(binary, skill=False, cancel=False):
    scenario = 'queued-skill' if skill else 'quit-during-startup' if cancel else 'blocked-models'
    work = OUT / f'{scenario}-{time.time_ns()}'
    work.mkdir(parents=True)
    home, workspace = work/'home', work/'workspace'
    workspace.mkdir()
    log = work/'requests.jsonl'
    gate, entered = threading.Event(), threading.Event()
    state = State([{'text': 'STARTUP_REQUEST_COMPLETED'}], log)
    base = make_handler(state)

    class Handler(base):
        def do_GET(self):
            entered.set()
            gate.wait(30)
            try:
                super().do_GET()
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = isolated_env(home, server.server_port)
    env['TERM'] = 'xterm-256color'
    env['NUR_STARTUP_TRACE'] = str(work/'startup.jsonl')
    if skill:
        for root, body in [(home/'.nur/skills', 'GLOBAL_SHADOWED_7261'),
                           (workspace/'.nur/skills', 'LOCAL_SKILL_BODY_7261')]:
            folder = root/'different-folder-name'
            folder.mkdir(parents=True)
            (folder/'SKILL.md').write_text('---\nname: startup-probe\ndescription: Startup fixture\n---\n'+body, encoding='utf-8')
    (home/'.nur/config.toml').write_text(
        f'provider = "vllm"\nmodel = "local-model"\nbase_url = "http://127.0.0.1:{server.server_port}/v1"\n'
        'theme = "gold"\necosystem_auto_ensure = false\nauto_update = false\n', encoding='utf-8')
    term = None
    lease = BuilderLease(home/'.nur/cache/skills-index.lock') if skill else None
    start = time.monotonic()
    try:
        term = Terminal(binary, workspace, env)
        term.until('F6 inspect', timeout=5)
        first_frame = time.monotonic() - start
        assert entered.wait(5), 'model discovery worker did not start'
        prompt = '/startup-probe STARTUP_DRAFT_7319' if skill else '/quit' if cancel else 'STARTUP_DRAFT_7319'
        term.type_text(prompt)
        term.until(prompt)
        term.write('\r')
        time.sleep(.3)
        assert state.count == 0, 'request sent before model discovery completed'
        if cancel:
            deadline = time.monotonic()+5
            while time.monotonic() < deadline:
                alive = term.proc.isalive() if os.name == 'nt' else term.proc.poll() is None
                if not alive:
                    print(f'PASS {scenario}: exited while model discovery was blocked', flush=True)
                    return {'scenario':scenario, 'passed':True, 'first_frame_seconds':first_frame}
                time.sleep(.05)
            raise AssertionError('quit waited for the blocked startup worker')
        gate.set()
        if lease:
            # Keep the skill worker stalled after the provider is usable. A
            # second draft must remain editable without sending the first early.
            term.type_text('STILL_EDITABLE_4832')
            term.until('STILL_EDITABLE_4832', timeout=5)
            assert state.count == 0, 'request bypassed pending skill discovery'
            lease.release()
        term.until('STARTUP_REQUEST_COMPLETED', timeout=45)
        requests = [json.loads(s) for s in log.read_text(encoding='utf-8').splitlines()]
        assert len(requests) == 1, f'draft submitted {len(requests)} times'
        assert requests[0]['model'] == 'e2e-model', requests[0]['model']
        assert any('STARTUP_DRAFT_7319' in str(m.get('content','')) for m in requests[0]['messages'])
        if skill:
            sent = json.dumps(requests[0]['messages'])
            assert 'LOCAL_SKILL_BODY_7261' in sent, 'queued slash did not activate the project override'
            assert 'GLOBAL_SHADOWED_7261' not in sent, 'global skill overrode the project skill'
        trace = [json.loads(row) for row in (work/'startup.jsonl').read_text().splitlines()]
        stages = {row['stage']:row['ms'] for row in trace}
        assert stages['session_saved'] <= stages['ready'], 'request became ready before the session was saved'
        assert stages['catalog_ready'] <= stages['ready'], 'request became ready before model metadata'
        print(f'PASS {scenario}: first frame {first_frame:.3f}s, draft delivered exactly once', flush=True)
        return {'scenario':scenario,'first_frame_seconds':first_frame,'passed':True}
    finally:
        gate.set()
        if lease: lease.release()
        if term:
            (work/'terminal.txt').write_text(term.output, encoding='utf-8')
            term.close()
        server.shutdown()
        server.server_close()


def blocked_auth(binary, fail=False):
    scenario = 'failed-auth-fallback' if fail else 'blocked-auth'
    work = OUT / f'{scenario}-{time.time_ns()}'
    work.mkdir(parents=True)
    home, workspace = work/'home', work/'workspace'
    workspace.mkdir()
    env = isolated_env(home, 1)
    env['TERM'] = 'xterm-256color'
    helpers = work/'helpers'
    helpers.mkdir()
    gate, entered = work/'release', work/'entered'
    helper = helpers/'refresh.py'
    helper.write_text(
        'import pathlib,time,sys\n'
        f'p=pathlib.Path({str(work)!r})\n'
        'with (p/"entered").open("a",encoding="utf-8") as log: log.write("refresh\\n")\n'
        'end=time.monotonic()+30\n'
        'while not (p/"release").exists() and time.monotonic()<end: time.sleep(.02)\n'
        + ('sys.exit(1)\n' if fail else 'print("synthetic-refreshed-token")\n'), encoding='utf-8')
    if os.name == 'nt':
        (helpers/'gh.cmd').write_text(f'@echo off\r\n"{sys.executable}" "{helper}"\r\n', encoding='utf-8')
    else:
        import shlex
        path = helpers/'gh'
        path.write_text(f'#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(helper))}\n')
        path.chmod(0o700)
    env['PATH'] = str(helpers)+os.pathsep+env.get('PATH','')
    (home/'.nur/config.toml').write_text(
        'provider = "github-copilot"\nmodel = "gpt-4o"\ntheme = "gold"\n'
        'auto_update = false\necosystem_auto_ensure = false\n', encoding='utf-8')
    (home/'.nur/auth.json').write_text(json.dumps({
        'api_key':'synthetic-expired-token','provider':'github-copilot',
        'source':'oauth','auth_method':'oauth','refresh_token':'gh','expires_at':1,
    }), encoding='utf-8')
    if fail:
        (home/'.nur/provider_keys.json').write_text(json.dumps({'github-copilot':'synthetic-fallback-key'}), encoding='utf-8')
    env['NUR_STARTUP_TRACE'] = str(work/'startup.jsonl')
    term = None
    try:
        term = Terminal(binary, workspace, env)
        term.until('F6 inspect', timeout=5)
        end = time.monotonic()+5
        while not entered.exists() and time.monotonic()<end: time.sleep(.02)
        assert entered.exists(), 'synthetic credential refresh was not entered'
        term.type_text('AUTH_WAIT_DRAFT_8257')
        term.until('AUTH_WAIT_DRAFT_8257', timeout=5)
        assert not gate.exists(), 'authentication gate was released before typing worked'
        gate.write_text('release')
        end = time.monotonic()+10
        while time.monotonic()<end:
            saved = json.loads((home/'.nur/auth.json').read_text())
            trace = (work/'startup.jsonl').read_text()
            if (fail and 'session_saved' in trace) or (not fail and saved['api_key'] == 'synthetic-refreshed-token'): break
            time.sleep(.05)
        if fail:
            assert 'session_saved' in trace, 'failed refresh never reached the API-key fallback'
            assert len(entered.read_text().splitlines()) == 1, 'failed active OAuth was refreshed more than once'
        else:
            assert saved['api_key'] == 'synthetic-refreshed-token', 'refreshed credentials were not saved'
        term.until('AUTH_WAIT_DRAFT_8257')
        print(f'PASS {scenario}: typing remained live and credential preparation completed', flush=True)
        return {'scenario':scenario,'passed':True}
    finally:
        gate.write_text('release')
        if term:
            (work/'terminal.txt').write_text(term.output, encoding='utf-8')
            term.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin')
    parser.add_argument('scenarios', nargs='*')
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    binary = find_binary(args.bin).resolve()
    cases = {'blocked-models':lambda:blocked_models(binary),
             'queued-skill':lambda:blocked_models(binary, skill=True),
             'quit-during-startup':lambda:blocked_models(binary, cancel=True),
             'blocked-auth':lambda:blocked_auth(binary),
             'failed-auth-fallback':lambda:blocked_auth(binary, fail=True)}
    result = [cases[name]() for name in (args.scenarios or cases)]
    (OUT/'report.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
