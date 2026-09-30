import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { resolveEvalModel } from '../lib/eval-model';

const ROOT = path.resolve(import.meta.dir, '..');
const source = fs.readFileSync(path.join(ROOT, 'test/skill-e2e-qa-bugs.test.ts'), 'utf8');
const setup = source.match(/^function browserSetupSection\(\): string \{[\s\S]*?^\}/m)?.[0];
const runner = source.match(/^  async function runPlantedBugEval\([\s\S]*?^  \}/m)?.[0];
const registrations = [...source.matchAll(/^  testConcurrentIfSelected\('qa-b[678]-[^']+', async \(\) => \{[\s\S]*?^  \}, CAPTURE_LONG_MS\);/gm)].map(match => match[0]);
if (!setup || !runner || registrations.length !== 3) throw new Error('Missing actual planted-browser fixture functions or registrations');
const script = new Bun.Transpiler({ loader: 'ts' }).transformSync([setup, runner, ...registrations].join('\n'));
const asset = fs.readFileSync(path.join(ROOT, 'qa/sections/browser-setup.md'), 'utf8');
const ids = ['qa-b6-static', 'qa-b7-spa', 'qa-b8-checkout'];

for (const id of ids) test.each(['complete section', 'additional trailing policy', 'missing section'])(`${id} retains the carved browser setup: %s`, async scenario => {
  const owned = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'qa-bugs-fixture-')));
  const section = scenario === 'additional trailing policy' ? asset + '\n## Additional policy\nPreserve this final policy.\n' : asset;
  const calls: unknown[] = [];
  const callbacks = new Map<string, () => Promise<void>>();
  const stopped = new Error('Stopped at the offline actor boundary');
  try {
    fs.mkdirSync(path.join(owned, 'qa/sections'), { recursive: true });
    fs.writeFileSync(path.join(owned, 'qa/SKILL.md'), '# QA entrypoint\nRead the carved sections.\n');
    if (scenario !== 'missing section') fs.writeFileSync(path.join(owned, 'qa/sections/browser-setup.md'), section);
    new Function('fs', 'path', 'os', 'ROOT', 'setupBrowseShims', 'testServer', 'browseBin',
      'runSkillTest', 'runId', 'CAPTURE_MS', 'CAPTURE_LONG_MS', 'testConcurrentIfSelected', 'resolveEvalModel', script)(
      fs, path, { ...os, tmpdir: () => owned }, owned, () => {}, { url: 'http://fixture.invalid' }, '/unused/browse',
      async (options: { workingDirectory: string; prompt: string; testName: string }) => {
        calls.push(options);
        const actual = fs.readFileSync(path.join(options.workingDirectory, 'BROWSER-SETUP.md'), 'utf8');
        expect(actual).toBe(section);
        expect(actual.indexOf('## Browser access decision')).toBeLessThan(actual.indexOf('## BROWSER SETUP'));
        expect(actual).toContain('Unknown caller: use report-only authority');
        expect(actual).toContain('## Browser fallback');
        expect(actual).toContain('Invocation does not authorize external mutations');
        expect(options.testName).toBe(id);
        expect(options.prompt).toContain('read BROWSER-SETUP.md in this directory and follow it exactly');
        throw stopped;
      }, 'offline-fixture', 300000, 600000,
      (name: string, callback: () => Promise<void>) => callbacks.set(name, callback), resolveEvalModel,
    );
    expect([...callbacks.keys()]).toEqual(ids);
    if (scenario === 'missing section') {
      await expect(callbacks.get(id)!()).rejects.toThrow('ENOENT');
      expect(calls).toHaveLength(0);
    } else {
      await expect(callbacks.get(id)!()).rejects.toBe(stopped);
      expect(calls).toHaveLength(1);
    }
  } finally {
    fs.rmSync(owned, { recursive: true, force: true });
  }
});
