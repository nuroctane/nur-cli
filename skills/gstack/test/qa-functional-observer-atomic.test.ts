import { test, expect } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawn } from 'node:child_process';
import { observeQAWrites, qaWriteVerdict } from './helpers/qa-functional-observer';

function ownedRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'qa-observer-'));
  fs.chmodSync(root, 0o700);
  for (const directory of ['.qa-state', 'qa-reports', 'src', 'test']) fs.mkdirSync(path.join(root, directory), { mode: 0o700 });
  return root;
}

test('observes permitted atomic state and report replacements without losing their directory watches', async () => {
  const root = ownedRoot();
  fs.mkdirSync(path.join(root, '.qa-state', 'partial'), { mode: 0o700 });
  const observer = await observeQAWrites(root);
  let stopped = false;
  try {
    const child = spawn(process.execPath, ['-e', `
      const fs = require('node:fs');
      const path = require('node:path');
      const directory = path.join(process.argv[1], '.qa-state', 'partial');
      for (let i = 0; i < 600; i++) {
        const temporary = path.join(directory, 'ledger.tmp');
        fs.writeFileSync(temporary, '{}', { mode: 0o600 });
        fs.renameSync(temporary, path.join(directory, 'ledger.json'));
      }
    `, root], { stdio: 'ignore', timeout: 10_000 });
    const exit = await new Promise<number | null>((resolve, reject) => {
      child.once('error', reject);
      child.once('close', resolve);
    });
    expect(exit).toBe(0);

    const report = path.join(root, 'qa-reports', 'report.md');
    fs.writeFileSync(report, 'first', { mode: 0o600 });
    observer.drain();
    const temporary = report + '.tmp';
    fs.writeFileSync(temporary, 'second', { mode: 0o600 });
    fs.renameSync(temporary, report);
    const observation = observer.stop();
    stopped = true;

    expect(qaWriteVerdict(observation, 'qa-only')).toEqual([]);
    expect(observation.events).toContainEqual(expect.objectContaining({ path: '.qa-state/partial/ledger.tmp', mask: 0x100 }));
    expect(observation.events).toContainEqual(expect.objectContaining({ path: '.qa-state/partial/ledger.json', mask: 0x80 }));
    expect(observation.events).toContainEqual(expect.objectContaining({ path: 'qa-reports/report.md', mask: 0x80 }));
    expect(observation.events).toContainEqual(expect.objectContaining({ path: 'qa-reports/report.md.tmp', mask: 0x2 }));
    expect(fs.readFileSync(report, 'utf8')).toBe('second');
    expect(observation.after['.qa-state/partial/ledger.json']).toBeDefined();
  } finally {
    if (!stopped) observer.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

for (const relative of ['src/cli.ts', 'test/amount.regression-1.test.ts']) {
  test(`observes native atomic replacement of ${relative} without losing coverage`, async () => {
    const root = ownedRoot();
    const target = path.join(root, relative);
    if (relative.startsWith('src/')) fs.writeFileSync(target, 'original', { mode: 0o600 });
    const observer = await observeQAWrites(root);
    let stopped = false;
    try {
      const temporary = target + '.tmp.201.native';
      fs.writeFileSync(temporary, 'replacement', { mode: 0o600 });
      observer.drain();
      fs.renameSync(temporary, target);
      observer.drain();
      fs.appendFileSync(target, '-written-after-rename');
      const observation = observer.stop();
      stopped = true;

      expect(observation.events).toContainEqual(expect.objectContaining({ path: relative + '.tmp.201.native', mask: 0x40 }));
      expect(observation.events).toContainEqual(expect.objectContaining({ path: relative, mask: 0x80 }));
      expect(observation.events).toContainEqual(expect.objectContaining({ path: relative, mask: 0x800 }));
      const moved = observation.events.findIndex(event => event.path === relative && event.mask === 0x800);
      expect(observation.events.slice(moved + 1)).toContainEqual(expect.objectContaining({ path: relative, mask: 0x2 }));
      expect(observation.changed).toContain(relative);
      expect(fs.readFileSync(target, 'utf8')).toBe('replacement-written-after-rename');
      expect(observation.complete).toBe(true);
      expect(qaWriteVerdict(observation, 'qa')).toEqual([]);
      expect(qaWriteVerdict(observation, 'qa-only')).toContain(`forbidden qa-only write: ${relative}`);
    } finally {
      if (!stopped) observer.stop();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
}

for (const relative of ['src/core.ts', 'test/core.test.ts']) {
  test(`retains the inode watch for ${relative} moved into allowed state and restored`, async () => {
    const root = ownedRoot();
    const source = path.join(root, relative);
    fs.writeFileSync(source, 'original', { mode: 0o600 });
    const observer = await observeQAWrites(root);
    let stopped = false;
    const fd = fs.openSync(source, 'r+');
    try {
      const moved = path.join(root, '.qa-state', 'moved');
      fs.renameSync(source, moved);
      observer.drain();
      fs.writeSync(fd, Buffer.from('changed!'), 0, 8, 0);
      observer.drain();
      fs.writeSync(fd, Buffer.from('original'), 0, 8, 0);
      fs.renameSync(moved, source);
      const observation = observer.stop();
      stopped = true;

      expect(observation.before[relative]).toBe(observation.after[relative]);
      expect(observation.changed).not.toContain(relative);
      const movedIndex = observation.events.findIndex(event => event.path === relative && event.mask === 0x800);
      expect(movedIndex).toBeGreaterThanOrEqual(0);
      expect(observation.events.slice(movedIndex + 1)).toContainEqual(expect.objectContaining({ path: relative, mask: 0x2 }));
      expect(observation.complete).toBe(true);
      expect(qaWriteVerdict(observation, 'qa')).toEqual([]);
      expect(qaWriteVerdict(observation, 'qa-only')).toContain(`forbidden qa-only write: ${relative}`);
    } finally {
      fs.closeSync(fd);
      if (!stopped) observer.stop();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
}

for (const relative of ['', 'src']) {
  test(`still fails closed when the ${relative || 'root'} directory moves and returns`, async () => {
    const root = ownedRoot();
    const directory = relative ? path.join(root, relative) : root;
    const observer = await observeQAWrites(root);
    let stopped = false;
    try {
      fs.renameSync(directory, directory + '.moved');
      fs.renameSync(directory + '.moved', directory);
      const observation = observer.stop();
      stopped = true;

      expect(observation.complete).toBe(false);
      expect(observation.failures.some(failure => failure.startsWith('watch target moved or unmounted:'))).toBe(true);
      expect(observation.events).toContainEqual(expect.objectContaining({ path: relative, mask: 0x800 }));
      expect(qaWriteVerdict(observation, 'qa')).toContain('incomplete write observation');
    } finally {
      if (!stopped) observer.stop();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
}

test('still fails closed when a new directory vanishes before its contents can be watched', async () => {
  const root = ownedRoot();
  const observer = await observeQAWrites(root);
  let stopped = false;
  try {
    const directory = path.join(root, '.qa-state', 'gap');
    fs.mkdirSync(directory);
    fs.writeFileSync(path.join(directory, 'unobserved'), 'changed');
    fs.rmSync(directory, { recursive: true });
    const observation = observer.stop();
    stopped = true;

    expect(observation.complete).toBe(false);
    expect(observation.failures).toContain('new directory vanished before watch: .qa-state/gap');
    expect(qaWriteVerdict(observation, 'qa')).toContain('incomplete write observation');
  } finally {
    if (!stopped) observer.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('still fails closed for an unmount event on a watched leaf inode', async () => {
  const root = ownedRoot();
  const source = path.join(root, 'src', 'core.ts');
  fs.writeFileSync(source, 'original', { mode: 0o600 });
  const observer = await observeQAWrites(root);
  let stopped = false;
  try {
    const inode = fs.statSync(source).ino.toString(16);
    const watches = fs.readdirSync('/proc/self/fdinfo').flatMap(fd => {
      try { return fs.readFileSync(`/proc/self/fdinfo/${fd}`, 'utf8').split('\n'); }
      catch { return []; }
    });
    const watch = watches.find(line => line.startsWith('inotify wd:') && line.includes(` ino:${inode} `));
    expect(watch).toBeDefined();
    const record = Buffer.alloc(16);
    record.writeInt32LE(parseInt(watch!.match(/^inotify wd:([0-9a-f]+)/)![1], 16), 0);
    record.writeUInt32LE(0x2000, 4);
    observer.injectKernelRecordsForTest(record);
    const observation = observer.stop();
    stopped = true;

    expect(observation.complete).toBe(false);
    expect(observation.failures).toContain('watch target moved or unmounted: src/core.ts');
    expect(observation.events).toContainEqual(expect.objectContaining({ path: 'src/core.ts', mask: 0x2000 }));
    expect(qaWriteVerdict(observation, 'qa')).toContain('incomplete write observation');
  } finally {
    if (!stopped) observer.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

for (const mutation of ['write-restore', 'rename', 'delete', 'hardlink', 'symlink'] as const) {
  test(`still rejects forbidden ${mutation} while allowing state directory writes`, async () => {
    const root = ownedRoot();
    const source = path.join(root, 'src', 'core.ts');
    fs.writeFileSync(source, 'original', { mode: 0o600 });
    const observer = await observeQAWrites(root);
    let stopped = false;
    try {
      const link = path.join(root, '.qa-state', 'link');
      if (mutation === 'write-restore') {
        fs.writeFileSync(source, 'changed');
        fs.writeFileSync(source, 'original');
      } else if (mutation === 'rename') {
        fs.renameSync(source, source + '.moved');
        fs.renameSync(source + '.moved', source);
      } else if (mutation === 'delete') {
        fs.unlinkSync(source);
        fs.writeFileSync(source, 'original', { mode: 0o600 });
      } else if (mutation === 'hardlink') {
        fs.linkSync(source, link);
        observer.drain();
        fs.writeFileSync(link, 'changed');
        fs.writeFileSync(link, 'original');
        fs.unlinkSync(link);
      } else {
        fs.symlinkSync(source, link);
        observer.drain();
      }
      const observation = observer.stop();
      const verdict = qaWriteVerdict(observation, 'qa-only');
      stopped = true;
      if (mutation === 'hardlink' || mutation === 'symlink') expect(verdict.some(failure => failure.includes('Fixture path traverses a link'))).toBe(true);
      else expect(verdict.some(failure => failure.includes('forbidden qa-only write: src/core.ts'))).toBe(true);
      if (mutation === 'hardlink') {
        expect(observation.events).toContainEqual(expect.objectContaining({ path: 'src/core.ts', mask: 0x2 }));
        expect(observation.before['src/core.ts']).toBe(observation.after['src/core.ts']);
        expect(verdict).toContain('forbidden qa-only write: src/core.ts');
      }
    } finally {
      if (!stopped) observer.stop();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
}

test('still fails closed for unavailable directory watches and malformed kernel records', async () => {
  const root = ownedRoot();
  const observer = await observeQAWrites(root);
  let stopped = false;
  try {
    const unknown = Buffer.alloc(16);
    unknown.writeInt32LE(99999, 0);
    unknown.writeUInt32LE(2, 4);
    observer.injectKernelRecordsForTest(unknown);
    observer.injectKernelRecordsForTest(Buffer.alloc(1));
    const overflow = Buffer.alloc(16);
    overflow.writeInt32LE(-1, 0);
    overflow.writeUInt32LE(0x4000, 4);
    observer.injectKernelRecordsForTest(overflow);
    fs.rmdirSync(path.join(root, 'qa-reports'));
    const verdict = qaWriteVerdict(observer.stop(), 'qa-only');
    stopped = true;
    expect(verdict).toContain('event for unknown watch');
    expect(verdict.some(failure => failure.includes('truncated kernel event'))).toBe(true);
    expect(verdict).toContain('kernel queue overflow');
    expect(verdict).toContain('directory watch lost: qa-reports');
  } finally {
    if (!stopped) observer.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});
