import { afterAll, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';

const CLI = path.resolve(import.meta.dir, '../bin/gstack-qa-evidence');
const ROOT = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'qa-evidence-'));
afterAll(() => fs.rmSync(ROOT, { recursive: true, force: true }));

function fixture() {
  const root = fs.mkdtempSync(path.join(ROOT, 'case-'));
  const run = (...args: string[]) => {
    expect(JSON.stringify([process.execPath, CLI, ...args]).length, 'Fixture argv must stay short for Windows process launch').toBeLessThan(8192);
    const result = spawnSync(process.execPath, [CLI, ...args], { cwd: root, encoding: 'utf8', timeout: 10_000 });
    expect(result.error, result.error?.message).toBeUndefined();
    return result;
  };
  const json = (name: string, value: unknown) => fs.writeFileSync(path.join(root, name), JSON.stringify(value), { mode: 0o600 });
  const capture = (id: string, program: string, timeout = '4000') => run('capture', root, id, '--timeout-ms', timeout, '--', process.execPath, '-e', program);
  return { root, run, json, capture };
}

function receipt(text: string) {
  expect(text.trim().startsWith('QA_EVIDENCE ')).toBe(true);
  return JSON.parse(text.trim().slice('QA_EVIDENCE '.length));
}

test('native capture executes once, preserves exact JSON and stderr, and materializes without transcription', () => {
  const f = fixture();
  const observed = { stateRoot: '/home/runner/.cache/owned', windows: 'C:\\owned\\a.json', text: '雪\n\t"\\', rows: ['abc'.repeat(20000)], value: 7, absent: null };
  f.json('payload.json', observed);
  const result = f.capture('001', `const fs = require('node:fs'); fs.appendFileSync('effects', 'once'); process.stderr.write('diagnostic\\n'); console.log(fs.readFileSync('payload.json', 'utf8'));`);
  expect(result.status, result.stderr).toBe(0);
  const captured = receipt(result.stdout);
  expect(captured).toMatchObject({ action: 'capture', status: 'complete', id: '001', exitCode: 0 });
  expect(result.stdout).not.toContain('stateRoot');
  expect(result.stderr).toBe('');
  expect(fs.readFileSync(path.join(f.root, 'effects'), 'utf8')).toBe('once');
  expect(fs.readFileSync(path.join(f.root, '.qa-evidence/001/stdout'), 'utf8')).toBe(JSON.stringify(observed) + '\n');
  expect(fs.readFileSync(path.join(f.root, '.qa-evidence/001/stderr'), 'utf8')).toBe('diagnostic\n');
  f.json('intent.json', { capture: '001', observationCommand: 'first native command', hypothesis: 'The successful boundary suggests testing the rejected input next.', nextCommand: 'second native command' });
  const checkpoint = f.run('checkpoint', f.root, '001', 'intent.json');
  expect(checkpoint.status, checkpoint.stderr).toBe(0);
  expect(receipt(checkpoint.stdout)).toMatchObject({ action: 'checkpoint', status: 'complete', id: '001' });
  expect(JSON.parse(fs.readFileSync(path.join(f.root, 'exploration-001.json'), 'utf8'))).toEqual({
    observationCommand: 'first native command', observed, hypothesis: 'The successful boundary suggests testing the rejected input next.', nextCommand: 'second native command',
  });
  f.json('annotations.json', { revision: 'revision', runtime: 'runtime', cwd: f.root, limits: ['Only the declared contract was checked.'], evidence: [{ capture: '001', command: 'first native command', contract: 'README.md', expected: 'Declared exact result', classification: 'pass' }], learning: ['001'] });
  const report = f.run('materialize', f.root, 'annotations.json');
  expect(report.status, report.stderr).toBe(0);
  const final = JSON.parse(fs.readFileSync(path.join(f.root, 'evidence.json'), 'utf8'));
  expect(final.evidence).toEqual([{ command: 'first native command', contract: 'README.md', expected: 'Declared exact result', classification: 'pass', observed }]);
  expect(final.learning).toEqual([{ observationCommand: 'first native command', hypothesis: 'The successful boundary suggests testing the rejected input next.', nextCommand: 'second native command' }]);
  expect(fs.readFileSync(path.join(f.root, 'effects'), 'utf8')).toBe('once');
  expect(f.capture('001', `require('node:fs').appendFileSync('effects', 'twice')`).status).toBe(2);
  expect(fs.readFileSync(path.join(f.root, 'effects'), 'utf8')).toBe('once');
  expect(f.run('checkpoint', f.root, '001', 'intent.json').status).toBe(2);
  expect(f.run('materialize', f.root, 'annotations.json').status).toBe(2);
  if (process.platform !== 'win32') {
    for (const name of ['exploration-001.json', 'evidence.json', '.qa-evidence/001/stdout', '.qa-evidence/001/stderr', '.qa-evidence/001/receipt.json']) expect(fs.statSync(path.join(f.root, name)).mode & 0o777).toBe(0o600);
    expect(fs.statSync(path.join(f.root, '.qa-evidence')).mode & 0o777).toBe(0o700);
    expect(fs.statSync(path.join(f.root, '.qa-evidence/001')).mode & 0o777).toBe(0o700);
  }
});

test('complete nonzero commands retain their actual exit, and non-JSON observations retain all bytes', () => {
  const f = fixture();
  const result = f.capture('001', `process.stdout.write('  exact\\ntext\\n'); process.stderr.write('separate warning\\n'); process.exitCode = 69;`);
  expect(result.status).toBe(69);
  expect(receipt(result.stdout)).toMatchObject({ status: 'complete', exitCode: 69 });
  f.json('intent.json', { capture: '001', observationCommand: 'dependency check', hypothesis: 'The unavailable dependency leaves an independent valid contract to check.', nextCommand: 'adjacent check' });
  expect(f.run('checkpoint', f.root, '001', 'intent.json').status).toBe(0);
  expect(JSON.parse(fs.readFileSync(path.join(f.root, 'exploration-001.json'), 'utf8')).observed).toBe('  exact\ntext\n');
});

test('expired and timed-out captures cannot produce checkpoint observations', () => {
  const f = fixture();
  const result = f.capture('001', `console.log('{}'); setInterval(() => {}, 1000);`, '100');
  expect(result.status).toBe(124);
  expect(receipt(result.stdout)).toMatchObject({ status: 'incomplete', exitCode: 124 });
  f.json('intent.json', { capture: '001', observationCommand: 'timed-out check', hypothesis: 'An incomplete capture must never be promoted to passing evidence.', nextCommand: 'next check' });
  expect(f.run('checkpoint', f.root, '001', 'intent.json').status).toBe(2);
  expect(fs.existsSync(path.join(f.root, 'exploration-001.json'))).toBe(false);
});

test('capture shares a working-directory-relative deadline without resetting or relocating it', () => {
  const f = fixture();
  fs.mkdirSync(path.join(f.root, 'reports'));
  const guard = spawnSync(process.execPath, [path.resolve(import.meta.dir, '../bin/gstack-qa-deadline'), 'start', 'reports/deadline.json', '5'],
    { cwd: f.root, encoding: 'utf8', timeout: 10000 });
  expect(guard.status, guard.stderr).toBe(0);
  const before = fs.readFileSync(path.join(f.root, 'reports/deadline.json'));
  const result = f.run('capture', 'reports', '001', '--deadline', 'reports/deadline.json', '--', process.execPath, '-e', 'console.log("{}")');
  expect(result.status, result.stderr).toBe(0);
  expect(receipt(result.stdout)).toMatchObject({ status: 'complete', exitCode: 0 });
  expect(fs.readFileSync(path.join(f.root, 'reports/deadline.json'))).toEqual(before);
  expect(fs.existsSync(path.join(f.root, 'reports/.qa-evidence/001/deadline.json'))).toBe(false);
});

test.each(['stdout', 'stderr', 'receipt.json'])('changed captured %s fails closed', name => {
  const f = fixture();
  expect(f.capture('001', `console.log(JSON.stringify({ value: 7 }));`).status).toBe(0);
  fs.appendFileSync(path.join(f.root, '.qa-evidence/001', name), 'changed');
  f.json('intent.json', { capture: '001', observationCommand: 'first command', hypothesis: 'The next command checks an adjacent boundary with retained evidence.', nextCommand: 'next command' });
  expect(f.run('checkpoint', f.root, '001', 'intent.json').status).toBe(2);
  expect(fs.existsSync(path.join(f.root, 'exploration-001.json'))).toBe(false);
});

test.each(['intent-link', 'capture-link', 'hard-link', 'outside-root', 'extra-observed', 'missing-capture'])('unowned or fabricated source fails closed: %s', mode => {
  const f = fixture();
  expect(f.capture('001', `console.log('{}');`).status).toBe(0);
  const intent: Record<string, unknown> = { capture: mode === 'missing-capture' ? '002' : '001', observationCommand: 'first command', hypothesis: 'The next command tests a different boundary without rewriting observations.', nextCommand: 'next command' };
  if (mode === 'extra-observed') intent.observed = { invented: true };
  f.json('intent.json', intent);
  let source = 'intent.json';
  if (mode === 'intent-link') { fs.symlinkSync(path.join(f.root, source), path.join(f.root, 'linked.json')); source = 'linked.json'; }
  if (mode === 'hard-link') fs.linkSync(path.join(f.root, source), path.join(f.root, 'linked.json'));
  if (mode === 'capture-link') { fs.renameSync(path.join(f.root, '.qa-evidence/001/stdout'), path.join(f.root, 'original')); fs.symlinkSync(path.join(f.root, 'original'), path.join(f.root, '.qa-evidence/001/stdout')); }
  if (mode === 'outside-root') source = '../outside.json';
  expect(f.run('checkpoint', f.root, '001', source).status).toBe(2);
  expect(fs.existsSync(path.join(f.root, 'exploration-001.json'))).toBe(false);
});

test('explicit public capture returns exact safe observations and inline intent preserves the four-field checkpoint', () => {
  const f = fixture();
  const observed = { stateRoot: '/owned/.cache/project', value: '雪\\path\n' };
  const captured = f.run('capture', f.root, '001', '--public', '--timeout-ms', '4000', '--', process.execPath, '-e', `console.log(${JSON.stringify(JSON.stringify(observed))})`);
  expect(captured.status, captured.stderr).toBe(0);
  expect(JSON.parse(captured.stdout.split('\n')[0])).toEqual(observed);
  expect(receipt(captured.stdout.split('\n').find(line => line.startsWith('QA_EVIDENCE '))!)).toMatchObject({ publicOutput: true, status: 'complete' });
  expect(f.run('checkpoint', f.root, '001', '001', 'observed command', 'The completed public result suggests checking another boundary.', 'next command').status).toBe(0);
  const note = JSON.parse(fs.readFileSync(path.join(f.root, 'exploration-001.json'), 'utf8'));
  expect(Object.keys(note).sort()).toEqual(['hypothesis', 'nextCommand', 'observationCommand', 'observed']);
  expect(note.observed).toEqual(observed);
});

test('public permission never exposes detected credentials in output or receipts', () => {
  const f = fixture();
  const credential = ['gh', 'p_'].join('') + randomBytes(18).toString('hex');
  const result = f.run('capture', f.root, '001', '--public', '--timeout-ms', '4000', '--', process.execPath, '-e', `console.log(JSON.stringify({ credential: ${JSON.stringify(credential)} }));`);
  expect(result.status).toBe(2);
  expect(receipt(result.stdout)).toMatchObject({ status: 'sensitive', publicOutput: true });
  expect(result.stdout + result.stderr).not.toContain(credential);
  expect(fs.existsSync(path.join(f.root, '.qa-evidence/001/observation.json'))).toBe(false);
  expect(f.run('checkpoint', f.root, '001', '001', 'sensitive command', 'This cannot be promoted into a safe observation or passing checkpoint.', 'next command').status).toBe(2);
});

test.each(['resumed', 'blocked-forever'])('evidence receipts survive a genuinely %s stdout pipe without extending command execution', async mode => {
  const f = fixture();
  const preload = path.join(f.root, 'full-pipe.ts');
  fs.writeFileSync(preload, `import { write } from 'node:fs'; if (process.argv[1] === ${JSON.stringify(CLI)}) { write(1, Buffer.alloc(2 * 1024 * 1024, 32), () => {}); await Bun.sleep(100); }`);
  const child = spawn(process.execPath, ['--preload', preload, CLI, 'capture', f.root, '001', '--timeout-ms', '1000', '--', process.execPath, '-e', `require('node:fs').writeFileSync(${JSON.stringify(path.join(f.root, 'effect'))}, 'once'); process.exit(69);`], { cwd: f.root, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stdout!.on('data', bytes => { stdout += bytes; });
  child.stderr!.on('data', bytes => { stderr += bytes; });
  child.stdout!.pause();
  const finished = new Promise<number | null>(resolve => child.once('exit', resolve));
  const closed = new Promise<void>(resolve => child.once('close', () => resolve()));
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const file = path.join(f.root, '.qa-evidence/001/receipt.json');
    for (let index = 0; index < 300 && !fs.existsSync(file); index++) await Bun.sleep(10);
    expect(fs.existsSync(file)).toBe(true);
    expect(JSON.parse(fs.readFileSync(file, 'utf8'))).toMatchObject({ status: 'complete', exitCode: 69 });
    expect(fs.readFileSync(path.join(f.root, 'effect'), 'utf8')).toBe('once');
    expect(child.exitCode).toBeNull();
    if (mode === 'resumed') child.stdout!.resume();
    const exit = await Promise.race([finished, new Promise<null>(resolve => { timer = setTimeout(() => resolve(null), 7000); })]);
    expect(exit, stderr).toBe(mode === 'resumed' ? 69 : 2);
    child.stdout!.resume();
    await closed;
    if (mode === 'resumed') expect(receipt(stdout.split('\n').find(line => line.startsWith('QA_EVIDENCE '))!)).toMatchObject({ status: 'complete', exitCode: 69 });
  } finally {
    clearTimeout(timer);
    child.stdout!.resume();
    child.kill('SIGKILL');
    await closed;
  }
}, 15_000);

test.each(['early', 'timeout'])('capture closes owned descendants and their output handles after %s completion', async mode => {
  const f = fixture();
  const leaf = path.join(f.root, 'leaf.ts');
  const driver = path.join(f.root, 'driver.ts');
  const pidFile = path.join(f.root, 'leaf.pid');
  fs.writeFileSync(leaf, `require('node:fs').writeFileSync(process.argv[2], String(process.pid)); console.log('leaf ready'); setInterval(() => {}, 1000);`);
  fs.writeFileSync(driver, `import { spawn } from 'node:child_process'; import { existsSync } from 'node:fs'; const child = spawn(process.execPath, [${JSON.stringify(leaf)}, ${JSON.stringify(pidFile)}], { stdio: 'inherit' }); child.unref(); const until = Date.now() + 3000; while (!existsSync(${JSON.stringify(pidFile)})) { if (Date.now() > until) process.exit(11); await Bun.sleep(10); } ${mode === 'early' ? 'process.exit(7);' : 'setInterval(() => {}, 1000);'}`);
  let pid = 0;
  const alive = () => { try { process.kill(pid, 0); return true; } catch { return false; } };
  try {
    const result = f.run('capture', f.root, '001', '--timeout-ms', '1000', '--', process.execPath, driver);
    expect(result.status, result.stderr).toBe(mode === 'early' ? 7 : 124);
    expect(receipt(result.stdout)).toMatchObject({ status: mode === 'early' ? 'complete' : 'incomplete' });
    pid = Number(fs.readFileSync(pidFile, 'utf8'));
    for (let index = 0; index < 100 && alive(); index++) await Bun.sleep(20);
    expect(alive()).toBe(false);
  } finally { if (pid && alive()) process.kill(pid, 'SIGKILL'); }
});

test('UTF-8 capture retains a BOM and control bytes rather than silently changing non-JSON output', () => {
  const f = fixture();
  const value = '\ufeffnative\u0000text\r\n';
  expect(f.capture('001', `process.stdout.write(${JSON.stringify(value)});`).status).toBe(0);
  expect(f.run('checkpoint', f.root, '001', '001', 'native command', 'The exact text suggests checking the next declared input boundary.', 'next command').status).toBe(0);
  expect(JSON.parse(fs.readFileSync(path.join(f.root, 'exploration-001.json'), 'utf8')).observed).toBe(value);
});

test('invalid UTF-8 is retained privately as incomplete, never reconstructed as text', () => {
  const f = fixture();
  const result = f.capture('001', 'process.stdout.write(Buffer.from([0xff, 0x80]));');
  expect(result.status).toBe(2);
  expect(receipt(result.stdout)).toMatchObject({ status: 'incomplete', exitCode: 0 });
  expect([...fs.readFileSync(path.join(f.root, '.qa-evidence/001/stdout'))]).toEqual([255, 128]);
  expect(f.run('checkpoint', f.root, '001', '001', 'native command', 'An incomplete text capture cannot qualify as an observed result.', 'next command').status).toBe(2);
});

test.skipIf(process.platform === 'win32')('unavailable Windows containment refuses the command before dispatch', () => {
  const f = fixture();
  const preload = path.join(f.root, 'unavailable.ts');
  const marker = path.join(f.root, 'effect');
  fs.writeFileSync(preload, `Object.defineProperty(process, 'platform', { value: 'win32' });`);
  const result = spawnSync(process.execPath, ['--preload', preload, CLI, 'capture', f.root, '001', '--timeout-ms', '1000', '--', process.execPath, '-e', `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'unexpected')`], { cwd: f.root, encoding: 'utf8', timeout: 10000 });
  expect(result.status).toBe(2);
  expect(fs.existsSync(marker)).toBe(false);
});

test.skipIf(process.platform !== 'win32')('abrupt Windows evidence-wrapper exit closes the inherited job', async () => {
  const f = fixture();
  const marker = path.join(f.root, 'native.pid');
  const child = spawn(process.execPath, [CLI, 'capture', f.root, '001', '--timeout-ms', '10000', '--', process.execPath, '-e', `require('node:fs').writeFileSync(${JSON.stringify(marker)}, String(process.pid)); setInterval(() => {}, 1000);`], { cwd: f.root, stdio: 'ignore' });
  const closed = new Promise<void>(resolve => child.once('close', () => resolve()));
  let pid = 0;
  const alive = () => { try { process.kill(pid, 0); return true; } catch { return false; } };
  try {
    for (let index = 0; index < 300 && !fs.existsSync(marker); index++) await Bun.sleep(10);
    expect(fs.existsSync(marker)).toBe(true);
    pid = Number(fs.readFileSync(marker, 'utf8'));
    child.kill('SIGKILL');
    await closed;
    for (let index = 0; index < 100 && alive(); index++) await Bun.sleep(20);
    expect(alive()).toBe(false);
  } finally { child.kill('SIGKILL'); if (pid && alive()) process.kill(pid, 'SIGKILL'); await closed; }
});
