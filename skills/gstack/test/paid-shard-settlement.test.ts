import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawnSync } from 'node:child_process';

const root = path.resolve(import.meta.dir, '..');

test.each(['complete', 'delayed', 'stalled', 'closed', 'error'])('paid spool settlement through the registered caller: %s', scenario => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'paid-spool-'));
  try {
    const script = `
import fs from 'node:fs';
import { EventEmitter } from 'node:events';
import { mock } from 'bun:test';
const scenario = ${JSON.stringify(scenario)};
const directory = ${JSON.stringify(dir)};
let opened = 0, ended = 0, destroyed = 0;
mock.module('node:fs', () => ({ ...fs, createWriteStream(filename) {
  opened++;
  const fd = fs.openSync(filename, 'wx', 0o600);
  let closed = false;
  const stream = new EventEmitter();
  stream.writableFinished = false;
  stream.destroyed = false;
  stream.write = chunk => { fs.writeSync(fd, chunk); return true; };
  stream.destroy = () => {
    if (!closed) { fs.closeSync(fd); closed = true; destroyed++; }
    stream.destroyed = true;
    queueMicrotask(() => stream.emit('close'));
    return stream;
  };
  stream.end = callback => {
    ended++;
    const finish = () => {
      stream.writableFinished = true;
      stream.emit('finish');
      callback?.();
      stream.destroy();
    };
    if (scenario === 'complete') queueMicrotask(finish);
    if (scenario === 'delayed') setTimeout(finish, 25);
    if (scenario === 'closed') stream.destroy();
    if (scenario === 'error') queueMicrotask(() => stream.emit('error', new Error('fixture spool failure')));
    return stream;
  };
  return stream;
}}));
const { runPaidShards } = await import(${JSON.stringify(path.join(root, 'scripts/test-paid-shards.ts'))});
const lines = [];
const started = Date.now();
const result = await runPaidShards([['spool-control']], {
  rootDir: ${JSON.stringify(root)}, timeoutMs: 1_000, jobs: 1, logDir: directory,
  commandFor: () => ({ command: process.execPath, args: ['-e', 'console.log("retained evidence"); console.log("Ran 1 tests across 1 files. [1ms]")'] }),
  log: line => lines.push(line),
});
console.log('SETTLEMENT_RESULT:' + JSON.stringify({ result, elapsed: Date.now() - started, opened, ended, destroyed, lines }));
`;
    const child = spawnSync(process.execPath, ['-e', script], {
      cwd: root, encoding: 'utf8', timeout: 10_000,
      env: { ...process.env, EVALS: '', EVALS_TIER: '', EVALS_SHARD_TIMEOUT_MS: '' },
    });
    expect(child.error, child.stderr).toBeUndefined();
    expect(child.status, child.stderr).toBe(0);
    const line = child.stdout.split('\n').find(line => line.startsWith('SETTLEMENT_RESULT:'));
    expect(line).toBeDefined();
    const facts = JSON.parse(line!.slice('SETTLEMENT_RESULT:'.length));
    expect(facts.opened).toBe(1);
    expect(facts.ended).toBe(1);
    expect(facts.destroyed).toBe(1);
    expect(facts.elapsed).toBeLessThan(2_500);
    expect(facts.result.executed).toBe(1);
    expect(facts.result.neverStarted).toBe(0);
    expect(facts.result.outcomes[0].status).toBe(
      ['complete', 'delayed'].includes(scenario) ? 'passed' : scenario === 'stalled' ? 'timed-out' : 'failed',
    );
    const logs = fs.readdirSync(dir).filter(name => name.endsWith('.log'));
    expect(logs).toHaveLength(1);
    expect(fs.readFileSync(path.join(dir, logs[0]), 'utf8')).toBe('retained evidence\nRan 1 tests across 1 files. [1ms]\n');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}, 30_000);
