import { afterEach, describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { buildCodexOfferingPrompt, codexOfferingSources } from './helpers/codex-offering-fixture';
import captured from './fixtures/codex-offering-cdd-public.json';
import timedOut from './fixtures/codex-offering-timeout-public.json';

const roots: string[] = [];
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); });
function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'gstack-offering-source-'));
  roots.push(root);
  fs.mkdirSync(path.join(root, 'work/plan-ceo-review/sections'), { recursive: true });
  fs.writeFileSync(path.join(root, 'work/plan-ceo-review/SKILL.md'), '# CEO\nRead sections/review-sections.md for the outside voice.\n');
  fs.writeFileSync(path.join(root, 'work/plan-ceo-review/sections/review-sections.md'), '# Outside Voice\nUse the real integration documented here.\n');
  return { root, work: path.join(root, 'work'), skill: path.join(root, 'work/plan-ceo-review') };
}

describe('Codex offering source lookup', () => {
  test('captured timeouts wrote oversized summaries successfully but never completed execution', () => {
    expect(timedOut.cases.map(row => row.attempt)).toEqual([1, 2]);
    for (const row of timedOut.cases) {
      expect(row.exitReason).toBe('timeout');
      expect(row.timeoutMs).toBe(120_000);
      expect(row.tools.length).toBeLessThan(row.maxTurns);
      expect(row.tools.every(tool => tool.acknowledged && !tool.isError)).toBe(true);
      expect(row.tools.some(tool => !['Read', 'Bash', 'Write'].includes(tool.name))).toBe(false);
      expect(row.summaryWords).toBeGreaterThan(600);
      expect(row.tools.at(-1)?.name).toBe('Write');
      expect(row.writeSeconds * 1000).toBeLessThan(row.timeoutMs);
      expect(row.writeAcknowledged).toBe(true);
      expect(row.providerFinalResult).toBe(false);
    }
  });

  test('bounded summaries retain all five audit questions and require a terminal response after writing', () => {
    const root = path.resolve(import.meta.dir, '..');
    for (const skill of ['office-hours', 'plan-ceo-review', 'plan-design-review', 'plan-eng-review']) {
      const prompt = buildCodexOfferingPrompt({ root, skill, featureName: 'outside voice', summaryPath: '/tmp/offering-summary.md' });
      expect(prompt.split('\n').filter(line => /^\d\. /.test(line))).toEqual([
        '1. How is Codex availability checked? (what exact bash command?)',
        '2. How is the user prompted? (via AskUserQuestion? what are the options?)',
        '3. What happens when Codex is NOT available? (fallback to subagent? skip entirely?)',
        '4. Is this step blocking (gates the workflow) or optional (can be skipped)?',
        '5. What prompt/context is sent to Codex?',
      ]);
      expect(prompt).toContain('read the relevant complete sections');
      expect(prompt).toContain('identify anything they do not document');
      expect(prompt).toContain('source file/line citations, at most 600 words total');
      expect(prompt).toContain('Answer every question and cover its relevant branches');
      expect(prompt).toContain('Preserve the exact availability command');
      expect(prompt).toContain('instead of copying whole blocks');
      expect(prompt.indexOf('After the Write succeeds')).toBeGreaterThan(prompt.indexOf('Write your summary to'));
      expect(prompt).toContain('finish with one sentence naming the saved path');
      expect(prompt).toContain('Do not repeat the audit in your final response');
    }
  });

  test('captured failures used all eight tool calls before writing, finding carved evidence late', () => {
    expect(captured.cases.map(row => row.attempt)).toEqual([1, 2]);
    for (const row of captured.cases) {
      expect(row.exitReason).toBe('error_max_turns');
      expect(row.tools).toHaveLength(row.maxTurns);
      expect(row.tools.every(tool => ['Read', 'Bash'].includes(tool.name))).toBe(true);
      expect(row.tools.slice(0, 5).filter(tool => tool.name === 'Read')
        .every(tool => tool.input.file_path?.endsWith('/plan-ceo-review/SKILL.md'))).toBe(true);
      const carvedRead = row.tools.findIndex(tool => tool.name === 'Read'
        && tool.input.file_path?.endsWith('/plan-ceo-review/sections/review-sections.md'));
      expect(carvedRead).toBeGreaterThanOrEqual(6);
    }
  });

  test('the real prompt builder lists the complete generated target sources before the audit questions', () => {
    const root = path.resolve(import.meta.dir, '..');
    for (const skill of ['office-hours', 'plan-ceo-review', 'plan-design-review', 'plan-eng-review']) {
      const sources = codexOfferingSources(root, skill);
      const generated = fs.readdirSync(path.join(root, skill, 'sections')).filter(file => file.endsWith('.md')).sort();
      expect(sources).toEqual([`${skill}/SKILL.md`, ...generated.map(file => `${skill}/sections/${file}`)]);
      const prompt = buildCodexOfferingPrompt({ root, skill, featureName: 'outside voice', summaryPath: '/tmp/offering-summary.md' });
      for (const source of sources) expect(prompt.indexOf(JSON.stringify(source))).toBeLessThan(prompt.indexOf('1. How is Codex availability checked?'));
      expect(prompt).toContain('one scoped search');
      expect(prompt).toContain('read-only source lookup');
      expect(prompt).toContain('Write your summary to /tmp/offering-summary.md');
      expect(prompt).not.toContain('command -v codex'); // Answers must come from the real documentation.
    }
  });

  test('inventory excludes template duplicates and sibling skills without dropping generated sections', () => {
    const { work, skill } = fixture();
    fs.writeFileSync(path.join(skill, 'sections/review-sections.md.tmpl'), 'DUPLICATE');
    fs.writeFileSync(path.join(skill, 'sections/other-evidence.md'), '# Additional generated evidence\n');
    fs.mkdirSync(path.join(work, 'sibling'));
    fs.writeFileSync(path.join(work, 'sibling/SKILL.md'), 'SIBLING ANSWERS');
    expect(codexOfferingSources(work, 'plan-ceo-review')).toEqual([
      'plan-ceo-review/SKILL.md', 'plan-ceo-review/sections/other-evidence.md', 'plan-ceo-review/sections/review-sections.md',
    ]);
  });

  test('missing entrypoint fails before a partial lookup task can run', () => {
    const { work, skill } = fixture();
    fs.unlinkSync(path.join(skill, 'SKILL.md'));
    expect(() => codexOfferingSources(work, 'plan-ceo-review')).toThrow();
  });

  test('outside-root skill and linked source cannot enter the manifest', () => {
    const { root, work, skill } = fixture();
    fs.mkdirSync(path.join(root, 'outside'));
    fs.writeFileSync(path.join(root, 'outside/SKILL.md'), 'OUTSIDE SOURCE');
    expect(() => codexOfferingSources(work, '../outside')).toThrow('outside its fixture scope');
    fs.unlinkSync(path.join(skill, 'SKILL.md'));
    fs.symlinkSync(path.join(root, 'outside/SKILL.md'), path.join(skill, 'SKILL.md'));
    expect(() => codexOfferingSources(work, 'plan-ceo-review')).toThrow('outside its fixture scope');
  });

  test('malformed or empty generated documents fail instead of producing a partial manifest', () => {
    const { work, skill } = fixture();
    fs.mkdirSync(path.join(skill, 'sections/not-a-file.md'));
    expect(() => codexOfferingSources(work, 'plan-ceo-review')).toThrow('not a file');
    fs.rmdirSync(path.join(skill, 'sections/not-a-file.md'));
    fs.writeFileSync(path.join(skill, 'sections/review-sections.md'), '');
    expect(() => codexOfferingSources(work, 'plan-ceo-review')).toThrow('empty or missing document');
  });
});
