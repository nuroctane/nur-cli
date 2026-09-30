import { describe, expect, spyOn, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { startQaDeadline, readQaDeadline } from '../lib/qa-deadline';
import { observeQAWrites, type QAWriteObservation } from './helpers/qa-functional-observer';

const linkFile = fs.linkSync;

async function publication(directory: string, during?: (root: string, temporary: string, target: string,
  observer: Awaited<ReturnType<typeof observeQAWrites>>) => QAWriteObservation | void) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'qa-publish-')));
  fs.chmodSync(root, 0o700);
  for (const name of ['.qa-state', 'qa-reports', 'reports', 'src']) fs.mkdirSync(path.join(root, name), { mode: 0o700 });
  const observer = await observeQAWrites(root);
  const target = path.join(root, directory, 'deadline.json');
  const link = fs.linkSync;
  let observation: QAWriteObservation | undefined;
  let temporary = '';
  let linksDuring = 0;
  const intercept = spyOn(fs, 'linkSync').mockImplementation((from, to) => {
    link(from, to);
    temporary = String(from);
    linksDuring = fs.lstatSync(to).nlink;
    observation = during?.(root, temporary, String(to), observer) || undefined;
    if (!observation) observer.drain();
  });
  try {
    const state = startQaDeadline(target, '60');
    intercept.mockRestore();
    observation ??= observer.stop();
    return { observation, linksDuring, temporary: path.relative(root, temporary),
      linksAfter: fs.lstatSync(target).nlink, state, read: readQaDeadline(target) };
  } finally {
    intercept.mockRestore();
    if (!observation) observer.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
}

(process.platform === 'linux' ? describe : describe.skip)('deadline publication through the registered kernel observer', () => {
  for (const directory of ['reports', 'qa-reports', '.qa-state']) {
    test(`accepts only the transient real publication in ${directory}`, async () => {
      const result = await publication(directory);
      expect(result.linksDuring).toBe(2);
      expect(result.linksAfter).toBe(1);
      expect(result.read).toEqual(result.state);
      expect(result.observation.failures).toEqual([]);
      expect(result.observation.complete).toBe(true);
      expect(result.observation.events.some(event => event.path === `${directory}/deadline.json` && (event.mask & 0x100))).toBe(true);
      expect(result.observation.events.some(event => event.path === result.temporary && (event.mask & 0x200))).toBe(true);
    });
  }

  test('does not exempt a correctly named source-directory hardlink pair', async () => {
    const result = await publication('src');
    expect(result.observation.complete).toBe(false);
    expect(result.observation.failures.some(failure => /path=src\/.*dev=\d+ ino=\d+ nlink=2/.test(failure))).toBe(true);
  });

  for (const mutation of ['source-hardlink', 'third-alias', 'wrong-name', 'different-directory', 'different-inode', 'writable', 'malformed', 'extra-field', 'invalid-time', 'invalid-budget'] as const) {
    test(`rejects ${mutation} during the real publication even after cleanup`, async () => {
      const result = await publication('qa-reports', (root, temporary, target, observer) => {
        const bytes = fs.readFileSync(target);
        const alias = path.join(root, mutation === 'source-hardlink' ? 'src' : mutation === 'different-directory' ? 'reports' : 'qa-reports',
          mutation === 'different-directory' ? path.basename(temporary) : 'untrusted-alias');
        if (mutation === 'source-hardlink' || mutation === 'third-alias') linkFile(target, alias);
        if (mutation === 'wrong-name' || mutation === 'different-directory') fs.renameSync(temporary, alias);
        if (mutation === 'different-inode') {
          fs.unlinkSync(temporary);
          fs.writeFileSync(temporary, bytes, { mode: 0o400 });
          linkFile(target, alias);
        }
        if (mutation === 'writable') fs.chmodSync(target, 0o600);
        if (['malformed', 'extra-field', 'invalid-time', 'invalid-budget'].includes(mutation)) {
          const state = JSON.parse(bytes.toString());
          if (mutation === 'extra-field') state.extra = true;
          if (mutation === 'invalid-time') state.startedAt = 'tomorrow';
          if (mutation === 'invalid-budget') state.budgetMs = 0;
          fs.chmodSync(target, 0o600);
          fs.writeFileSync(target, mutation === 'malformed' ? '{' : JSON.stringify(state));
          fs.chmodSync(target, 0o400);
        }
        observer.drain();
        if (mutation === 'wrong-name' || mutation === 'different-directory') fs.renameSync(alias, temporary);
        if (mutation === 'source-hardlink' || mutation === 'third-alias' || mutation === 'different-inode') fs.unlinkSync(alias);
        if (mutation === 'different-inode') { fs.unlinkSync(temporary); linkFile(target, temporary); }
        fs.chmodSync(target, 0o600);
        fs.writeFileSync(target, bytes);
        fs.chmodSync(target, 0o400);
      });
      expect(result.observation.complete).toBe(false);
      expect(result.observation.failures.some(failure => /path=qa-reports\/.*dev=\d+ ino=\d+ nlink=[23]/.test(failure))).toBe(true);
      expect(result.linksAfter).toBe(1);
    });
  }

  test('rejects an external symlink in place of the deadline temporary', async () => {
    const external = fs.mkdtempSync(path.join(os.tmpdir(), 'qa-external-'));
    const witness = path.join(external, 'witness');
    fs.writeFileSync(witness, 'external');
    try {
      const result = await publication('reports', (_root, temporary, target, observer) => {
        fs.unlinkSync(temporary);
        fs.symlinkSync(witness, temporary);
        observer.drain();
        fs.unlinkSync(temporary);
        linkFile(target, temporary);
      });
      expect(result.observation.complete).toBe(false);
      expect(result.observation.failures.some(failure => failure.includes('Fixture path traverses a link') && failure.includes('path=reports/.qa-deadline-'))).toBe(true);
      expect(fs.readFileSync(witness, 'utf8')).toBe('external');
    } finally { fs.rmSync(external, { recursive: true, force: true }); }
  });

  for (const mutation of ['persistent-pair', 'new-inode', 'changed-state', 'writable-final'] as const) {
    test(`requires final one-link immutable settlement: ${mutation}`, async () => {
      const result = await publication('reports', (_root, temporary, target, observer) => {
        observer.drain();
        if (mutation !== 'persistent-pair') fs.unlinkSync(temporary);
        if (mutation === 'new-inode') {
          const bytes = fs.readFileSync(target);
          fs.writeFileSync(target + '.replacement', bytes, { mode: 0o400 });
          fs.renameSync(target + '.replacement', target);
        }
        if (mutation === 'changed-state') {
          const state = JSON.parse(fs.readFileSync(target, 'utf8'));
          state.budgetMs += 1;
          fs.chmodSync(target, 0o600);
          fs.writeFileSync(target, JSON.stringify(state));
          fs.chmodSync(target, 0o400);
        }
        if (mutation === 'writable-final') fs.chmodSync(target, 0o600);
        return observer.stop();
      });
      expect(result.observation.complete).toBe(false);
      expect(result.observation.failures.some(failure => /path=reports\/deadline.json.*dev=\d+ ino=\d+ nlink=[12]/.test(failure))).toBe(true);
    });
  }

  test('does not hide a lost directory watch behind an authenticated publication', async () => {
    const result = await publication('reports', (root, _temporary, _target, observer) => {
      observer.drain();
      fs.renameSync(path.join(root, 'reports'), path.join(root, 'moved-reports'));
      fs.renameSync(path.join(root, 'moved-reports'), path.join(root, 'reports'));
    });
    expect(result.observation.complete).toBe(false);
    expect(result.observation.failures).toContain('watch target moved or unmounted: reports');
  });
});
