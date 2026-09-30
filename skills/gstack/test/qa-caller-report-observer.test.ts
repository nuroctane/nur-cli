import { describe, expect, spyOn, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { readQaDeadline, startQaDeadline } from '../lib/qa-deadline';
import { observeQAWrites, qaWriteAllowed, qaWriteVerdict } from './helpers/qa-functional-observer';

function fixture() {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'qa-report-watch-')));
  fs.chmodSync(root, 0o700);
  for (const directory of ['.qa-state', 'reports', 'reports-sibling', 'foreign/reports', 'src']) {
    fs.mkdirSync(path.join(root, directory), { recursive: true, mode: 0o700 });
  }
  return root;
}

async function atomicPublication(directory: string, declared?: string) {
  const root = fixture();
  const temporary = path.join(root, directory, 'exploration-004.json.tmp.2644.340bb6ad0afe');
  const target = path.join(root, directory, 'exploration-004.json');
  const observer = await observeQAWrites(root, { reportDirectory: declared });
  const stat = fs.lstatSync;
  let renamedAtWatch = false;
  let stopped = false;
  const hook = spyOn(fs, 'lstatSync').mockImplementation(((file: fs.PathLike, options?: any) => {
    const entry = stat(file, options);
    if (String(file) === temporary && options === undefined && !renamedAtWatch) {
      fs.renameSync(temporary, target);
      renamedAtWatch = true;
    }
    return entry;
  }) as typeof fs.lstatSync);
  try {
    fs.writeFileSync(temporary, '{"observed":"retained"}\n', { mode: 0o600 });
    observer.drain();
    if (!renamedAtWatch) fs.renameSync(temporary, target);
    hook.mockRestore();
    const observation = observer.stop();
    stopped = true;
    return { observation, renamedAtWatch, content: fs.readFileSync(target, 'utf8'),
      temporary: path.relative(root, temporary), target: path.relative(root, target) };
  } finally {
    hook.mockRestore();
    if (!stopped) observer.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
}

(process.platform === 'linux' ? describe : describe.skip)('caller-owned report directory observation', () => {
  test('observes atomic report publication without the disappearing per-file watch race', async () => {
    const result = await atomicPublication('reports', 'reports');
    expect(result.observation.failures).toEqual([]);
    expect(result.observation.complete).toBe(true);
    expect(result.renamedAtWatch).toBe(false);
    expect(result.content).toBe('{"observed":"retained"}\n');
    for (const mask of [0x100, 0x2, 0x8, 0x40]) {
      expect(result.observation.events).toContainEqual(expect.objectContaining({ path: result.temporary, mask }));
    }
    const moved = result.observation.events.find(event => event.path === result.temporary && event.mask === 0x40)!;
    expect(moved.cookie).toBeGreaterThan(0);
    expect(result.observation.events).toContainEqual(expect.objectContaining({ path: result.target, mask: 0x80, cookie: moved.cookie }));
    expect(result.observation.after[result.target]).toBeDefined();
    expect(qaWriteAllowed(result.target, 'qa-only')).toBe(false);
    expect(qaWriteAllowed(result.target, 'qa')).toBe(false);
    expect(qaWriteVerdict(result.observation, 'qa-only')).toContain(`forbidden qa-only write: ${result.target}`);
  });

  for (const [directory, declared] of [['reports', undefined], ['reports-sibling', 'reports'], ['foreign/reports', 'reports']] as const) {
    test(`retains per-file monitoring outside the declared directory: ${directory}/${declared ?? 'default'}`, async () => {
      const result = await atomicPublication(directory, declared);
      expect(result.renamedAtWatch).toBe(true);
      expect(result.observation.complete).toBe(false);
      expect(result.observation.failures.some(failure => failure.includes(`Could not watch ${result.temporary}`))).toBe(true);
      expect(qaWriteVerdict(result.observation, 'qa-only')).toContain(`forbidden qa-only write: ${result.target}`);
    });
  }

  test('supports an explicitly selected nested report directory, not an implicit reports name', async () => {
    const result = await atomicPublication('foreign/reports', 'foreign/reports');
    expect(result.observation.complete).toBe(true);
    expect(result.observation.failures).toEqual([]);
    expect(result.renamedAtWatch).toBe(false);
    expect(qaWriteAllowed(result.target, 'qa-only')).toBe(false);
  });

  for (const selected of ['.', '..', '../outside-reports', 'missing', 'src/core.ts']) {
    test(`rejects an invalid report-directory declaration: ${selected}`, async () => {
      const root = fixture();
      fs.writeFileSync(path.join(root, 'src/core.ts'), 'original', { mode: 0o600 });
      let observer: Awaited<ReturnType<typeof observeQAWrites>> | undefined;
      try {
        await expect((async () => {
          observer = await observeQAWrites(root, { reportDirectory: selected });
        })()).rejects.toThrow();
      } finally {
        observer?.stop();
        fs.rmSync(root, { recursive: true, force: true });
      }
    });
  }

  test('rejects an existing foreign directory and a linked declaration', async () => {
    const root = fixture();
    const foreign = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'qa-foreign-report-')));
    fs.chmodSync(foreign, 0o700);
    fs.symlinkSync(path.join(root, 'reports'), path.join(root, 'linked-reports'));
    try {
      await expect(observeQAWrites(root, { reportDirectory: foreign })).rejects.toThrow('escapes its owned root');
      await expect(observeQAWrites(root, { reportDirectory: 'linked-reports' })).rejects.toThrow('traverses a link');
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
      fs.rmSync(foreign, { recursive: true, force: true });
    }
  });

  test('keeps the existing authenticated deadline-publication link exception narrow', async () => {
    const root = fixture();
    const observer = await observeQAWrites(root, { reportDirectory: 'reports' });
    const link = fs.linkSync;
    let linksDuring = 0;
    let stopped = false;
    const hook = spyOn(fs, 'linkSync').mockImplementation((from, to) => {
      link(from, to);
      linksDuring = fs.lstatSync(to).nlink;
      observer.drain();
    });
    try {
      const target = path.join(root, 'reports/deadline.json');
      const state = startQaDeadline(target, '60');
      hook.mockRestore();
      const observation = observer.stop();
      stopped = true;
      expect(linksDuring).toBe(2);
      expect(fs.lstatSync(target).nlink).toBe(1);
      expect(readQaDeadline(target)).toEqual(state);
      expect(observation.complete).toBe(true);
      expect(observation.failures).toEqual([]);
      expect(qaWriteAllowed('reports/deadline.json', 'qa-only')).toBe(false);
    } finally {
      hook.mockRestore();
      if (!stopped) observer.stop();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  for (const mutation of ['vanishing-child', 'lost-root', 'moved-root'] as const) {
    test(`preserves directory monitoring for ${mutation}`, async () => {
      const root = fixture();
      const reports = path.join(root, 'reports');
      const observer = await observeQAWrites(root, { reportDirectory: 'reports' });
      let stopped = false;
      try {
        if (mutation === 'vanishing-child') {
          const child = path.join(reports, 'gap');
          fs.mkdirSync(child);
          fs.writeFileSync(path.join(child, 'unobserved'), 'changed');
          fs.rmSync(child, { recursive: true });
        } else if (mutation === 'lost-root') fs.rmdirSync(reports);
        else {
          fs.renameSync(reports, reports + '.moved');
          fs.renameSync(reports + '.moved', reports);
        }
        const observation = observer.stop();
        stopped = true;
        expect(observation.complete).toBe(false);
        expect(observation.failures).toContain(mutation === 'vanishing-child'
          ? 'new directory vanished before watch: reports/gap'
          : mutation === 'lost-root' ? 'directory watch lost: reports'
            : 'watch target moved or unmounted: reports');
      } finally {
        if (!stopped) observer.stop();
        fs.rmSync(root, { recursive: true, force: true });
      }
    });
  }

  for (const kind of ['symlink', 'hardlink', 'directory-symlink'] as const) {
    test(`does not exempt a ${kind} inside the declared report directory`, async () => {
      const root = fixture();
      const source = path.join(root, 'src/core.ts');
      fs.writeFileSync(source, 'original', { mode: 0o600 });
      const observer = await observeQAWrites(root, { reportDirectory: 'reports' });
      let stopped = false;
      try {
        const link = path.join(root, 'reports/link');
        if (kind === 'hardlink') fs.linkSync(source, link);
        else fs.symlinkSync(kind === 'directory-symlink' ? path.join(root, 'src') : source, link);
        observer.drain();
        fs.unlinkSync(link);
        const observation = observer.stop();
        stopped = true;
        expect(observation.complete).toBe(false);
        expect(observation.failures.some(failure => failure.includes('Fixture path traverses a link'))).toBe(true);
      } finally {
        if (!stopped) observer.stop();
        fs.rmSync(root, { recursive: true, force: true });
      }
    });
  }

  test('retains a forbidden source inode watch when moved through the declared report directory', async () => {
    const root = fixture();
    const source = path.join(root, 'src/core.ts');
    const moved = path.join(root, 'reports/moved');
    fs.writeFileSync(source, 'original', { mode: 0o600 });
    const observer = await observeQAWrites(root, { reportDirectory: 'reports' });
    const fd = fs.openSync(source, 'r+');
    let stopped = false;
    try {
      fs.renameSync(source, moved);
      observer.drain();
      fs.writeSync(fd, 'changed!', 0);
      observer.drain();
      fs.writeSync(fd, 'original', 0);
      fs.renameSync(moved, source);
      const observation = observer.stop();
      stopped = true;
      expect(observation.complete).toBe(true);
      expect(observation.before['src/core.ts']).toBe(observation.after['src/core.ts']);
      expect(observation.events).toContainEqual(expect.objectContaining({ path: 'src/core.ts', mask: 0x2 }));
      expect(qaWriteVerdict(observation, 'qa-only')).toContain('forbidden qa-only write: src/core.ts');
    } finally {
      fs.closeSync(fd);
      if (!stopped) observer.stop();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
});
