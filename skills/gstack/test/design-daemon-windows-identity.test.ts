import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { pathToFileURL } from 'node:url';

for (const { name, pwsh } of [
  { name: 'prefers installed PowerShell', pwsh: 'C:\\Program Files\\PowerShell\\7\\pwsh.exe' },
  { name: 'falls back when PowerShell is absent', pwsh: null },
]) test(`Windows daemon identity ${name} and fails closed on missing or failed queries`, () => {
  // Keep the native-platform adapter and module mock outside the parent shard.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'design-windows-identity-'));
  try {
    const script = path.join(dir, 'identity.test.ts');
    const stateModule = pathToFileURL(path.resolve(import.meta.dir, '../design/src/daemon-state.ts')).href;
    fs.writeFileSync(script, `
import { expect, mock, spyOn, test } from 'bun:test';
let commandLine = '"C:\\\\Program Files\\\\bun.exe" daemon.ts --gstack-design-daemon';
let queryFails = false;
const calls = [];
mock.module('child_process', () => ({ execFileSync(command, args, options) {
  calls.push({ command, args, options });
  if (queryFails) throw new Error('query failed');
  return commandLine;
} }));
const { verifyIdentity, readCmdline, CMDLINE_MARKER } = await import(${JSON.stringify(stateModule)});
test('actual identity gate consumes the native query and rejects negative controls', () => {
  const nativePlatform = Object.getOwnPropertyDescriptor(process, 'platform');
  const which = spyOn(Bun, 'which').mockReturnValue(${JSON.stringify(pwsh)});
  Object.defineProperty(process, 'platform', { value: 'win32' });
  try {
    expect(verifyIdentity(process.pid, CMDLINE_MARKER)).toBe(true);
    expect(calls).toHaveLength(1);
    expect(which).toHaveBeenCalledTimes(1);
    expect(which.mock.calls[0]).toEqual(['pwsh.exe', { PATH: process.env.PATH ?? '' }]);
    expect(calls[0].command).toBe(${JSON.stringify(pwsh ?? 'powershell.exe')});
    expect(calls[0].args).toContain('-NoProfile');
    expect(calls[0].args).toContain('-NonInteractive');
    expect(calls[0].args.at(-1)).toContain('ProcessId = ' + process.pid);
    expect(calls[0].options.encoding).toBe('utf8');
    expect(calls[0].options.stdio).toEqual(['ignore', 'pipe', 'ignore']);
    expect(calls[0].options.windowsHide).toBe(true);
    expect(calls[0].options.timeout).toBe(2000);
    expect(verifyIdentity(process.pid, CMDLINE_MARKER, 123)).toBe(true);
    expect(calls.at(-1).options.timeout).toBe(123);
    const boundedCount = calls.length;
    for (const timeout of [0, -1, NaN, Infinity]) expect(readCmdline(process.pid, timeout)).toBe('');
    expect(calls).toHaveLength(boundedCount);
    expect(which).toHaveBeenCalledTimes(boundedCount);
    commandLine = 'unrelated-process.exe';
    expect(verifyIdentity(process.pid, CMDLINE_MARKER)).toBe(false);
    commandLine = '';
    expect(verifyIdentity(process.pid, CMDLINE_MARKER)).toBe(false);
    queryFails = true;
    const failedCount = calls.length;
    expect(verifyIdentity(process.pid, CMDLINE_MARKER)).toBe(false);
    // A failed preferred executable must not trigger a second query through
    // the fallback and consume a second deadline or accept another result.
    expect(calls).toHaveLength(failedCount + 1);
    expect(calls.every(call => call.command === ${JSON.stringify(pwsh ?? 'powershell.exe')})).toBe(true);
    const count = calls.length;
    for (const pid of [0, -1, NaN, Infinity, 1.5]) expect(readCmdline(pid)).toBe('');
    expect(calls).toHaveLength(count);
    expect(which).toHaveBeenCalledTimes(count);
  } finally {
    which.mockRestore();
    Object.defineProperty(process, 'platform', nativePlatform);
  }
});
`);
    const result = Bun.spawnSync([process.execPath, 'test', script], {
      env: process.env, timeout: 10_000,
    });
    expect(result.exitCode, result.stdout.toString() + result.stderr.toString()).toBe(0);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
