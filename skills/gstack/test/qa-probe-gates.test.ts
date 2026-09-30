import { describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { ALL_HOST_CONFIGS } from '../hosts';
import { generateQAExploratory, generateQAMethodReads, generateQAReview, generateQAReviewPreflight } from '../scripts/resolvers/qa';
import { generatePlanVerificationExec } from '../scripts/resolvers/review';
import { HOST_PATHS } from '../scripts/resolvers/types';

function assertPreparation(text: string) {
  expect(text).toContain('Complete these Reads in order before writing charters or probing');
  expect(text).toContain('Do not repeat a Read already completed in this invocation');
  const stages = ['1. Read `sections/scope.md`', 'in full and select the surfaces',
    '2. Read the selected surface methods below in full', '**Functional surfaces:**',
    'Read `sections/system-functional.md` in full.', '**Browser surfaces only:**',
    'Read `sections/qa-patterns.md` in full.', '## 1. Charter and preflight',
    'Write a **charter**', 'Start once before baseline:', '1. First demonstrate success'];
  const positions = stages.map(stage => text.indexOf(stage));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  for (const section of ['scope', 'system-functional', 'qa-patterns']) {
    expect(text.split(`Read \`sections/${section}.md\``)).toHaveLength(2);
  }
}

function assertBoundsAndLayout(text: string) {
  for (const contract of [
    'Browser Quick: SECONDS=30', 'Browser Full/Regression: SECONDS=900',
    'Functional Full, Quick and Regression have no default total timer',
    "Set SECONDS to the shorter mode/caller limit",
    "an unlimited mode uses the caller\'s bound",
    'Without a total time limit, do not start D',
    'announce finite command timeouts',
    "EARLIER_UTC = caller\'s absolute deadline, if set",
    'Clocks/checkpoints use REPORT_DIR; mixed standalone runs use REPORT_DIR/browser and REPORT_DIR/functional, with one final report at REPORT_DIR. Caller paths win.',
    'in the probe directory, beside its deadline if bounded',
  ]) expect(text).toContain(contract);
  expect(text.indexOf('Caller paths win.')).toBeLessThan(text.indexOf('Start once before baseline:'));
}

function assertPlanExecution(text: string, shared = generateQAExploratory({ host: 'claude', skillName: 'qa', tmplPath: '', paths: HOST_PATHS.claude })) {
  const step = text.slice(text.indexOf('**3. Run smoke and plan checks.**'), text.indexOf('**4. Check freshness before reporting.**')).replace(/\s+/g, ' ');
  for (const contract of [
    'Follow the shared Probe loop for smoke checks, replays and revalidation until the smoke limit',
    'Then run required plan checks, even after smoke expires',
    'using the same procedure but no smoke guard; never reset the clock',
    "Use finite command timeouts, capped at the caller\'s remaining time if it has a deadline",
    'When the caller\'s deadline expires, mark unfinished checks not-run',
    'Await clock/guard results before acting',
  ]) expect(step).toContain(contract);
  expect(step.indexOf('Follow the shared Probe loop')).toBeLessThan(step.indexOf('Then run required plan checks'));
  expect(step.indexOf('Then run required plan checks')).toBeLessThan(step.indexOf('same procedure'));
  for (const contract of ['First demonstrate success: output AND durable effects',
    'Wait for successful checkpoint publication before dispatch',
    'Replay the exact failing command/request from the same initial fixture state']) {
    expect(shared).toContain(contract);
  }
}

describe('QA probe entry and checkpoint gates', () => {
  test('the native CI preparation excerpt lacks the required acknowledged-read gate', () => {
    const captured = JSON.parse(fs.readFileSync(path.join(import.meta.dir, 'fixtures/qa-functional-ci-36505065023.json'), 'utf8'));
    const output = captured.omittedReadEvents.flatMap(event => event.message.content)
      .find(block => block.type === 'tool_result' && typeof block.content === 'string' && block.content.includes('# Shared exploratory QA')).content;
    const old = output.replace(/^\s*\d+(?:→|\t)/gm, '');
    expect(old).toContain('Read `sections/scope.md`');
    expect(old).toContain('Read `sections/system-functional.md`');
    const current = generateQAExploratory({ host: 'claude', skillName: 'qa-only', tmplPath: '', paths: HOST_PATHS.claude });
    for (const clause of ['## 0. Preparation gate', 'Await each successful Read result before continuing',
      'description, section index or remembered method is not a completed instruction Read',
      'If either required Read is missing, complete it now before Charter and preflight']) {
      expect(old).not.toContain(clause);
      expect(current).toContain(clause);
    }
    assertPreparation(current);
  });

  test('the report template no longer asks agents to sanitize public state paths', () => {
    const report = fs.readFileSync(path.join(import.meta.dir, '../qa/templates/functional-report-template.md'), 'utf8');
    expect(report).toContain('EXACT SAFE OUTPUT AND STATE PATHS');
    expect(report).toContain('REDACTION AND REPRODUCIBILITY LIMITS');
    expect(report).not.toContain('SANITIZED OUTPUT/STATE PATHS');
  });

  test('parent QA instructions resolve nested methods and reports from the same installed QA directory', () => {
    for (const host of ALL_HOST_CONFIGS) {
      for (const skillName of ['review', 'ship']) {
        const ctx = { host: host.name, skillName, tmplPath: '', paths: HOST_PATHS[host.name] };
        const preflight = generateQAReviewPreflight(ctx).replace(/\s+/g, ' ');
        const body = generateQAReview(ctx).replace(/\s+/g, ' ');
        expect(preflight).toContain('Resolve QA\'s `sections/...` and `templates/...` paths from that installed QA SKILL.md directory, not the caller or product directory');
        expect(body).toContain('Read QA\'s `sections/browser-setup.md`');
        expect(body).toContain('Read QA\'s `templates/functional-report-template.md`');
        if (skillName === 'review') {
          expect(body).toContain('Read QA\'s `templates/qa-report-template.md`');
          expect(body).toContain('Reuse Step 4\'s surfaces and completed Reads');
          expect(body).toContain('Finish missing methods before charters; do not repeat completed Reads');
        }
      }
    }
  });

  test('composes and checks a complete note before immutable publication on every host', () => {
    for (const host of ALL_HOST_CONFIGS) {
      for (const skillName of ['qa', 'qa-only']) {
        const text = generateQAExploratory({ host: host.name, skillName, tmplPath: '', paths: HOST_PATHS[host.name] });
        const stages = [...(skillName === 'qa-only' ? ['Classify the last result before copying it'] : []),
          'Check fields before publication', 'bun Q checkpoint R NNN CAPTURE_ID',
          'Browser checkpoints use Write', 'Wait for successful checkpoint publication', '3. Run that exact probe'];
        const positions = stages.map(stage => text.indexOf(stage));
        expect(positions.every(position => position >= 0)).toBe(true);
        expect(positions).toEqual([...positions].sort((a, b) => a - b));
        expect(text).toContain('No drafts/placeholders');
        expect(text).toContain('corrections cannot repair published notes');
        if (skillName === 'qa-only') {
          expect(text).toContain('never invent a substitute path, identity or state');
          expect(text).toContain('For actual secrets/private payloads, withhold those values');
          expect(text).toContain('and replay limits in the report');
          expect(text).toContain('stop the affected probe chain');
          expect(text).toContain('An absolute state path is not itself a secret');
        } else {
          expect(text).toContain('No drafts/placeholders or invented safe-path redactions');
          expect(text).toContain('Withhold unsafe values, disclose limits and stop that chain');
        }
        if (skillName === 'qa-only') expect(text).toContain('If capture is incomplete, report that limit instead of reconstructing it');
      }
    }
  });

  test('mode bounds and checkpoint nesting are explicit before dispatch', () => {
    for (const skillName of ['qa', 'qa-only']) {
      const text = generateQAExploratory({ host: 'claude', skillName, tmplPath: '', paths: HOST_PATHS.claude });
      for (const contract of [
        'Reuse resolved REPORT_DIR',
        'charters as Markdown in the report',
        "Set SECONDS to the shorter mode/caller limit",
        'Without a total time limit, do not start D',
        'Browser Quick: SECONDS=30',
        'Browser Full/Regression: SECONDS=900',
        'exactly four top-level fields:', 'observationCommand:', 'observed:', 'hypothesis:', 'nextCommand:',
        'observed: its exact decoded child JSON (no wrapper/extra keys), or its full non-JSON text',
        'QA_DEADLINE receipts are not observations',
      ]) expect(text).toContain(contract);
      assertBoundsAndLayout(text);
      expect(text.indexOf('Browser Quick: SECONDS=30')).toBeLessThan(text.indexOf('bun G start D'));
    }
  });

  test('each shared loop loads its selected methods before choosing or executing probes', () => {
    for (const host of ALL_HOST_CONFIGS) {
      for (const skillName of ['qa', 'qa-only']) {
        const ctx = { host: host.name, skillName, tmplPath: '', paths: HOST_PATHS[host.name] };
        const text = generateQAExploratory(ctx);
        const methods = generateQAMethodReads(ctx);
        expect(text).toContain(methods);
        expect(text.indexOf(methods)).toBeLessThan(text.indexOf('1. First demonstrate success'));
        assertPreparation(text);
        const decision = text.indexOf('Decide whether another probe is needed');
        const write = text.indexOf('**Publish before probing.**');
        expect(decision).toBeGreaterThan(-1);
        expect(decision).toBeLessThan(write);
        expect(text.slice(decision, write)).toContain('write the report, not a checkpoint');
        expect(text.slice(decision, write)).toContain('If expired or no safe next probe remains');
        expect(text.slice(decision, write)).not.toContain('If done or blocked');
        if (skillName === 'qa-only') {
          expect(text).toContain('Check fields before publication');
          expect(text).toContain('retain the entire result unchanged, including owned fixture paths, IDs, hashes');
        } else {
          expect(text).toContain('Preserve every safe program-JSON key/value');
          expect(text).toContain('Preserve every safe program-JSON key/value and identity hash unchanged');
        }
        expect(text).toContain('Q supplies observed; never transcribe it');
      }
    }
  });

  test('expiry branches precede baseline, checkpoint, probe and replay execution on every host', () => {
    for (const host of ALL_HOST_CONFIGS) for (const skillName of ['qa', 'qa-only', 'review', 'ship']) {
      const text = generateQAExploratory({ host: host.name, skillName, tmplPath: '', paths: HOST_PATHS[host.name] });
      const steps = text.slice(text.indexOf('1. First demonstrate success')).split(/\n(?=[2-5]\. )/);
      expect(steps).toHaveLength(5);
      expect(text).toContain('/gstack-qa-deadline');
      expect(text).toContain('bun G start D SECONDS [EARLIER_UTC]');
      expect(text).toContain('Bounded browsers: `bun G run D -- COMMAND ARGS`');
      expect(text).toContain('Never reset D/bypass G');
      expect(text).toContain('invalid/missing D stops probes');
      expect(steps[0]).toContain('demonstrate success: output AND durable effects');
      expect(steps[0]).toContain('Guard if bounded; await completion');
      expect(steps[1]).toContain('If bounded, run `bun G status D`');
      expect(steps[1].indexOf('If expired')).toBeLessThan(steps[1].indexOf('**Publish before probing.**'));
      expect(steps[1]).toContain('STOP exploration; write the report, not a checkpoint');
      expect(steps[2]).toContain('Run that exact probe; G enforces the deadline when bounded');
      expect(steps[2]).toContain('G enforces the deadline');
      expect(steps[2]).toContain('Report refusals as not-run');
      expect(steps[3]).toContain('via steps 2–3');
      expect(steps[3]).toContain('then minimize via those gates');
      expect(steps[3]).toContain('Expiry leaves confirmation/minimization incomplete');
      expect(steps[4]).toContain('return to step 2 for each affected revalidation');
    }
  });

  test('parent QA makes method loading a stop gate even for plan verification', () => {
    for (const host of ALL_HOST_CONFIGS) {
      for (const skillName of ['review', 'ship']) {
        const ctx = { host: host.name, skillName, tmplPath: '', paths: HOST_PATHS[host.name] };
        const text = (skillName === 'review' ? generateQAReviewPreflight(ctx) : '') + generateQAReview(ctx);
        expect(text).toContain('> **STOP.** Before any probe, including plan checks, complete the ordered scope/method Reads below');
        expect(text).toContain('{{QA_RESOURCE:exploratory}}');
        expect(text).not.toContain('{{QA_RESOURCE:scope}}');
        expect(text).not.toContain('Read `sections/system-functional.md`');
        const shared = generateQAExploratory({ ...ctx, skillName: 'qa' });
        assertPreparation(shared);
        expect(text).toContain('Before any probe, including plan checks');
        assertPlanExecution(text);
        const required = text.indexOf(skillName === 'review'
          ? '**2. Check readiness and list required checks.**' : '**2. List required checks');
        expect(required).toBeGreaterThan(-1);
        expect(text.indexOf('> **STOP.**')).toBeLessThan(required);
        if (skillName === 'review') {
          const isolation = text.indexOf('**1. Set the charter and isolation.**');
          const setup = text.indexOf("Read QA's `sections/browser-setup.md`");
          expect(isolation).toBeGreaterThan(text.indexOf('> **STOP.**'));
          expect(setup).toBeGreaterThan(required);
          expect(text.slice(isolation, required).replace(/\s+/g, ' ')).toContain('complete the shared isolation/permission preflight before setup');
          expect(text).toContain('Step 4 is read-only: defer charters, setup and probes to Step 4.7');
        } else {
          const setup = text.indexOf("For browsers, Read QA's `sections/browser-setup.md`");
          expect(setup).toBeGreaterThan(required);
          expect(setup).toBeLessThan(text.indexOf('**3. Run smoke and plan checks'));
        }
      }
    }
  });

  test('preparation controls reject reordered scope, duplicate Reads and early charters', () => {
    const text = generateQAExploratory({ host: 'claude', skillName: 'qa', tmplPath: '', paths: HOST_PATHS.claude });
    assertPreparation(text);
    const method = 'Read `sections/system-functional.md` in full.';
    for (const changed of [
      method + '\n' + text.replace(method, ''),
      text.replace(method, method + '\n' + method),
      'Write a **charter**\n' + text,
      text.replace('Do not repeat a Read already completed in this invocation', 'Repeat all Reads'),
    ]) expect(() => assertPreparation(changed)).toThrow();
  });

  test('layout and bounds controls reject mixed caller overrides, split reports and unbounded commands', () => {
    const text = generateQAExploratory({ host: 'claude', skillName: 'qa', tmplPath: '', paths: HOST_PATHS.claude });
    assertBoundsAndLayout(text);
    for (const [before, after] of [
      ['mixed standalone runs', 'all mixed runs'],
      ['REPORT_DIR/browser and REPORT_DIR/functional', 'REPORT_DIR'],
      ['with one final report at REPORT_DIR', 'write a final report per surface'],
      ['Caller paths win.', 'Surface paths win.'],
      ['finite command timeouts', 'unbounded command timeouts'],
      ["shorter mode/caller limit", 'the mode duration regardless of caller'],
    ]) expect(() => assertBoundsAndLayout(text.replace(before, after))).toThrow();
  });

  test('required plan checks cannot inherit the expired smoke guard or lose caller bounds and checkpoints', () => {
    for (const skillName of ['review', 'ship']) {
      const text = generateQAReview({ host: 'claude', skillName, tmplPath: '', paths: HOST_PATHS.claude }).replace(/\s+/g, ' ');
      assertPlanExecution(text);
      for (const [before, after] of [
        ['Then run required plan checks, even after smoke expires', 'Skip plan checks when smoke expired'],
        ['no smoke guard; never reset the clock', 'restart and use the smoke guard'],
        ['same procedure', 'Start a new checkpoint sequence'],
        ['at the caller\'s remaining time', 'with no caller cap'],
        ['When the caller\'s deadline expires, mark unfinished checks not-run', 'If that deadline expired, mark the check passed'],
        ['Await clock/guard results before acting', 'Ignore clock results'],
      ]) {
        expect(text).toContain(before);
        expect(() => assertPlanExecution(text.replace(before, after))).toThrow();
      }
      const smoke = 'Follow the shared Probe loop for smoke checks, replays and revalidation until the smoke limit.';
      expect(() => assertPlanExecution(text.replace(smoke, '').replace('**4. Check', smoke + '\n**4. Check'))).toThrow();
      const shared = generateQAExploratory({ host: 'claude', skillName: 'qa', tmplPath: '', paths: HOST_PATHS.claude });
      for (const contract of ['First demonstrate success: output AND durable effects',
        'Wait for successful checkpoint publication before dispatch',
        'Replay the exact failing command/request from the same initial fixture state']) {
        expect(() => assertPlanExecution(text, shared.replace(contract, 'Optional evidence'))).toThrow();
      }
    }
  });

  test('core review collects runtime checks without executing them ahead of QA setup', () => {
    for (const [file, step] of [['review/SKILL.md.tmpl', 'Step 4.7'], ['ship/sections/review-army.md.tmpl', 'Step 9.2.1']]) {
      const template = fs.readFileSync(path.join(import.meta.dir, '..', file), 'utf8');
      const text = template.replace('{{QA_REVIEW_PREFLIGHT}}', generateQAReviewPreflight({ host: 'claude', skillName: 'review', tmplPath: '', paths: HOST_PATHS.claude }));
      const staticRule = step === 'Step 4.7' ? 'Step 4 is read-only: defer charters, setup and probes to Step 4.7' : `This pass is static; defer product probes to ${step}`;
      expect(text).toContain(staticRule);
      expect(text.indexOf(staticRule)).toBeLessThan(text.indexOf('{{QA_REVIEW}}'));
      if (step === 'Step 4.7') {
        expect(template.indexOf('{{QA_REVIEW_PREFLIGHT}}')).toBeGreaterThan(template.indexOf('## Step 4:'));
        expect(text.indexOf(staticRule)).toBeLessThan(text.indexOf('Apply both checklist passes in order'));
      }
    }
  });

  test('plan execution waits for the actual shared method reads, not just collection', () => {
    const text = generatePlanVerificationExec({ host: 'claude', skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude });
    expect(text).toContain('Do not invoke an entire QA skill or start probes here');
    expect(text).toContain('Before the first plan command, complete Step 9.2.1');
    expect(text).toContain('method Reads and the shared probe loop');
  });
});
