import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { generateReviewArmy } from '../scripts/resolvers/review-army';
import { generateCrossReviewDedup, generatePlanCompletionAuditReview, generatePlanCompletionAuditShip, generateSharedCodeReuse, generateScopeDrift } from '../scripts/resolvers/review';
import { generateQAExploratory, generateQAReview } from '../scripts/resolvers/qa';
import { generateConfidenceCalibration } from '../scripts/resolvers/confidence';
import { HOST_PATHS, type TemplateContext } from '../scripts/resolvers/types';

const root = join(import.meta.dir, '..');
const skill = readFileSync(join(root, 'review/SKILL.md.tmpl'), 'utf8');
const adversarial = readFileSync(join(root, 'review/sections/adversarial.md.tmpl'), 'utf8');

test('review audits deliverables before deferring behavioral plan checks to the QA preflight', () => {
  const ctx: TemplateContext = { skillName: 'review', tmplPath: '', host: 'claude', paths: HOST_PATHS.claude };
  const audit = generatePlanCompletionAuditReview(ctx).replace(/\s+/g, ' ');
  for (const contract of [
    'Separate static audit evidence from behavioral checks',
    'retain the exact command, expected outcome and source for Step 4.7',
    'They remain pending execution, never DONE from a diff',
    'A mixed item contributes to both lists',
    'Zero audited deliverables do not waive these checks',
    'Keep external-state and human-only checks under the existing audit rules',
    'If only behavioral checks remain, report zero audited deliverables and retain their pending Step 4.7 list',
    'For each audited deliverable, run the verification dispatch',
    'File-existence checks and verified read-only content validators are static audit checks, not behavioral probes',
    'Inspect the validator and its hooks before running it; verify read-only effects and access to the target',
    'leave the item UNVERIFIABLE and defer the command to Step 4.7',
    'Do not start applications, exercise APIs or mutate state during this audit',
    'If found and verified safe above, invoke it',
  ]) expect(audit).toContain(contract);
  expect(audit.indexOf('Inspect the validator and its hooks')).toBeLessThan(audit.indexOf('If found and verified safe above, invoke it'));
  expect(audit).not.toContain('For each extracted plan item, run the verification dispatch');
  const qa = generateQAReview(ctx);
  expect(qa).toContain('Then run required plan checks, even after smoke expires');
  expect(qa).toContain('Report clean/completed only when all required checks pass on current inputs');
});

test('review prior-Skip matching includes adversarial and Greptile findings without relaxing eligibility', () => {
  const dedup = generateCrossReviewDedup({ skillName: 'review', tmplPath: '', host: 'claude', paths: HOST_PATHS.claude });
  expect(dedup).toContain('For every combined finding, including core, specialist, exploratory QA, adversarial and valid actionable Greptile findings, check:');
  expect(dedup).toContain('Suppress only when all conditions hold: the user skipped the same unchanged finding');
  expect(dedup).toContain('same advisory/defect kind');
  expect(dedup).toContain('Never use a skipped advisory to suppress a real defect');
  expect(dedup).toContain('Only suppress `skipped` findings — never `fixed` or `auto-fixed`');
  expect(skill).toContain('Run Step 5.0 severity/prior-skip dedup on all');
});

test('Review audit and prior-Skip clarifications do not route Ship through Review steps', () => {
  for (const host of Object.keys(HOST_PATHS) as TemplateContext['host'][]) {
    const ctx: TemplateContext = { skillName: 'ship', tmplPath: '', host, paths: HOST_PATHS[host] };
    const audit = generatePlanCompletionAuditShip(ctx);
    expect(audit).toContain('Step 8.1/9');
    expect(audit).not.toContain('Step 4.7');
    expect(audit).not.toContain('Separate static audit evidence from behavioral checks');
    const dedup = generateCrossReviewDedup(ctx);
    expect(dedup).toContain('Step 9.3: Cross-review finding dedup');
    expect(dedup).not.toContain('Step 5.0');
    expect(dedup).not.toContain('For every combined finding');
  }
});

test('review collects every source before its single parent fix phase', () => {
  const markers = [
    '## Step 4: Critical pass', '### TODOS cross-reference',
    '### Documentation staleness check', '{{SECTION:review-army}}',
    '{{QA_REVIEW}}', '{{SECTION:adversarial}}',
    '## Step 5: Fix-First Review', '{{CROSS_REVIEW_DEDUP}}',
    '### Step 5a:', '### Step 5b:', '### Step 5c:', '### Step 5d:',
    '## Step 5.8: Persist Eng Review result',
  ];
  const positions = markers.map(marker => skill.indexOf(marker));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  expect(skill.match(/## Step 5: Fix-First Review/g)).toHaveLength(1);
  expect(skill.replace(/\s+/g, ' ')).toContain('Do not edit reviewed source until Step 5');
  expect(skill.replace(/\s+/g, ' ')).toContain('every dispatched reader has returned or is confirmed stopped');
});

test('review settles adversarial attempts before fixing and has one full-pass back edge', () => {
  const generated = readFileSync(join(root, 'review/sections/adversarial.md'), 'utf8');
  const decision = skill.slice(skill.indexOf('## Step 5.8: Persist Eng Review result')).replace(/\s+/g, ' ');
  expect(generated).toContain('## Step 4.8: Adversarial review');
  expect(generated).toContain("queued for the parent's Fix-First handling at Step 5; do not edit during Step 4.8");
  expect(generated.replace(/\s+/g, ' ')).toContain('before the parent applies queued fixes');
  expect(generated).toContain('Return all findings and structured-review decisions to Step 5');
  expect(decision).toContain('A pass covers Steps 3–5, including all reviewers before fixes');
  expect(decision).toContain('Below 3, repeat Steps 3–5 with a new REVIEW_START');
  expect(decision).not.toContain('Route Step');
  expect(decision).not.toContain('Steps 5.0–5d');
  expect(decision).toContain('without a clean summary or a fourth pass');
});

test('review small-diff and failed-reader paths retain QA and the required adversarial pass', () => {
  const army = generateReviewArmy({ skillName: 'review', tmplPath: 'review/SKILL.md.tmpl',
    host: 'claude', paths: HOST_PATHS.claude });
  expect(army).toContain("Continue to Step 4.6 with the core findings and an empty specialist list, then the parent's Exploratory QA step and Step 4.8 (adversarial review), then Step 5");
  expect(army).not.toContain('design-lite');
  expect(army).toContain('Missing dispatched coverage remains incomplete, never completed or clean');
  expect(army).toContain('Continue independent Step 4.7 QA and Step 4.8 adversarial review');
  expect(army).not.toContain("Exploratory QA step, then continue to Step 5.");
  expect(army).not.toContain('If the Red Team subagent fails or times out, skip silently');
});

test('review defines QA confidence, severity, impact selection and numeric version comparison', () => {
  const flat = skill.replace(/\s+/g, ' ');
  expect(flat).toContain('For QA findings, assign confidence (1–10) from replay/code evidence');
  expect(flat).toContain("retain Step 4.7's severity, not a severity inferred from confidence");
  expect(flat).toContain('A probe is affected when its entrypoint, dependencies, contract or replay inputs change');
  expect(flat).toContain('If impact is uncertain, rerun it');
  expect(flat).toContain('Compare dotted version components as integers from left to right');
});

test('review emits scope check after the plan audit and before the checklist', () => {
  const markers = [
    '{{SCOPE_DRIFT}}', '{{SECTION:plan-completion}}',
    '## Step 2: Read the checklist',
  ];
  const positions = markers.map(marker => skill.indexOf(marker));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  const audit = readFileSync(join(root, 'review/sections/plan-completion.md'), 'utf8').replace(/\s+/g, ' ');
  expect(audit).toContain('When continuing after the audit (no HIGH-impact gate, or option B/C), emit the single final Scope Check');
  expect(audit).toContain("Step 1.5's provisional notes and this plan context");
  expect(audit).toContain('Emit Step 1.5\'s Scope Check once without plan fields');
  expect(skill).not.toContain('Finish Step 1.5 here');
});

test('review composes confidence-tagged findings into one final report with explicit incomplete coverage', () => {
  const flat = skill.replace(/\s+/g, ' ');
  expect(flat).toContain('Use CRITICAL/INFORMATIONAL labels in the finding format');
  expect(flat).toContain("Step 5.8 combines these finding lines with the checklist's action groups");
  const report = flat.slice(flat.indexOf('### Report the final review'), flat.indexOf('{{LEARNINGS_LOG}}'));
  expect(report).toContain('Emit one final report, merging all reviewers rather than concatenating their reports');
  expect(report).toContain('counts final unresolved non-advisory defects');
  expect(report).toContain('State INCOMPLETE if `COMPLETED` is false, even when N=0');
  expect(report).toContain("Use the checklist's action groups with confidence-tagged finding lines");
  expect(report).toContain('Keep fixed, skipped and advisory items separate from unresolved defects; retain their dispositions');
  expect(report).toContain("Append Step 4.7's single `## Exploratory QA and Verification Results` section");
  expect(report).toContain('Neither coverage gaps nor advice are defects');
});

test('small-diff persistence uses an empty specialist map without manufacturing skipped coverage', () => {
  const army = generateReviewArmy({ skillName: 'review', tmplPath: 'review/SKILL.md.tmpl',
    host: 'claude', paths: HOST_PATHS.claude });
  expect(army).toContain('For DIFF_LINES < 50, keep `specialists: {}`; do not manufacture per-specialist scope records');
  expect(army).toContain('Otherwise record each considered specialist');
  expect(skill).toContain("Use Step 4.6's `specialists` object unchanged, including its empty small-diff map");
  const ship = readFileSync(join(root, 'ship/sections/review-army.md.tmpl'), 'utf8');
  expect(ship).toContain('`specialists`: `{}` for a small-diff skip');
  expect(skill).toContain('every required Step 4.7 probe passes');
  expect(ship).toContain('all required probes pass');
});

test('caller QA runs charter and setup after resource loading and has a severity for unmatched functional failures', () => {
  for (const skillName of ['review', 'ship']) {
    const body = generateQAReview({ skillName, tmplPath: '', host: 'claude', paths: HOST_PATHS.claude });
    const preparation = body.indexOf(skillName === 'review'
      ? '**1. Set the charter and isolation.**'
      : 'Run the shared preflight;');
    const probes = body.indexOf('**3. Run smoke and plan checks.**');
    expect(preparation).toBeGreaterThan(-1);
    if (skillName === 'review') {
      const readiness = body.indexOf('**2. Check readiness and list required checks.**');
      expect(readiness).toBeGreaterThan(preparation);
      expect(probes).toBeGreaterThan(readiness);
      expect(body.slice(preparation, readiness).replace(/\s+/g, ' ')).toContain('complete the shared isolation/permission preflight before setup');
    } else expect(preparation).toBeGreaterThan(body.indexOf('**2. List required checks.**'));
    expect(probes).toBeGreaterThan(preparation);
    expect(body.replace(/\s+/g, ' ')).toContain('unmatched functional failures are `functional-contract`, `CRITICAL`');
    expect(body).toContain('Setup/permission blockers are not defects');
    expect(body).toContain('Test creation needs user approval');
    expect(body).toContain('Return verified defects to Fix-First');
    expect(body).not.toContain('for parent approval');
  }
});

test('review prepares context and deduplicates before classifying findings', () => {
  const positions = [
    '## Step 3.5: Slop scan', '## Step 3.6: Gather review context',
    '{{LEARNINGS_SEARCH}}', '{{ASIDE_RESEARCH}}', '## Step 4: Critical pass',
    '## Step 5: Fix-First Review', '{{CROSS_REVIEW_DEDUP}}',
    '**Keep decisions through fix cycles:**', '### Step 5a: Classify each finding',
  ].map(marker => skill.indexOf(marker));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  expect(skill).toContain('findings before Step 5a classification');
});

test('review owns the complete persistence contract after the adversarial read', () => {
  const step = skill.slice(skill.indexOf('## Step 5.8: Persist Eng Review result'));
  expect(skill.indexOf('{{SECTION:adversarial}}')).toBeLessThan(skill.indexOf('## Step 5.8: Persist Eng Review result'));
  expect(adversarial).not.toContain('### Before persisting Eng Review (Step 5.8)');
  for (const contract of [
    'repeat Steps 3–5', 'at most 3 fix cycles', 'final zero-edit pass, reconcile',
    'by structural identity', 'advisory/defect kind',
    'original `evidence_paths`/`helper_target`', 'without requiring deleted pre-extraction blocks',
    'Current findings determine recurring defects and unresolved counts', 'earlier fixes do not suppress them',
    'snapshot_covered_paths',
    'raw bytes equal the bound snapshot blobs', 'prior-cycle, supplied or prior-record coverage',
    'REVIEW_START', 'COMPLETED', 'CONVERGED', 'CYCLES', 'Step 4.7',
    'native Step 4.8 adversarial pass', 'means false, as does a failed native review',
    'optional outside', 'their own incomplete records when unavailable', 'named-risk',
    'zero counts', '`completed:false`', '`specialists`', '`findings`',
    'verified exploratory QA findings',
    'approved **and completed**', 'explicit Skip', 'sharedLibsFingerprint',
    '`review_binding`', 'validated captured branch',
  ]) expect(step.toLowerCase().replace(/\s+/g, ' ')).toContain(contract.toLowerCase());
  expect(step.indexOf('### 1. Re-review after edits'))
    .toBeLessThan(step.indexOf('### 2. Fill the record'));
  expect(step.indexOf('### 2. Fill the record'))
    .toBeLessThan(step.indexOf('~/.claude/skills/gstack/bin/gstack-review-log'));
  expect(step).toContain('`quality_score` is Step 4.6\'s specialist score');
  expect(step).toContain('unresolved non-advisory core defects still count');
  expect(step.indexOf('Pre-Landing Review: N issues (X critical, Y informational)'))
    .toBeGreaterThan(step.indexOf('~/.claude/skills/gstack/bin/gstack-review-log'));
  expect(step).toContain('`## Exploratory QA and Verification Results`');
});

test('review distinguishes required native coverage from optional outside coverage', () => {
  const section = readFileSync(join(root, 'review/sections/adversarial.md'), 'utf8');
  expect(section).toContain('Only this optional outside adversarial pass is non-blocking');
  expect(section).not.toContain('All errors are non-blocking');
  expect(section).toContain('The native pass is required for Step 5.8 completion');
  expect(skill).toContain('Core findings use the confidence gates below');
  expect(skill).toContain('Step 4.6 applies its specialist gates');
});

test('review identifies probe selection, report assets and the detected diff base', () => {
  const generated = generateQAReview({ skillName: 'review', tmplPath: 'review/SKILL.md.tmpl',
    host: 'claude', paths: HOST_PATHS.claude });
  const checklist = readFileSync(join(root, 'review/checklist.md'), 'utf8');
  expect(generated).toContain('Smoke: 5 minutes/12 probes, one success and the riskiest changed failure/edge');
  expect(generated).toContain('Required even for small diffs or missing plans/servers');
  expect(generated.replace(/\s+/g, ' ')).toContain("Use checklist severity; unmatched functional failures are `functional-contract`, `CRITICAL`");
  expect(generated).toContain('Setup/permission blockers are not defects');
  expect(generated).toContain('Test creation needs user approval');
  expect(generated).toContain("Read QA\'s `templates/functional-report-template.md`. Title it");
  expect(generated).toContain('Link every checkpoint');
  expect(generated).toContain('No second report');
  expect(checklist).toContain('merge-base diff from the caller');
  expect(checklist).not.toContain('git diff origin/main');
});

test('caller QA defines execution, evidence ownership and report adaptation before handoff', () => {
  for (const skillName of ['review', 'ship']) {
    const generated = generateQAReview({ skillName, tmplPath: `${skillName}/SKILL.md.tmpl`,
      host: 'claude', paths: HOST_PATHS.claude }).replace(/\s+/g, ' ');
    for (const contract of [
      'Only the parent runs report-only discovery',
      'Never overwrite another run',
      'Follow the shared Probe loop for smoke checks, replays and revalidation until the smoke limit',
      'using the same procedure but no smoke guard; never reset the clock',
      'Read agent/user updates and await results without batching them with reporting/logging',
      'Compare each probe\'s recorded source, tests, contracts, commands and fixtures (or input fingerprint) with current inputs, even without updates',
      'Re-review changed or uncertain coverage',
      "Use checklist severity",
    ]) expect(generated).toContain(contract);
    const shared = generateQAExploratory({ skillName: 'qa', tmplPath: '', host: 'claude', paths: HOST_PATHS.claude });
    for (const contract of ['First demonstrate success: output AND durable effects',
      'Wait for successful checkpoint publication before dispatch',
      'Replay the exact failing command/request from the same initial fixture state']) {
      expect(shared).toContain(contract);
    }
    if (skillName === 'review') {
      expect(generated).toContain('Title it `## Exploratory QA and Verification Results`');
      expect(generated).toContain('keep metadata/outcome tables');
      expect(generated).toContain('demote other headings one level');
      expect(generated).toContain('include it here under `### Browser results`');
      expect(generated).toContain('other headings demoted two levels');
      expect(generated).toContain('Keep browser/functional scores and outcomes separate');
      expect(generated).toContain('save browser baseline/evidence normally');
      expect(generated).toContain('Prepare one provisional QA section');
      expect(generated).toContain('Update affected outcomes/checkpoint links through repairs/revalidation');
      expect(generated).toContain('Continue to Step 4.8 even if blocked');
      expect(generated).toContain('Step 5.8 appends this section once after final findings and decides completion');
    } else {
      expect(generated).toContain('PR section `## Exploratory QA`');
      expect(generated).toContain('fields as subsections');
    }
  }
});

test('review section index follows the actual pre-fix execution order', () => {
  const manifest = JSON.parse(readFileSync(join(root, 'review/sections/manifest.json'), 'utf8'));
  expect(manifest.sections.map((section: { id: string }) => section.id)).toEqual([
    'plan-completion', 'review-army', 'adversarial', 'shared-code-reuse',
  ]);
});

test('review finalization ownership: initialize invocation state and capture the core token before reading', () => {
  const start = skill.slice(skill.indexOf('## Step 3: Get the diff'), skill.indexOf('## Step 3.4:'));
  expect(start).toContain('one invocation action list and CYCLES=0');
  expect(start).toContain('Keep both through re-reviews');
  expect(skill.match(/CYCLES=0/g)).toHaveLength(1);
  expect(start).toContain('gstack-review-log --start review\ngit diff "$DIFF_BASE"');
  expect(start).toContain('Save the printed REVIEW_START for this core candidate before reading its diff');
  expect(start).toContain('Each re-review captures a new token before reading, never at log time');
  expect(start.replace(/\s+/g, ' ')).toContain('Earlier core tokens remain unused');
  expect(start).toContain('Native/outside reviewer attempts own separate PASS_START tokens, not REVIEW_START');
  expect(start).toContain('Step 5.8 finishes only the final core token');
});

test('review finalization ownership: late findings use Fix-First before the bounded parent transition', () => {
  const flat = skill.replace(/\s+/g, ' ');
  const markers = [
    '{{SECTION:adversarial}}',
    '## Step 5: Fix-First Review',
    'Structured approval does not waive advisory/test_stub ASK gates',
    '## Step 5.8: Persist Eng Review result',
    'Edited: increment CYCLES once',
    'No edits: fill the record below',
    '### 2. Fill the record',
    '--finish REVIEW_START',
  ];
  const positions = markers.map(marker => flat.indexOf(marker));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  expect(flat).toContain('Structured approval does not waive advisory/test_stub ASK gates');
  expect(flat).toContain('A pass covers Steps 3–5, including all reviewers before fixes');
  expect(flat).toContain('at most 3 fix cycles');
  expect(flat).toContain('Below 3, repeat Steps 3–5 with a new REVIEW_START');
  expect(flat).toContain('At 3, persist `converged:false` and remaining findings');
  const limit = flat.slice(flat.indexOf('At 3,'), flat.indexOf('No edits:'));
  expect(limit).toContain('by filling and saving the record below');
  expect(limit).toContain('Report nonconvergence and coverage gaps, then STOP this invocation');
  expect(limit).toContain('without a clean summary or a fourth pass');
});

test('review finalization ownership: affected QA reuse cannot replace a full review or erase decisions', () => {
  const step = skill.slice(skill.indexOf('## Step 5.8: Persist Eng Review result'));
  const flat = step.replace(/\s+/g, ' ');
  expect(flat).toContain('On a repeat, execute Steps 3–5 in order');
  expect(flat).toContain('rerun affected probes after source, test, contract, command or fixture changes');
  expect(flat).toContain("At Step 4.7, reuse only this invocation's unchanged-input QA evidence");
  expect(flat).toContain('Reusing a probe never skips a review step');
  expect(flat).toContain("final zero-edit pass, reconcile this invocation's actions with current findings");
  expect(flat).toContain('original `evidence_paths`/`helper_target`');
  expect(flat).toContain('Current findings determine recurring defects and unresolved counts; earlier fixes do not suppress them');
  expect(flat).toContain('Re-read its final-snapshot supporting source and reconfirm the decision');
  expect(flat).toContain('otherwise report its history without a reusable skip');
  expect(flat).toContain('The logger computes `snapshot_covered_paths` from eligible paths whose raw bytes equal the bound snapshot blobs');
  expect(flat).toContain('Never carry prior-cycle, supplied or prior-record coverage forward or build this proof yourself');
  expect(flat).toContain('Fixed advice needs no skip coverage');
  expect(flat).toContain('include this invocation\'s revalidated decisions');
});

test('review finalization ownership: required native completion and optional outside records stay separate', () => {
  const step = skill.slice(skill.indexOf('### 2. Fill the record'));
  const flat = step.replace(/\s+/g, ' ');
  expect(flat).toContain('native Step 4.8 adversarial pass finish');
  expect(flat).toContain('every required Step 4.7 probe passes');
  expect(flat).toContain('Any failed, blocked, inconclusive or not-run required probe means false, as does a failed native review');
  expect(flat).toContain('`/ship` named-risk acceptance cannot complete `/review`');
  expect(flat).toContain('The required in-host adversarial result controls native completion');
  expect(flat).toContain('Optional outside attempts keep their own incomplete records when unavailable');
  expect(flat).toContain('cannot substitute for the native result, or vice versa');
  expect(flat).toContain('structured-review gate still applies');
  expect(flat).toContain('zero counts and `completed:false`');
  expect(flat).toContain('`CONVERGED`: true only for a completed zero-edit pass');
});

test('review finalization ownership: finish only the final core token without log-time capture', () => {
  const step = skill.slice(skill.indexOf('## Step 5.8: Persist Eng Review result'));
  const flat = step.replace(/\s+/g, ' ');
  expect(step.match(/--finish REVIEW_START/g)).toHaveLength(1);
  expect(step).not.toContain('--start review');
  expect(step).not.toContain('--finish PASS_START');
  expect(flat).toContain('Never invent a binding or replace REVIEW_START at log time');
  expect(flat).toContain('finish only the final core token');
  expect(step).toContain('"completed":COMPLETED,"converged":CONVERGED,"cycles":CYCLES');
});

test('review finalization ownership: the plan audit retains its high-impact gate before the final scope check', () => {
  const plan = readFileSync(join(root, 'review/sections/plan-completion.md.tmpl'), 'utf8');
  expect(plan).toContain('INFORMATIONAL except for the HIGH-impact discrepancy question below');
  expect(plan).toContain('resolve that gate before the final Scope Check');
  expect(plan).not.toContain('never blocks the review');
  expect(plan).toContain('{{PLAN_COMPLETION_AUDIT_REVIEW}}');
  const audit = readFileSync(join(root, 'review/sections/plan-completion.md'), 'utf8');
  const gate = audit.indexOf('**HIGH-impact discrepancies** trigger AskUserQuestion');
  expect(gate).toBeGreaterThan(-1);
  expect(gate).toBeLessThan(audit.indexOf('When continuing after the audit (no HIGH-impact gate, or option B/C)'));
  expect(audit).toContain('then it gates via AskUserQuestion');
  expect(audit).toContain('A ends this invocation before code review or implementation');
  expect(audit).toContain('after implementation, start a fresh /review');
  expect(audit).toContain('B queues the approved TODO changes for Step 5, not this read-only audit');
  expect(audit).toContain('B/C continue to the final Scope Check and Step 2');
  expect(audit).toContain('None of these choices authorizes shipping or waives required verification');
  expect(skill).toContain("including Step 1.5's approved TODO changes");
});

test('review scope notes remain provisional until the plan section emits the only final scope check', () => {
  const scope = generateScopeDrift({ skillName: 'review', tmplPath: 'review/SKILL.md.tmpl',
    host: 'claude', paths: HOST_PATHS.claude }).replace(/\s+/g, ' ');
  expect(scope).toContain('Keep these notes provisional. Next, execute the plan-completion section');
  expect(scope).toContain('it resolves the HIGH-impact decision and emits the single final Scope Check before Step 2');
  expect(scope).not.toContain('Scope Check: [CLEAN');
  expect(scope).not.toContain('available plan-audit results');
});

test('review confidence uses its severity labels without an undefined P0 exception', () => {
  const ctx: TemplateContext = { skillName: 'review', tmplPath: 'review/SKILL.md.tmpl',
    host: 'claude', paths: HOST_PATHS.claude };
  const confidence = generateConfidenceCalibration(ctx);
  expect(confidence).toContain('Only report a suspected release-blocking catastrophe');
  expect(confidence).toContain('widespread data loss, total outage or system-wide compromise');
  expect(confidence).toContain('label it CRITICAL and explicitly speculative');
  expect(confidence).toContain('[CRITICAL]');
  expect(confidence).toContain('[CRITICAL|INFORMATIONAL]');
  expect(confidence).not.toMatch(/\bP[012]\b/);
  expect(confidence).toContain('If you cannot quote the motivating line(s), the finding is unverified');
  const flat = confidence.replace(/\s+/g, ' ');
  for (const rule of [
    '| 9-10 | Specific code verifies a concrete bug or exploit. | Show normally |',
    '| 7-8 | High-confidence pattern match; very likely correct. | Show normally |',
    '| 5-6 | Moderate; could be a false positive. | Show with caveat:',
    'Medium confidence, verify this is actually an issue',
    '| 3-4 | Suspicious but may be fine. | Suppress from main report. Include in appendix only. |',
    '| 1-2 | Speculation. | Only report a suspected release-blocking catastrophe',
    'Quote the specific code line', 'file:line and verbatim text',
    'For a missing field, quote its class definition; for a nullable value, its initialization; for a race, both sides',
    'Force its confidence to 4-5: use 4 for appendix-only reporting, or 5 only when the finding belongs in the main report with the medium-confidence caveat below',
    'Never invent speculative confidence 7+',
    'read and quote their generating metaclass, descriptor, ORM Meta block, migration, decorator or schema',
    'Missing literal names in the class body or grep results do not prove absence',
    'If the user confirms a reported finding scored < 7 is real, log the corrected pattern as a learning',
  ]) expect(flat).toContain(rule);
  expect(confidence.indexOf('Pre-emit verification gate')).toBeLessThan(confidence.indexOf('| Score |'));
  expect(confidence).not.toContain('FP classes the gate kills');
  expect(confidence).not.toContain('1539-framework-aware-review.md');
  expect(generateConfidenceCalibration({ ...ctx, skillName: 'ship' })).toContain('Only report if severity would be P0');
});

test('review names the lifecycle and record owners before using their persistence rules', () => {
  const start = skill.slice(skill.indexOf('## Step 3: Get the diff'), skill.indexOf('## Step 3.4:')).replace(/\s+/g, ' ');
  expect(start).toContain('An invocation is this /review run; a pass reviews one candidate before any fixes');
  expect(start).toContain('REVIEW_START / PASS_START | Opaque start receipts from the logger');
  expect(start).toContain('a matching key alone never proves a prior Skip is reusable');
  expect(start).toContain("`review_binding` | The logger's proof tying a finished review to its captured candidate");
  expect(start).toContain('`snapshot_covered_paths` | Supporting advice files the logger proved byte-identical to that candidate');
  expect(start).toContain('Used by the prior-Skip checker, never supplied by the reviewer');
});

test('review invocation-local advice reuse keeps raw-source and changed-decision gates', () => {
  const decisions = skill.slice(skill.indexOf('**Keep decisions through fix cycles:**'),
    skill.indexOf('### Step 5a:')).replace(/\s+/g, ' ');
  for (const contract of [
    'Immediately save completed AUTO-FIX/fix and explicit Skip actions',
    'keeping defects separate from advice',
    "retain the helper's fingerprint, `advisory`, `evidence_paths` and `helper_target`",
    're-read every supporting caller and helper destination',
    'including secondary callers and transformed/indirect paths',
    'Compare their raw source with the decision evidence',
    'Unrelated auto-fixes do not reopen unchanged identity, contract and tradeoffs',
    'Material proposal, behavior, migration or risk changes require a new question',
    "cannot suppress new/recurring defects or replace Step 5.0's prior-review checker",
  ]) expect(decisions).toContain(contract);
});

for (const skillName of ['review', 'ship']) {
  const ctx: TemplateContext = { skillName, tmplPath: `${skillName}/SKILL.md.tmpl`,
    host: 'claude', paths: HOST_PATHS.claude };
  const army = generateReviewArmy(ctx);
  const flat = army.replace(/\s+/g, ' ');

  test(`${skillName} clarity: terminal failure permits independent work but never certifies coverage`, () => {
    expect(flat).toContain('Confirm that each task has finished or is stopped');
    expect(flat).toContain('A timeout alone does not prove termination');
    expect(flat).toContain("If a reader or writer is still active, wait; if its state is unknown, inspect its task/process status");
    expect(flat).toContain("If you cannot confirm it stopped, use the parent's Fix-First stop path without edits");
    expect(flat).toContain('Continue independent evidence collection after a terminal failure');
    expect(flat).toContain('Missing dispatched coverage remains incomplete, never completed or clean');
    expect(flat).not.toContain('Specialists are additive — partial results are better than no results');
    const redTeam = flat.slice(flat.indexOf('### Red Team dispatch'));
    expect(redTeam).toContain('confirm it stopped and record its review as incomplete, just as for other specialists');
    expect(redTeam).toContain('original specialist outputs and rerun stages 1–7');
  });

  test(`${skillName} clarity: ordered specialist merge separates validation from scoring and provenance`, () => {
    const markers = ['#### 1. Parse outputs', '#### 2. Validate severity',
      '#### 3. Identify and merge', '#### 4. Apply specialist confidence gates',
      '#### 5. Score and present specialists', '#### 6. Save specialist activity',
      '#### 7. Hand off to Fix-First'];
    const positions = markers.map(marker => army.indexOf(marker));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a, b) => a - b));
    const validation = flat.slice(flat.indexOf('#### 2.'), flat.indexOf('#### 3.'));
    expect(validation).toContain('core and specialist findings');
    expect(validation).toContain('remove `advisory` and retain its `CRITICAL` severity');
    expect(validation).toContain('Never downgrade severity');
    expect(validation).toContain('Valid INFORMATIONAL advisories remain advisory');
    const merge = flat.slice(flat.indexOf('#### 3.'), flat.indexOf('#### 4.'));
    expect(merge.indexOf('Partition defects and advisories')).toBeLessThan(merge.indexOf('grouping by fingerprint'));
    for (const gate of ['sharedLibsFingerprint', 'literal JSON on stdin', 'never trust a supplied hash',
      'Missing/malformed metadata cannot deduplicate', 'highest confidence', '+1 (cap at 10)',
      'distinct specialists', 'all source names', 'Core findings never earn a specialist confidence boost']) {
      expect(merge).toContain(gate);
    }
    for (const gate of ['Confidence 7+', 'Confidence 5-6', 'Confidence 3-4', 'Confidence 1-2']) expect(army).toContain(gate);
    const scoring = flat.slice(flat.indexOf('#### 5.'), flat.indexOf('#### 6.'));
    expect(scoring).toContain('Only specialist findings enter this header and `quality_score`; core findings do not');
    expect(scoring).toContain('NON-advisory');
    expect(scoring).toContain('quality_score = max(0, 10 - (critical_count * 2 + informational_count * 0.5))');
    expect(scoring).toContain('unresolved-defect totals');
    expect(flat).toContain('Advisory findings COUNT in the stats `findings` field');
    expect(flat).toContain('Count only findings that specialist actually returned');
    expect(flat).toContain('core-only advice must not create a specialist dispatch or finding');
    expect(flat).toContain('ASK-only');
  });

  test(`${skillName} clarity: shared-code reuse gives executable decisions and retains checker safeguards`, () => {
    const reuse = generateSharedCodeReuse(ctx).replace(/\s+/g, ' ');
    const markers = ['1. **Read the evidence.**', '2. **Run the checker.**',
      '3. **Act on its result.**', '4. **Persist through the logger.**'];
    const positions = markers.map(marker => reuse.indexOf(marker));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a, b) => a - b));
    for (const gate of ['first-party authored provenance', 'all supporting callers and the helper destination',
      '--check-shared-libs REVIEW_START', "<<'GSTACK_SHARED_LIBS_REUSE_JSON'", 'literal JSON on stdin',
      '`reusable: true`', 'False, command failure or unreadable output', 'never suppression',
      'Do not supply your own snapshot, prior record or coverage', 'sharedLibsFingerprint',
      'without consuming/replacing it', 'actual repo, raw branch and current snapshot',
      'completed/converged', 'verified binding', 'explicit Skip', 'logger-versioned `snapshot_covered_paths`',
      'older unversioned coverage', 'Sanitized branch names are not identity', 'canReuseSharedLibsAdvisory',
      'byte-for-byte with its blob', 'assume-unchanged, skip-worktree', 'sparse index', 'symlinks/ancestors',
      'submodules', 'ignored/outside or unreadable', 'active/unknown Git filters', 'encodings and line conversion',
      'fsmonitor and optional locks', 'never uses external diff/textconv', 'Unknown evidence fails closed',
      'logger recomputes final coverage', 'Real defects retain normal Fix-First handling independently']) {
      expect(reuse).toContain(gate);
    }
  });
}

test('review clarity: settlement gates edits separately from incomplete required coverage', () => {
  const fix = skill.slice(skill.indexOf('## Step 5: Fix-First Review'), skill.indexOf('{{CROSS_REVIEW_DEDUP}}')).replace(/\s+/g, ' ');
  expect(fix).toContain('every dispatched reader has returned or is confirmed stopped');
  expect(fix).toContain('active or unknown reader/writer');
  expect(fix).toContain('persist incomplete at Step 5.8 and STOP without edits');
  expect(fix).toContain('Terminal failure does not block fixes from independent evidence');
  expect(fix).toContain('Missing required output still makes the pass incomplete');
});

test('review clarity: Greptile reply choices never substitute for Fix-First approval', () => {
  const fix = skill.slice(skill.indexOf('## Step 5: Fix-First Review'), skill.indexOf('{{CROSS_REVIEW_DEDUP}}'));
  expect(fix).toContain('VALID & ACTIONABLE Greptile findings');
  const greptile = skill.slice(skill.indexOf('### Greptile comment resolution'), skill.indexOf('## Step 5.8:'));
  const flat = greptile.replace(/\s+/g, ' ');
  expect(flat).toContain('Step 5c alone supplies A) Fix / B) Skip');
  expect(flat).not.toContain('A: Fix it now, B: Acknowledge, C: False positive');
  expect(flat).toContain('reply decisions, not code approval');
  expect(flat).toContain('B) Propose a code change');
  expect(flat).toContain('return to Steps 5c–5d with an ASK proposal');
  expect(flat).toContain('Show the exact change and any `test_stub`; wait for approval before editing');
  expect(flat).toContain('no new fix permission');
});

test('ship review clarity: parent settlement gate precedes classification and cannot waive coverage', () => {
  const ship = readFileSync(join(root, 'ship/sections/review-army.md.tmpl'), 'utf8');
  const gate = ship.slice(ship.indexOf('## Step 9.4:'), ship.indexOf('1. **Classify')).replace(/\s+/g, ' ');
  expect(gate).toContain("Before edits, inspect every dispatched reader/writer's handle");
  expect(gate).toContain('Wait for return or confirm termination');
  expect(gate).toContain('otherwise log incomplete through items 5–6 and STOP without edits');
  expect(gate).toContain('After terminal failure, independent evidence may support fixes');
  expect(gate).toContain('missing dispatched output still blocks continuation, even with a QA exception');
});
