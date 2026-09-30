import { describe, test, expect } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as os from 'node:os';
import { createQAFunctionalFixture, fixtureCommand, fixtureGit, ownedPath, qaFixtureActor } from './helpers/qa-functional-fixture';

describe('functional QA native fixtures', () => {
  test('bounded clock stays inside native capture deadline and actor command boundary', () => {
    const source = fs.readFileSync(path.join(import.meta.dir, 'helpers/qa-functional-eval.ts'), 'utf8');
    expect(source).toContain('const timeout = Math.max(1, deadlineAt - Date.now() - SESSION_DRAIN_GRACE_MS)');
    expect(source).toContain('timeout, completionReserveMs: timeout / 4');
    expect(source).toContain('exactly date -u +%Y-%m-%dT%H:%M:%SZ');
    for (const mode of ['qa', 'qa-only'] as const) expect(qaFixtureActor(mode)).toContain('date -u +%Y-%m-%dT%H:%M:%SZ');
  });
  test('creates a committed, clean standalone repository outside the checkout', () => {
    const fixture = createQAFunctionalFixture('cli');
    try {
      expect(fixtureGit(fixture.root, ['status', '--porcelain'])).toBe('');
      expect(path.resolve(fixtureGit(fixture.root, ['rev-parse', '--show-toplevel']))).toBe(fixture.root);
      expect(fixture.revision).toMatch(/^[a-f0-9]{40}$/);
      expect(fixtureGit(fixture.root, ['config', '--local', 'user.name'])).toBe('QA Fixture');
      expect(fixtureGit(fixture.root, ['config', '--local', 'user.email'])).toBe('qa-fixture@gstack.test');
      expect(fixtureGit(fixture.root, ['config', '--local', 'commit.gpgsign'])).toBe('false');
      expect(fixtureCommand(fixture.root, ['test']).exit).toBe(0);
      expect(() => createQAFunctionalFixture('cli', { parent: path.resolve(import.meta.dir, '..') })).toThrow('outside');
    } finally { fixture.cleanup(); }
  });

  test('CLI preserves exact streams, exit codes, durable effects and idempotency', () => {
    const fixture = createQAFunctionalFixture('cli');
    try {
      for (let attempt = 0; attempt < 2; attempt++) {
        expect(fixtureCommand(fixture.root, ['src/cli.ts', 'apply', 'first', '7'])).toEqual({ exit: 0, stdout: 'balance=7\n', stderr: '', signal: null });
      }
      expect(fixtureCommand(fixture.root, ['src/cli.ts', 'balance']).stdout).toBe('balance=7\n');
      const before = fs.readFileSync(path.join(fixture.root, '.qa-state/ledger.json'), 'utf8');
      for (const args of [['apply', 'other', '0'], ['--unknown'], ['apply']]) {
        const result = fixtureCommand(fixture.root, ['src/cli.ts', ...args]);
        expect(result.exit).toBe(2);
        expect(result.stdout).toBe('');
        expect(result.stderr.length).toBeGreaterThan(0);
        expect(fs.readFileSync(path.join(fixture.root, '.qa-state/ledger.json'), 'utf8')).toBe(before);
      }
    } finally { fixture.cleanup(); }
  });

  test('unannounced CLI bug accepts a numeric prefix; healthy control rejects it without effect', () => {
    for (const healthy of [false, true]) {
      const fixture = createQAFunctionalFixture('cli', { healthy });
      try {
        const result = JSON.parse(fixtureCommand(fixture.root, ['probe.ts', 'apply', 'prefix', '7junk']).stdout);
        expect(result.exit).toBe(healthy ? 2 : 0);
        expect(result.state.effects).toEqual(healthy ? [] : [{ id: 'prefix', cents: 7 }]);
        expect(result.stdout).toBe(healthy ? '' : 'balance=7\n');
        expect(fixtureCommand(fixture.root, ['test']).exit).toBe(0);
      } finally { fixture.cleanup(); }
    }
  });

  test('cancellation waits for readiness and missing dependency is setup-only', () => {
    const fixture = createQAFunctionalFixture('cli');
    try {
      const cancelled = fixtureCommand(fixture.root, ['cancel.ts']);
      expect(cancelled.exit, JSON.stringify(cancelled)).toBe(0);
      expect(JSON.parse(cancelled.stdout)).toMatchObject({ exit: 130, stdout: 'READY: awaiting cancellation\n', stderr: 'cancelled: no effect\n', state: { jobs: {}, effects: [] } });
      expect(JSON.parse(cancelled.stdout).stateRoot.startsWith(path.join(fixture.root, '.qa-state/cancel-'))).toBe(true);
      expect(fs.existsSync(path.join(fixture.root, '.qa-state/ledger.json'))).toBe(false);
      expect(fixtureCommand(fixture.root, ['src/cli.ts', 'export'])).toEqual({ exit: 69, stdout: '', stderr: 'SETUP_BLOCKED: optional qa-fixture-exporter-unavailable is not installed\n', signal: null });
      expect(fs.existsSync(path.join(fixture.root, '.qa-state/ledger.json'))).toBe(false);
    } finally { fixture.cleanup(); }
  });

  for (const healthy of [false, true]) {
    test(`webhook auth, durable completion, duplicates and cancellation (${healthy ? 'healthy' : 'seeded'})`, () => {
      const fixture = createQAFunctionalFixture('webhook', { healthy });
      try {
        const probe = (scenario: string) => JSON.parse(fixtureCommand(fixture.root, ['probe.ts', scenario]).stdout);
        expect(probe('reject').requests.map(request => request.status)).toEqual([401, 422]);
        expect(probe('reject').state).toEqual({ jobs: {}, effects: [] });
        for (const scenario of ['happy', 'duplicate']) {
          const result = probe(scenario);
          expect(result.requests.every(request => request.status === 202)).toBe(true);
          expect(result.state.effects).toEqual([{ id: 'delivery', cents: 7 }]);
          expect(result.state.jobs.delivery.status).toBe('complete');
        }
        expect(probe('cancel').state).toEqual({ jobs: { delivery: { cents: 7, status: 'pending', attempts: 0 } }, effects: [] });
        expect(fixtureCommand(fixture.root, ['probe.ts', 'dependency']).exit).toBe(69);
        expect(fixtureCommand(fixture.root, ['test']).exit).toBe(0);
      } finally { fixture.cleanup(); }
    });

    test(`partial recovery and both controlled concurrency orders (${healthy ? 'healthy' : 'seeded'})`, () => {
      const fixture = createQAFunctionalFixture('webhook', { healthy });
      try {
        for (const scenario of ['partial', 'concurrent-ab', 'concurrent-ba']) {
          const result = JSON.parse(fixtureCommand(fixture.root, ['probe.ts', scenario]).stdout);
          expect(result.state.effects).toHaveLength(healthy ? 1 : 2);
          expect(result.state.jobs.delivery.status).toBe('complete');
          expect(result.state.jobs.delivery.attempts).toBe(2);
          if (scenario === 'partial') {
            expect(result.interrupted).toBe('injected worker interruption after effect');
            expect(result.stateAfterInterruption).toEqual({ jobs: { delivery: { cents: 7, status: 'pending', attempts: 1 } }, effects: [{ id: 'delivery', cents: 7 }] });
          }
          else expect(result.order).toEqual(scenario.endsWith('ab') ? ['a', 'b'] : ['b', 'a']);
        }
      } finally { fixture.cleanup(); }
    });
  }

  test('rejects traversal, symlinks and hard links before fixture writes', () => {
    const fixture = createQAFunctionalFixture('cli');
    const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'qaf-outside-'));
    try {
      expect(() => ownedPath(fixture.root, '../escape')).toThrow('escapes');
      fs.symlinkSync(outside, path.join(fixture.root, '.qa-state/linked'));
      expect(() => ownedPath(fixture.root, '.qa-state/linked/file')).toThrow('link');
      fs.linkSync(path.join(fixture.root, 'src/cli.ts'), path.join(fixture.root, '.qa-state/hard'));
      expect(() => ownedPath(fixture.root, '.qa-state/hard')).toThrow('link');
      expect(fs.readdirSync(outside)).toEqual([]);
    } finally { fixture.cleanup(); fs.rmSync(outside, { recursive: true, force: true }); }
  });
});
