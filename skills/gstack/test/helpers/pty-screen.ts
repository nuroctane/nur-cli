import * as fs from 'node:fs';
import * as path from 'node:path';
import { createRequire } from 'node:module';

/** The existing xterm package ships this headless source beside its browser bundle. */
let terminalConstructor: Promise<any> | undefined;
function loadTerminal(): Promise<any> {
  return terminalConstructor ??= (async () => {
    try {
      const source = path.join(path.dirname(createRequire(import.meta.url).resolve('xterm/package.json')), 'src');
      const entry = path.join(source, 'headless/public/Terminal.ts');
      if (!fs.existsSync(entry)) throw new Error(`Installed xterm headless source is missing: ${entry}`);
      const transpiler = new Bun.Transpiler({ loader: 'ts', target: 'bun', trimUnusedImports: true,
        tsconfig: JSON.stringify({ compilerOptions: { experimentalDecorators: true, useDefineForClassFields: false } }) });
      const build = await Bun.build({ entrypoints: [entry], target: 'bun', format: 'esm',
        // xterm5 detects Node by navigator absence. This changes only the
        // compiled module; Bun's global navigator and other tests stay intact.
        define: { navigator: 'undefined' },
        plugins: [{ name: 'installed-xterm-headless', setup(builder) {
          builder.onResolve({ filter: /^(common|headless)\// }, args => {
            const stem = path.join(source, args.path);
            return { path: fs.existsSync(`${stem}.ts`) ? `${stem}.ts` : `${stem}.d.ts` };
          });
          builder.onLoad({ filter: /[/\\]xterm[/\\]src[/\\].*\.ts$/ }, async args => ({
            contents: transpiler.transformSync(await Bun.file(args.path).text()), loader: 'js',
          }));
        } }],
      });
      if (!build.success) throw new AggregateError(build.logs, 'Installed xterm headless build failed');
      const encoded = Buffer.from(await build.outputs[0].text()).toString('base64');
      return (await import(`data:text/javascript;base64,${encoded}`)).Terminal;
    } catch (cause) {
      throw new Error('PTY screen unavailable; cannot safely observe terminal input.', { cause });
    }
  })();
}

export interface PtyScreenFrame {
  text: string;
  /** JS string offset of the same writes consumed by the viewport. */
  inputOffset: number;
  styledText: Array<{ row: number; start: number; text: string; dim: boolean; inverse: boolean }>;
}

export interface PtyScreen {
  write(text: string): void;
  read(deadlineAt?: number): Promise<string>;
  readFrame(deadlineAt?: number): Promise<PtyScreenFrame>;
  dispose(): Promise<void>;
}

/** One terminal per session; read only the actual viewport, never scrollback. */
export async function createPtyScreen(cols: number, rows: number,
  options: { deadlineAt?: number; signal?: AbortSignal } = {}): Promise<PtyScreen> {
  const deadlineAt = options.deadlineAt ?? performance.now() + 5_000;
  if (!Number.isFinite(deadlineAt)) throw new RangeError('PTY screen requires a finite absolute deadline.');
  const Terminal = await loadTerminal();
  const terminal = new Terminal({ cols, rows, scrollback: 0, allowProposedApi: true });
  // Match current CLI scalar column widths instead of xterm5's Unicode 6
  // default. xterm still handles combining cells; this is not grapheme shaping.
  terminal.unicode.register({
    version: 'bun-scalar',
    wcwidth(codepoint: number) {
      const width = Bun.stringWidth(String.fromCodePoint(codepoint));
      if (width !== 0 && width !== 1 && width !== 2) throw new Error('Invalid terminal scalar width.');
      return width;
    },
  });
  terminal.unicode.activeVersion = 'bun-scalar';
  let pending = 0;
  let inputOffset = 0;
  let failure: unknown;
  let final: PtyScreenFrame | undefined;
  let closing: Promise<void> | undefined;
  const waiting = new Set<() => void>();
  const settled = () => { if (pending === 0 || failure) { for (const done of waiting) done(); waiting.clear(); } };
  const fail = (error: unknown) => {
    failure ??= new Error('PTY screen parse failed; viewport is incomplete.', { cause: error });
    settled();
  };
  const drain = async (readDeadline = deadlineAt) => {
    if (!Number.isFinite(readDeadline)) throw new RangeError('PTY screen requires a finite absolute deadline.');
    while (pending > 0 && !failure) {
      if (options.signal?.aborted) { fail(options.signal.reason); break; }
      const remaining = Math.min(deadlineAt, readDeadline) - performance.now();
      if (remaining <= 0) { fail(new Error('PTY screen write callback deadline exceeded.')); break; }
      await new Promise<void>(resolve => {
        const done = () => {
          clearTimeout(timer);
          options.signal?.removeEventListener('abort', abort);
          waiting.delete(done);
          resolve();
        };
        const abort = () => fail(options.signal?.reason);
        const timer = setTimeout(() => fail(new Error('PTY screen write callback deadline exceeded.')), remaining);
        waiting.add(done);
        options.signal?.addEventListener('abort', abort, { once: true });
      });
    }
    if (failure) throw failure;
  };
  const viewport = () => {
    const buffer = terminal.buffer.active;
    const styledText: PtyScreenFrame['styledText'] = [];
    const lines = Array.from({ length: rows }, (_, row) => {
      const line = buffer.getLine(buffer.baseY + row);
      let start = 0, previous = '';
      for (let col = 0; col <= cols; col++) {
        const cell = col < cols ? line?.getCell(col) : undefined;
        const key = cell && (cell.getChars() || cell.getWidth() === 0)
          ? `${Number(!!cell.isDim())}${Number(!!cell.isInverse())}` : '';
        if (key === previous) continue;
        if (previous && previous !== '00') styledText.push({row, start,
          text: line!.translateToString(false, start, col), dim: previous[0] === '1', inverse: previous[1] === '1'});
        start = col; previous = key;
      }
      return line?.translateToString(true) ?? '';
    });
    return {text: lines.join('\n'), inputOffset, styledText};
  };
  const readFrame = async (readDeadline?: number) => {
    await drain(readDeadline);
    if (closing) { await closing; return final!; }
    return viewport();
  };
  return {
    write(text) {
      if (closing) throw new Error('Cannot write to a disposed PTY screen.');
      if (!text) return;
      pending++;
      inputOffset += text.length;
      let completed = false;
      const complete = () => { if (!completed) { completed = true; pending--; settled(); } };
      try { terminal.write(text, complete); }
      catch (error) { fail(error); complete(); }
    },
    async read(readDeadline) {
      return (await readFrame(readDeadline)).text;
    },
    readFrame,
    dispose() {
      return closing ??= (async () => {
        try { await drain(); final = viewport(); }
        finally {
          try { terminal.dispose(); }
          catch (error) { if (!failure) throw error; }
        }
      })();
    },
  };
}
