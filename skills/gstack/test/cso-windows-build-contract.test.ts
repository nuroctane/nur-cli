import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

const source = fs.readFileSync(path.join(import.meta.dir, 'cso-windows-launcher.test.ts'), 'utf8');
const contract = source.slice(source.indexOf("describe('CSO native Windows build contract'"), source.indexOf("(windows ? describe : describe.skip)"));
const rejection = { status: 1, signal: null, stdout: '', stderr: 'direct, non-reparse staging directory' };

function registeredCases(result = rejection) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cso-build-contract-'));
  fs.mkdirSync(path.join(root, 'bin'));
  const cases: { name: string; run: () => void; timeout: number }[] = [];
  const children: { args: string[]; options: { timeout: number } }[] = [];
  const register = (name: string, run: () => void, timeout: number) => {
    if (name.startsWith('Windows build ')) cases.push({ name, run, timeout });
  };
  const testAdapter = Object.assign(register, {
    skipIf: () => testAdapter,
    each: (rows: readonly unknown[][]) => (name: string, run: (...args: unknown[]) => void, timeout: number) => {
      for (const row of rows) register(name.replace('%s', String(row[0])), () => run(...row), timeout);
    },
  });
  const executable = new Bun.Transpiler({ loader: 'ts', target: 'bun' }).transformSync(contract);
  new Function('describe', 'test', 'expect', 'fs', 'os', 'path', 'ROOT', 'windows', 'spawnSync', executable)(
    (_name: string, run: () => void) => run(), testAdapter, expect, fs, os, path, root, true,
    (command: string, args: string[], options: { timeout: number }) => {
      expect(command).toBe('powershell.exe');
      children.push({ args, options });
      return result;
    },
  );
  return { root, cases, children, cleanup: () => fs.rmSync(root, { recursive: true, force: true }) };
}

test('every native path probe has its own child deadline and bounded cleanup allowance', () => {
  const fixture = registeredCases();
  try {
    expect(fixture.cases).toHaveLength(5);
    expect(new Set(fixture.cases.map(entry => entry.name)).size).toBe(5);
    for (const entry of fixture.cases) {
      const before = fixture.children.length;
      entry.run();
      expect(fixture.children).toHaveLength(before + 1);
      expect(fixture.children.at(-1)!.options.timeout).toBe(30_000);
      expect(entry.timeout).toBe(35_000);
      const args = fixture.children.at(-1)!.args;
      const output = args[args.indexOf('-OutputPath') + 1];
      const lock = args[args.indexOf('-LockOutputPath') + 1];
      expect(output).toBeDefined();
      expect(lock).toBeDefined();
      expect(fs.readdirSync(path.join(fixture.root, 'bin'))).toEqual([]);
    }
  } finally { fixture.cleanup(); }
});

test.each([
  ['accepted output', { ...rejection, status: 0 }],
  ['timeout after diagnostic', { ...rejection, status: null, signal: 'SIGTERM', error: new Error('ETIMEDOUT') }],
  ['unrelated failure', { ...rejection, stderr: 'MSVC is unavailable' }],
] as const)('native path assertions reject %s and still clean their fixtures', (_name, result) => {
  const fixture = registeredCases(result as typeof rejection);
  try {
    expect(fixture.cases).toHaveLength(5);
    for (const entry of fixture.cases) {
      expect(entry.run).toThrow();
      expect(fs.readdirSync(path.join(fixture.root, 'bin'))).toEqual([]);
    }
  } finally { fixture.cleanup(); }
});
