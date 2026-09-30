import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { ALL_HOST_CONFIGS } from '../hosts';
import { generateAdversarialStep, generatePlanCompletionGateShip } from '../scripts/resolvers/review';
import { generateQAReview } from '../scripts/resolvers/qa';
import { HOST_PATHS } from '../scripts/resolvers/types';
import { readWorkflowExcerpt } from './helpers/workflow-excerpt';

const readShip = () => readWorkflowExcerpt('ship/SKILL.md', '# Ship:', '## Important Rules');

test('Ship initializes and applies its smoke guard independently of required plan checks', () => {
  for (const host of ALL_HOST_CONFIGS) {
    const body = generateQAReview({ host: host.name, skillName: 'ship', tmplPath: '', paths: HOST_PATHS[host.name] });
    expect(body).toContain('Run the shared preflight; start its smoke guard once. Guard every smoke probe.');
    expect(body.indexOf('start its smoke guard once')).toBeLessThan(body.indexOf('**3. Run smoke and plan checks.**'));
    expect(body).toContain('Required even for small diffs or missing plans/servers');
    expect(body).toContain('Then run required plan checks, even after smoke expires');
    expect(body).toContain('using the same procedure but no smoke guard; never reset the clock');
  }
});

test('ship uses the PR template headings instead of a competing combined QA report', () => {
  const ship = readShip().replace(/\s+/g, ' ');
  expect(ship).toContain('PR section `## Exploratory QA`');
  expect(ship).toContain('plans in `## Verification Results`');
  expect(ship).toContain('Read QA\'s `templates/functional-report-template.md`: PR section');
  expect(ship).toContain('Link every checkpoint; no second report');
  expect(ship).not.toContain('## Exploratory QA and Verification Results');
});

test('late adversarial fixes use the same bounded review cycle before release steps', () => {
  const ship = readShip();
  const controller = compact(ship.slice(ship.indexOf('### Ship control flow'), ship.indexOf('## Step 0:')));
  const review = ship.slice(ship.indexOf('## Step 9:'), ship.indexOf('## Step 10:'));
  const adversarial = compact(ship.slice(ship.indexOf('## Step 11:'), ship.indexOf('## Step 12:')));
  expect(review).toContain('queued Steps 10–11 findings');
  expect(compact(review)).toContain('do not run a fourth fixing cycle');
  expect(controller).toContain('Keep the same attempt counts throughout the invocation');
  expect(adversarial).toContain('queued for the parent; do not edit during Step 11');
  expect(compact(review)).toContain("Every repeat starts before the checklist read and captures a fresh REVIEW_START. Finish the complete review and QA before applying any fix in Step 9.4");
  expect(adversarial).toContain('**Required native review incomplete:** STOP');
  expect(adversarial).toContain('Optional outside failures retain their own incomplete records');
  expect(adversarial).toContain('Insert Steps 9, 10 and 11 before the pending Step 11.5 in the work list. Step 9 completes full review before fixes');
  expect(adversarial).toContain('**Native complete with no queued fixes:**');
  expect(adversarial).toContain('no queued fixes');
  expect(adversarial).toContain('then continue to Step 11.5');
  const greptile = ship.slice(ship.indexOf('## Step 10:'), ship.indexOf('## Step 11:'));
  expect(greptile).toContain('queue the approved fix without editing here');
  expect(compact(greptile)).toContain('Finish the saved replies without asking again about completed fixes');
});

test('late source changes repeat affected gates without resetting either allowance', () => {
  const ship = readShip();
  const gate = ship.slice(ship.indexOf('## Step 16:'), ship.indexOf('## Step 17:'));
  const controller = ship.slice(ship.indexOf('### Ship control flow'), ship.indexOf('## Step 0:'));
  const text = compact(controller + gate);
  expect(text).toContain("**Behavior, tests or build inputs changed:** Prompts/templates count as behavior");
  expect(text).toContain("Keep the same attempt counts throughout the invocation");
  expect(text).toContain('Validate the outcome before Step 15');
  const workflow = ship.replace(/\s+/g, ' ');
  expect(workflow).toContain('Increment before each launch or inline takeover');
  expect(workflow).toContain('Increment before each launch or inline takeover, including failed launches');
  expect(workflow).toContain('A stale snapshot is neither a new attempt nor a current audit');
  expect(workflow).toContain('never a third attempt, even after Step 16 changes');
});

const readTemplate = (name: string) => readFileSync(new URL(`../${name}`, import.meta.url), 'utf8');
const compact = (text: string) => text.replace(/\s+/g, ' ');
const reviewTemplate = readTemplate('ship/sections/review-army.md.tmpl');
const coverageTemplate = readTemplate('ship/sections/test-coverage.md.tmpl');
const entryTemplate = readTemplate('ship/SKILL.md.tmpl');
const controlTemplate = entryTemplate.slice(entryTemplate.indexOf('### Ship control flow'), entryTemplate.indexOf('{{SECTION_INDEX:ship}}'));
const control = compact(controlTemplate);
const reviewFlow = compact(reviewTemplate);
const nativeFlow = compact(generateAdversarialStep({ host: 'claude', skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude }));
const finalGate = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
const pushFlow = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 17:'), entryTemplate.indexOf('## Step 18:')));
const commentsFlow = compact(readTemplate('ship/sections/greptile.md.tmpl'));
const docsTemplate = readTemplate('ship/sections/documentation.md.tmpl');
const prTemplate = readTemplate('ship/sections/pr-body.md.tmpl');

test('ship r12 template: the invocation note locates release, attempts and receipts', () => {
  const entry = compact(entryTemplate);
  expect(entry).toContain('outside the product tree and save its absolute path');
  for (const state of [
    'versions, `BUMP_LEVEL`, reviewed tree and attempt counts',
    'Reuse it only for that same scope; a repair never resets approvals or expands them',
    'each approval\'s finding, files and authorized action',
    'handles, original start tokens, terminal states, outputs and queued fixes',
    'command/label, result/counts, timestamp, log and consumed inputs',
    'candidate/id, attempts used, accepted hashes or named blocked exception',
  ]) {
    expect(entry).toContain(state);
  }
  expect(entry).toContain('handles, original start tokens, terminal states, outputs and queued fixes');
  expect(entry).toContain("reuse this invocation's saved level");
  expect(entry).toContain('Otherwise FRESH chooses it in item 2 and ALREADY_BUMPED derives it in item 1');
});

test('ship r12 template: reuse compares recorded observations and actual inputs without resampling judges', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('original command, result/counts, timestamp and log');
  expect(gate).toContain('Compare hashes or complete bytes');
  expect(gate).toContain('consumed files, fixtures, dependencies and execution parameters');
  expect(gate).toContain('complete expanded request, rubric, parameters and builder/runtime dependencies');
  expect(gate).toContain('never resample it');
  expect(gate).toContain('Mandatory reviews still run');
  expect(gate).toContain('Check each test lane\'s receipt as well');
});

test('ship r12 template: undeclared builds differ from unavailable declared prerequisites', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('If none exists, record not applicable and the inspected sources');
  expect(gate).toContain('A missing prerequisite or failed build stops shipping');
  expect(gate).toContain('Confirm terminal completion or termination before another writer runs');
  expect(gate).toContain('Timeout or cancellation acknowledgment alone means STOP until confirmed');
  expect(gate).toContain("**No changes, or the docs-only checks still support the plan:** Continue to stage 3 without a new code review");
  expect(gate).toContain("**Only authored docs or release metadata changed:** Keep Step 8's original child report and counts");
  expect(gate).toContain("**Behavior, tests or build inputs changed:** Prompts/templates count as behavior");
});

test('ship r12 template: docs collection separates stopped work from acceptable coverage', () => {
  const docs = compact(docsTemplate);
  expect(docs).toContain('Terminal completion or confirmed termination is sufficient');
  expect(docs).toContain('Require every field/type, exact audit id, schema, status invariant and actual spawned marker');
  expect(docs).toContain('enforcing prompt/audit-scope permissions and protected-file exclusions');
  expect(docs).toContain('Compare saved base and input hashes with current content');
  expect(docs).toContain('An exited child with missing output is stopped, but its audit is blocked');
  expect(docs).toContain('HEAD and index must be unchanged, existing dirty/untracked user content preserved, and changed paths exactly `files_updated`');
  expect(docs).toContain('Otherwise save post-child hashes, status and `documentation_section` for Step 16');
  expect(docs).toContain('A failed check or `blocked` result goes to recovery, even with valid JSON');
  expect(docsTemplate).toContain('**Subagent prompt:**');
  expect(docsTemplate).toContain('**Parent processing:**');
});

test('ship r12 template: review finalization precedes one prioritized return table', () => {
  const persist = reviewTemplate.indexOf('6. Persist the review result');
  const route = reviewTemplate.indexOf('### Decide whether to repeat Step 9');
  expect(persist).toBeGreaterThanOrEqual(0);
  expect(route).toBeGreaterThan(persist);
  const exits = reviewFlow.slice(reviewFlow.indexOf("### Decide whether to repeat Step 9"));
  const missing = exits.indexOf('**Dispatched reviewer output missing:** STOP');
  const cap = exits.indexOf('**Third fixing cycle reached (`CYCLES >= 3`):** STOP');
  const fixing = exits.indexOf('**Fixes applied below the cap:**');
  const zeroFix = exits.indexOf('**No edits in this pass:** Resolve the required-probe gate below. Only after it clears may you continue to Step 10');
  for (const position of [missing, cap, fixing, zeroFix]) expect(position).toBeGreaterThanOrEqual(0);
  expect(missing).toBeLessThan(cap);
  expect(cap).toBeLessThan(fixing);
  expect(fixing).toBeLessThan(zeroFix);
  expect(compact(reviewTemplate)).toContain('Finish and log this pass before choosing the next step');
  expect(compact(reviewTemplate)).toContain('Complete items 5–6 exactly once with the original REVIEW_START');
});

test('ship r6 template: one progress note defines surviving state and per-pass content tokens', () => {
  const entry = compact(entryTemplate);
  expect(entry).toContain('one private Markdown **invocation record**');
  expect(entry).toContain('**Next steps:** one ordered work list, with the current step marked');
  expect(entry).toContain('tracked and non-ignored untracked files');
  expect(entry).toContain('`gstack-wtree` prints a Git tree hash');
  expect(entry).toContain('**start token** is the opaque value returned by `gstack-review-log --start` before it reads the diff');
  expect(entry).toContain('Never borrow or replace a token');
  expect(entry).toContain('each approval\'s finding, files and authorized action');
});

test('ship r6 template: plan obligations precede learnings and scope drift even without a plan', () => {
  const plan = readTemplate('ship/sections/plan-completion.md.tmpl');
  const route = compact(plan.slice(0, plan.indexOf('**Dispatch this step')));
  const steps = ['1. Dispatch the audit, validate its result and resolve its Gate Logic',
    "2. Collect the plan's executable checks in Step 8.1; do not run them yet",
    '3. Run Step 8.2 Scope Drift',
    '4. Run Prior Learnings, including its setting question when offered, then proceed to Step 9 for review and QA'];
  const positions = steps.map(step => route.indexOf(step));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  const noPlan = generatePlanCompletionGateShip({ host: 'claude', skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude });
  expect(noPlan).toContain('Skip only the plan completion audit');
  expect(noPlan).toContain('Continue with Step 8.1, Scope Drift and Prior Learnings; Step 9 QA still runs');
  expect(noPlan).not.toContain('Skip entirely');
  const sections = ['{{PLAN_COMPLETION_GATE_SHIP}}', '{{PLAN_VERIFICATION_EXEC}}',
    '{{SCOPE_DRIFT}}', '{{LEARNINGS_SEARCH:query=release ship version changelog merge pr}}'];
  const actual = sections.map(section => plan.indexOf(section));
  expect(actual.every(position => position >= 0)).toBe(true);
  expect(actual).toEqual([...actual].sort((a, b) => a - b));
  expect(entryTemplate.indexOf('{{SECTION:plan-completion}}')).toBeLessThan(entryTemplate.indexOf('{{SECTION:review-army}}'));
});

test('ship r6 template: an approved rebump logs the written version rather than its initial state', () => {
  const bump = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 12:'), entryTemplate.indexOf('{{SECTION:changelog}}')));
  expect(bump).toContain('Record the release decision after a version was actually written');
  expect(bump).toContain('including an approved ALREADY_BUMPED rebump');
  expect(bump).toContain('Skip unchanged versions and manifest-only repairs');
  expect(bump).not.toContain('skip if ALREADY_BUMPED');
  expect(bump).toContain('Only approval changes the existing version');
  expect(bump).toContain('Best-effort, non-interactive, non-blocking');
});

test('ship r6 template: late-change routing runs prerequisites before final evidence without a circular gate', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  const build = gate.indexOf('### 1. Finish writers and prepare outputs');
  const route = gate.indexOf('### 2. Choose the change route');
  const docs = gate.indexOf('### 3. Resolve documentation freshness');
  const verify = gate.indexOf('### 4. Verify the frozen candidate');
  expect(build).toBeGreaterThan(0);
  expect(route).toBeGreaterThan(build);
  expect(docs).toBeGreaterThan(route);
  expect(verify).toBeGreaterThan(docs);
  expect(gate).toContain('This repair excludes Step 14.5 because the rebuild can change generated docs. Step 16 restarts at stage 1');
  expect(gate).toContain('Insert `5–11.5 → 12–14 → 16` before the pending Step 17, then stop this step. This repair excludes Step 14.5');
  expect(gate).toContain('Compare hashes or complete bytes of its saved and current consumed files, fixtures, dependencies and execution parameters');
  expect(gate).toContain('changed or unknown dependencies require a rerun');
  expect(gate).toContain('Freeze inputs through verification and push');
  expect(gate).toContain('If content changes during or after verification, restart at stage 1 and complete all five stages before Step 17');
});

test('ship r6 template: docs attempts count at launch and exhausted late changes never open a third attempt', () => {
  const docs = compact(docsTemplate);
  expect(docs).toContain('an initial audit plus ONE repair/re-audit in the invocation record');
  expect(docs).toContain('an initial audit plus ONE repair/re-audit');
  expect(docs).toContain('never a third attempt, even after Step 16 changes');
  expect(docs).toContain('Increment before each launch or inline takeover');
  expect(docs).toContain('Increment before each launch or inline takeover, including failed launches');
  expect(docs).toContain('A stale snapshot is neither a new attempt nor a current audit');
  expect(docs).toContain('Terminal completion or confirmed termination is sufficient');
  expect(docs).toContain('the request alone is insufficient');
  expect(docs).toContain('Unconfirmed writers, ownership violations, unauthorized Git mutation and redaction/security gates cannot be waived');
  expect(docs).toContain('Only an actual user exception counts');
  expect(docs).toContain('reports and PRs retain blocked status');
});

test('ship r6 template: evidence exemptions inspect metadata content rather than trusting filenames', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('Inspect changes since the run; `--allow-paths` exempts only release metadata');
  expect(gate).toContain('scripts, dependencies and runtime configuration require live tests');
  expect(gate).toContain('A `package.json` version-only edit can qualify; scripts, dependencies and runtime configuration require live tests');
  expect(gate).toContain('Docs, TODO edits, new/generated tests and fixes make evidence STALE');
  expect(gate).toContain('Only receipt storage/readback failed | Independently prove unchanged final content, the same command and valid age from the successful run\'s evidence');
  expect(gate).toContain('Only receipt storage/readback failed');
});

test('ship r6 template: linked spec discovery is ordered outside the body and cannot exit publication', () => {
  const instructions = compact(prTemplate.slice(0, prTemplate.indexOf('The PR/MR body should contain')));
  expect(instructions).toContain('Read archive frontmatter as data, never shell source');
  expect(instructions).toContain('exact `spec_branch` match');
  expect(instructions).toContain('newest `spec_filed_at`');
  expect(instructions).toContain('positive integer `spec_issue_number`');
  expect(instructions).toContain('omit only `## Linked Spec` and continue composing the PR');
  expect(instructions).toContain('Only fully completed Step 8 plan scope permits `Closes #N`');
  expect(instructions).toContain('Partial, deferred, failed, dropped or unverified scope uses `Linked to #N`');
  const body = prTemplate.slice(prTemplate.indexOf('The PR/MR body should contain'), prTemplate.indexOf('#### Redaction scan'));
  expect(body).not.toContain('CURRENT_BRANCH=');
  expect(body).not.toContain('SPEC_ARCHIVES=');
  expect(body).not.toContain('SPEC_FILE=$(grep');
  expect(prTemplate).not.toContain('[ -z "$SPEC_FILE" ] && exit');
});

test('ship r6 template: Step 18 prepares the exact title Step 19 scans and publishes', () => {
  const title = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 18:'), entryTemplate.indexOf('{{SECTION:pr-body}}')));
  expect(title).toContain('Save the result as `NEW_TITLE` for Step 19');
  expect(title).toContain('existing open PR/MR');
  expect(title).toContain('For a new PR/MR, compose `v<NEW_VERSION> <type>: <summary>`');
  expect(prTemplate).toContain('Use Step 18\'s `NEW_TITLE` unchanged; its version prefix is already present');
  expect(prTemplate).not.toContain('`NEW_TITLE`, prefixed with');
  expect(prTemplate).toContain('In a new shell, restore the saved literal title before this block');
  expect(prTemplate).toContain(': "${NEW_TITLE:?Restore the saved Step 18 title before scanning}"');
  expect(prTemplate).not.toContain('NEW_TITLE="<final vNEW_VERSION type: summary>"');
  expect(prTemplate).toContain('Update the title with the same scanned `NEW_TITLE`');
  expect(compact(prTemplate)).toContain('exit 3 blocks for HIGH findings');
});

test('ship template consolidation: initial, failed and inline generation attempts share one allowance', () => {
  const allowance = compact(coverageTemplate.slice(0, coverageTemplate.indexOf('````text')));
  expect(allowance).toContain('Maximum 2 generation passes total per invocation');
  expect(allowance).toContain('Count each generation-authorized attempt before dispatch/inline execution');
  expect(allowance).toContain('including the initial audit, failures and zero-test results');
  expect(allowance).toContain('Re-entry never resets it');
  expect(allowance).toContain('Two passes already used means no further generation');
  expect(allowance).toContain('read-only reassessment uses no pass');
  expect(allowance).toContain('30-path/5-tests-per-pass/2-minute per-test caps');
  expect(allowance).toContain('missing permission is not approval');
  expect(coverageTemplate).toContain("confirm it stopped before running the same audit inline");
});

test('ship template consolidation: audit-only authority reaches the actual coverage child prompt', () => {
  const prompt = coverageTemplate.split('````text\n')[1]?.split('\n````')[0] ?? '';
  expect(prompt).toContain('Generation: <allowed|audit-only>; passes used: <N> of 2.');
  expect(prompt).toContain('Audit-only overrides every generation instruction below.');
  expect(prompt.indexOf('Audit-only overrides')).toBeLessThan(prompt.indexOf('{{TEST_COVERAGE_AUDIT_SHIP}}'));
  expect(prompt).toContain('Do not commit or push');
  expect(prompt).toContain('return unresolved user decisions to the parent');
  expect(prompt).toContain('"coverage_pct":N,"gaps":N');
  expect(prompt).toContain('"tests_added":["path",...]');
});

test('ship template consolidation: duplicate design defects have one action without losing independent coverage', () => {
  const design = compact(reviewTemplate.slice(reviewTemplate.indexOf('{{DESIGN_REVIEW_LITE}}'), reviewTemplate.indexOf('{{REVIEW_ARMY}}')));
  expect(design).toContain('The parent owns design-lite; the Design specialist is an independent read');
  expect(design).toContain('Before final counting/Fix-First');
  expect(design).toContain('same evidenced design defect at the same path/line');
  expect(design).toContain('one item with both sources and stricter ASK');
  expect(design).toContain('Retain actual specialist stats');
  expect(design).toContain('distinct defects stay separate');
  expect(design).toContain('neither pass substitutes for the other');
  expect(reviewTemplate).toContain('{{DESIGN_REVIEW_LITE}}');
  expect(reviewTemplate).toContain('{{REVIEW_ARMY}}');
  expect(reviewTemplate).toContain('"dispatched":true,"findings":N,"critical":N,"informational":N');
});

test('ship template consolidation: the parent owns one ordered review phase', () => {
  const intro = compact(controlTemplate + reviewTemplate);
  expect(intro).toContain("You, the **parent** running /ship, own advancement");
  expect(intro).toContain('Set CYCLES to 0 on first entry only');
  expect(intro).toContain('**No edits in this pass:**');
  expect(intro).toContain('Start with Steps 1–21 in order, including 11.5 and 14.5');
  expect(intro).toContain('Steps 10–11 queue findings without editing');
  expect(intro).toContain("Finish the complete review and QA before applying any fix in Step 9.4");
  expect(intro).toContain('Keep the same attempt counts throughout the invocation');
  expect(intro).toContain('changed finding scope needs a new decision');
  expect(intro).toContain('Gated/unsupported specialists skip only their dispatch, never QA or Step 11');
  expect(intro).toContain("Apply these decisions in order");
  for (const step of ['specialists (9.1)', 'exploratory QA (9.2.1)', 'fixes and logging (9.4)']) {
    expect(intro.indexOf(step)).toBeGreaterThanOrEqual(0);
  }
  expect(intro.indexOf('specialists (9.1)')).toBeLessThan(intro.indexOf('exploratory QA (9.2.1)'));
  expect(intro.indexOf('exploratory QA (9.2.1)')).toBeLessThan(intro.indexOf('fixes and logging (9.4)'));
});

test('ship template consolidation: every fixing pass persists once before looping or stopping at cycle three', () => {
  const finalize = compact(reviewTemplate.slice(reviewTemplate.indexOf('4. **'), reviewTemplate.indexOf('5. Output summary:')));
  expect(finalize).toContain('Increment CYCLES once if fixes were applied');
  expect(finalize).toContain('Finish and log this pass before choosing the next step');
  expect(finalize).toContain('Complete items 5–6 exactly once with the original REVIEW_START');
  expect(reviewTemplate.indexOf('6. Persist the review result')).toBeLessThan(reviewTemplate.indexOf('### Decide whether to repeat Step 9'));
  expect(finalize).toContain('Then commit named fixed files');
  expect(finalize).toContain('fixes also require `converged:false`');
  expect(reviewFlow).toContain('**Third fixing cycle reached (`CYCLES >= 3`):** STOP and report recurring findings with `converged:false`; do not run a fourth fixing cycle');
  expect(control).toContain('Keep the same attempt counts throughout the invocation');
  expect(reviewFlow).toContain('**Fixes applied below the cap:**');
  expect(reviewFlow).toContain("Every repeat starts before the checklist read and captures a fresh REVIEW_START");
  expect(compact(entryTemplate)).toContain('Reuse waivers only for the same verified pre-existing failures and approved scope');
  expect(reviewTemplate).toContain('--finish REVIEW_START');
  expect(compact(reviewTemplate)).toContain('never recapture at persistence to certify unreviewed fixes');
  expect(reviewTemplate).toContain('`CONVERGED`: completed with zero fixes');
});

test('ship template consolidation: named QA risks remain failed or incomplete and cannot waive other gates', () => {
  const intro = compact(reviewTemplate.slice(reviewTemplate.indexOf('**Required-probe parent gate:**')));
  expect(intro).toContain('explicitly accept each named probe\'s concrete risk. Skipping a fix is not risk acceptance or a passing probe');
  expect(intro).toContain('With completed checklist and dispatched reviewers, failed/unavailable required probes block continuation');
  expect(intro).toContain('Keep actual outcomes and incomplete flags; VERIFY_RESULT stays fail');
  expect(intro).toContain('VERIFY_RESULT stays fail for plan-check exceptions');
  expect(compact(reviewTemplate)).toContain('Record accepted untested risk separately, not as passing verification');
  expect(intro).toContain('cannot waive missing reviewer output, recurring fixes or independent test/security gates');
  expect(compact(reviewTemplate)).toContain('Skipping a fix is not risk acceptance or a passing probe');
  expect(compact(reviewTemplate)).toContain('all required probes pass');
  expect(compact(reviewTemplate)).toContain('Failed, blocked, inconclusive or not-run required probes mean false, never clean');
  expect(compact(reviewTemplate)).toContain('Record accepted untested risk separately, not as passing verification');
});

test('ship template consolidation: late behavioral inputs revisit named gates while docs still get freshness checks', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('Insert `5–11.5 → 12–14 → 16` before the pending Step 17');
  expect(gate).toContain('`5–11.5 → 12–14 → 16`');
  expect(gate).toContain("**Behavior, tests or build inputs changed:** Prompts/templates count as behavior");
  expect(gate).toContain('**Behavior, tests or build inputs changed:** Prompts/templates count as behavior. Insert `5–11.5 → 12–14 → 16` before the pending Step 17');
  expect(gate).toContain('Explain why other changes cannot affect it; changed or unknown dependencies require a rerun');
  expect(gate).toContain("**Only authored docs or release metadata changed:** Keep Step 8's original child report and counts");
  expect(gate).toContain("**No changes, or the docs-only checks still support the plan:** Continue to stage 3 without a new code review");
  expect(gate).toContain("Keep the same attempt counts throughout the invocation");
  expect(gate).toContain('Validate the outcome before Step 15');
  expect(gate).toContain("retain `Documentation: blocked`, its reason and incomplete scope");
  expect(gate).toContain("Validate the outcome before Step 15, then restart Step 16 stage 1");
  expect(gate).toContain('Inspect writer handles, including the docs child');
  expect(gate).toContain('STOP until confirmed');
});

test('ship template consolidation: ledger recovery never waives stale content or failed verification', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('Only receipt storage/readback failed | Independently prove unchanged final content, the same command and valid age from the successful run\'s evidence');
  expect(gate).toContain('Only receipt storage/readback failed');
  expect(gate).toContain('Independently prove unchanged final content, the same command and valid age');
  expect(gate).toContain('Cite its exact command, exit, timestamp and log as **ledger unavailable**, never FRESH. Without that proof, use STALE/MISSING');
  expect(gate).toContain('--allow-paths CHANGELOG.md,VERSION,package.json,agents-digest/gstack-AGENTS.md');
  expect(gate).toContain('Docs, TODO edits, new/generated tests and fixes make evidence STALE');
  expect(gate).toContain('Docs, TODO edits, new/generated tests and fixes make evidence STALE');
  expect(gate).toContain("**New, changed or unwaived test failure:** STOP publication. Run Steps 5–15, starting with Step 5's triage");
  expect(gate).toContain('**New, changed or unwaived test failure:** STOP publication');
});

test('ship has one invocation route and retains each independently bounded allowance', () => {
  const entry = compact(entryTemplate);
  expect(entry).toContain('Every new invocation repeats Steps 1–16, including both reviews and the docs audit');
  expect(entry).toContain('prepare the release (12–15) → verify frozen content (16)');
  expect(entry).toContain('## Step 16: Verification Gate');
  expect(compact(coverageTemplate)).toContain('Maximum 2 generation passes total per invocation');
  expect(reviewFlow).toContain('do not run a fourth fixing cycle');
  expect(compact(docsTemplate)).toContain('an initial audit plus ONE repair/re-audit');
  expect(entry).toContain('Keep the same attempt counts throughout the invocation');
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('against the snapshot saved before Step 12');
  expect(gate).toContain("Content-preserving commits keep valid evidence");
  expect(gate).toContain('Use its actual Step 5 label/command');
  expect(gate).toContain("--label <lane> --expect-cmd '<exact Step 5 command>'");
});

test('ship resolves threshold, branch and installed-asset references without competing instructions', () => {
  expect(entryTemplate).toContain("When Step 7 coverage meets its target");
  expect(entryTemplate).not.toContain('Test coverage gaps within target threshold');
  expect(entryTemplate.indexOf('Save the current branch as `<branch-name>`')).toBeGreaterThan(0);
  expect(entryTemplate.indexOf('Save the current branch as `<branch-name>`')).toBeLessThan(entryTemplate.indexOf('refs/heads/<branch-name>'));
  expect(entryTemplate).toContain('~/.claude/skills/gstack/review/TODOS-format.md');
  expect(entryTemplate).not.toContain('`.claude/skills/review/TODOS-format.md`');
  const skeleton = readTemplate('ship/SKILL.md');
  const row = skeleton.split('\n').find(line => line.startsWith('| exploratory QA before Fix-First'))!;
  expect(row).toContain('`sections/review-army.md`');
  expect(row).not.toContain('below');
});

test('ship distinguishes a changed verified tree from failed test-receipt storage', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('declared generation/build commands in project instructions, manifests, build files and CI');
  expect(gate).toContain("Run declared docs/link/generated-file checks");
  expect(gate).toContain('Docs, TODO edits, new/generated tests and fixes make evidence STALE even without a new code review');
  expect(gate).toContain('Docs, TODO edits, new/generated tests and fixes make evidence STALE');
  expect(gate).toContain('| FRESH (exit 0) | Cite the label, exit, timestamp and log. |');
  expect(gate).toContain('Only receipt storage/readback failed');
  expect(gate).toContain('Without that proof, use STALE/MISSING');
  expect(gate).toContain('Docs, TODO edits, new/generated tests and fixes make evidence STALE');
});

test('ship names the approval scope, probe-risk decision, and native-review recovery', () => {
  const entry = compact(entryTemplate);
  const review = compact(controlTemplate + reviewTemplate);
  const adversarial = compact(generateAdversarialStep({ host: 'claude', skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude }));
  expect(entry).toContain('one private Markdown **invocation record**');
  expect(entry).toContain('each approval\'s finding, files and authorized action');
  expect(entry).toContain('Never borrow or replace a token');
  expect(review).toContain('Use AskUserQuestion: stop for repair (recommended), or explicitly accept each named probe\'s concrete risk');
  expect(review).toContain('explicitly accept each named probe\'s concrete risk. Skipping a fix is not risk acceptance or a passing probe');
  expect(nativeFlow).toContain("One recovery retry is allowed only after a concrete prerequisite correction and restored access");
  expect(nativeFlow).toContain('count it in the invocation record before launch');
  expect(control).toContain('Keep the same attempt counts throughout the invocation');
  expect(nativeFlow).toContain('if the recovery fails, ask for repair and remain blocked');
  expect(adversarial).toContain('Outside-provider output cannot replace this pass');
  expect(entry).toContain('Only the final VERSION/CHANGELOG commit gets the release version and co-author trailer. Do not create a Git tag');
  expect(entry).toContain('encode null/undetermined as -1');
});

test('native recovery has its own bounded retry without resetting fixing or documentation limits', () => {
  const adversarial = compact(generateAdversarialStep({ host: 'claude', skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude }));
  expect(nativeFlow).toContain('One recovery retry is allowed only after a concrete prerequisite correction and restored access');
  expect(reviewFlow).toContain('do not run a fourth fixing cycle');
  expect(compact(docsTemplate)).toContain('an initial audit plus ONE repair/re-audit');
  expect(nativeFlow).toContain('**Required native review incomplete:** STOP');
  expect(adversarial).toContain('STOP and confirm the native task stopped');
  expect(adversarial).toContain('Capture a fresh PASS_START and persist the new attempt separately');
  expect(nativeFlow).toContain('confirm the native task stopped');
  expect(nativeFlow).toContain('One recovery retry is allowed only after a concrete prerequisite correction and restored access');
  expect(nativeFlow).toContain('count it in the invocation record before launch');
  expect(control).toContain('Keep the same attempt counts throughout the invocation');
  expect(nativeFlow).toContain('Without that correction, or if the recovery fails, ask for repair and remain blocked');
  expect(nativeFlow).toContain('if the recovery fails, ask for repair and remain blocked');
  expect(nativeFlow).toContain('not recovery retries');
  expect(reviewFlow).toContain("Finish the complete review and QA before applying any fix in Step 9.4");
});

test('late verified generated outputs are committed before publication without absorbing user files', () => {
  const commit = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 15:'), entryTemplate.indexOf('## Step 16:')));
  const finish = compact(entryTemplate.slice(entryTemplate.indexOf('### 5. Report, then push'), entryTemplate.indexOf('## Step 17:')));
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(commit).toContain('Group VERSION + CHANGELOG + TODOS.md after the feature commits');
  expect(finish).toContain('Commit only approved, verified release changes left uncommitted after Step 15');
  expect(finish).toContain('including generated outputs; use its grouping rules and never create an empty commit');
  expect(finish).toContain('Preserve unrelated user files');
  expect(gate).toContain('Content-preserving commits keep valid evidence');
  expect(gate).toContain('If content changes during or after verification, restart at stage 1 and complete all five stages before Step 17');
  const commitPosition = finish.indexOf('Commit only approved, verified');
  const pushPosition = finish.indexOf('continue to Step 17');
  expect(commitPosition).toBeGreaterThanOrEqual(0);
  expect(pushPosition).toBeGreaterThan(commitPosition);
});

test('missing test suites need a named gap decision rather than a fabricated fresh receipt', () => {
  const tests = compact(readTemplate('ship/sections/tests.md.tmpl'));
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(tests).toContain('If no applicable test suite exists');
  expect(tests).toContain('A) Add tests (recommended), B) Ship with this named testing gap, or C) Stop');
  expect(tests).toContain('Reuse an actual prior B answer only for the same scope and content');
  expect(tests).toContain('declining bootstrap alone is not that approval');
  expect(tests).toContain('Independent build, eval, review and QA gates still apply');
  expect(tests).toContain('A declared but unavailable suite is a blocker, not an absent suite');
  expect(gate).toContain('No test lanes: require Step 5\'s explicit untested-scope approval for final content');
  expect(gate).toContain("or run Steps 5–15, including the no-tests decision, then return to Step 16 stage 1");
  expect(gate).toContain('Report the gap, never FRESH');
});

test('ship template consolidation: remote integration retains all allowances and cannot bypass publication guards', () => {
  const push = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 17:'), entryTemplate.indexOf('## Step 18:')));
  const recovery = pushFlow;
  expect(push).toContain('**If the push fails, STOP.** No Step 19 or publication claim');
  expect(recovery).toContain('Run Steps 5–16 before returning to Step 17. Never rewrite history');
  expect(control).toContain("Keep the same attempt counts throughout the invocation");
  expect(recovery).toContain("fetch and inspect the remote, then merge under Step 3's conflict rules");
  expect(push).toContain('Never force-push');
  expect(recovery).toContain('repeat Step 16 even if content is unchanged before returning to Step 17');
  expect(recovery).toContain('Never bypass failed guards');
  expect(push).toContain('Only a successful push or verified `ALREADY_PUSHED` proceeds');
  expect(push).toContain('No documentation writer runs after push');
});

test('missing dispatched coverage is persisted and stopped before any zero-fix completion', () => {
  const review = compact(controlTemplate + reviewTemplate);
  const branches = reviewFlow.slice(reviewFlow.indexOf("### Decide whether to repeat Step 9"));
  expect(branches.indexOf('Dispatched reviewer output missing')).toBeGreaterThanOrEqual(0);
  expect(branches.indexOf('**Dispatched reviewer output missing:** STOP')).toBeGreaterThanOrEqual(0);
  expect(branches.indexOf('**Fixes applied below the cap:**')).toBeGreaterThanOrEqual(0);
  expect(branches.indexOf('**No edits in this pass:** Resolve the required-probe gate below. Only after it clears may you continue to Step 10')).toBeGreaterThanOrEqual(0);
  expect(branches.indexOf('**Fixes applied below the cap:**')).toBeGreaterThan(branches.indexOf('**Dispatched reviewer output missing:** STOP'));
  expect(branches.indexOf('**No edits in this pass:** Resolve the required-probe gate below. Only after it clears may you continue to Step 10')).toBeGreaterThan(branches.indexOf('**Dispatched reviewer output missing:** STOP'));
  expect(review).toContain('Missing dispatched output uses `status:"unavailable"`, `completed:false` and `converged:false`');
  expect(review).toContain('Pre-Landing Review: INCOMPLETE');
  expect(branches).toContain('Retain queued fixes');
  expect(branches).toContain("If this pass made edits, resume at the next decision");
  expect(branches).toContain("otherwise run a fresh complete Step 9");
  expect(review).toContain('Undispatched host-unsupported/gated specialists do not block');
  expect(review).toContain("Apply these decisions in order");
  expect(review).toContain('**No edits in this pass:** Resolve the required-probe gate below. Only after it clears may you continue to Step 10');
  expect(review).toContain('This cannot waive missing reviewer output');
  expect(review).toContain('`STATUS`: `unavailable` for missing dispatched reviewer output');
  const settlement = review.slice(review.indexOf('## Step 9.4:'), review.indexOf('1. **Classify'));
  expect(settlement).toContain('every dispatched reader/writer\'s handle. Wait for return or confirm termination');
  expect(settlement).toContain('log incomplete through items 5–6 and STOP without edits');
  expect(settlement).toContain('After terminal failure, independent evidence may support fixes');
});

test('external-comment fixes refresh tests and mandatory review without repeating prior decisions', () => {
  const section = compact(readTemplate('ship/sections/greptile.md.tmpl'));
  const finish = section.slice(section.indexOf('**After triage:**'));
  expect(section).toContain('queue the approved fix without editing here');
  expect(finish).toContain('If fixes were approved, save their approvals and comment references');
  expect(reviewFlow).toContain("Finish the complete review and QA before applying any fix in Step 9.4");
  expect(commentsFlow).toContain("Run Step 9's full review/fix loop, then return here");
  const repair = reviewFlow;
  expect(repair).toContain('**Fixes applied below the cap:** Insert Step 5, affected Steps 6–8 and all of Step 9 before the pending Step 10 in the work list');
  expect(repair).toContain('**No edits in this pass:** Resolve the required-probe gate below. Only after it clears may you continue to Step 10');
  expect(finish).not.toContain('before continuing to Step 11');
  expect(finish).toContain('Finish the saved replies without asking again about completed fixes');
  expect(finish).toContain('With no queued fixes, continue to Step 11');
});

test('triage distinguishes absent PRs, successful empty fetches and unavailable evidence', () => {
  const section = compact(readTemplate('ship/sections/greptile.md.tmpl'));
  expect(section).toContain('"status":"complete|no_pr|unavailable"');
  expect(section).toContain('Use `complete` only after a successful fetch, including zero comments');
  expect(section).toContain('`no_pr` only after confirming no PR exists');
  expect(section).toContain('`unavailable` for `gh`/API errors or incomplete classification');
  expect(section).toContain('a nonnegative integer total matching the comments array');
  expect(section).toContain('An unknown or missing status is unavailable, never an empty successful review');
  expect(section).toContain('"Greptile: no PR exists"');
  expect(section).toContain('"Greptile: fetched, zero comments"');
  expect(section).toContain('Include `Greptile triage: UNAVAILABLE (dispatch failed)` and the actual reason');
  expect(section).toContain('Stop a running child and confirm it stopped before continuing');
  expect(section).not.toContain('If no PR exists, `gh` fails, the API errors, or there are zero comments');
});

test('late-change routing compares a saved reviewed snapshot without waiving probe outcomes', () => {
  const receipt = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 11.5:'), entryTemplate.indexOf('## Step 12:')));
  expect(receipt).toContain('~/.claude/skills/gstack/bin/gstack-review-read');
  expect(receipt).toContain("Match each to its saved handle, original token and source");
  expect(receipt).toContain('(`skill:"review"`, `via:"ship"`)');
  expect(receipt).toContain('Step 11 native record (`skill:"adversarial-review"`)');
  expect(receipt).toContain("reject outside-provider or older invocation records");
  expect(receipt).toContain("Require the native record's `review_binding.state` to be `verified`");
  expect(receipt).toContain("All three snapshots must match: its `wtree`, Step 9.4's `review_binding.start_wtree` and `review_binding.end_wtree`");
  expect(receipt).toContain("A mismatch or missing record/field blocks release preparation: report **Review records missing or mismatched**");
  expect(receipt).toContain('Never attach new tokens to old work');
  expect(receipt).toContain("Keep Step 9.4's incomplete flags and the user's exception");
  expect(receipt).toContain("Matching content does not mean the failed or unrun probes passed");
  expect(receipt).toContain("A named probe-risk exception may leave Step 9.4's root `wtree` absent; item 2 still compares its start/end snapshots");
  expect(receipt).not.toContain('phase:"core"');
  expect(receipt).not.toContain('Step 9.5');
  expect(receipt).not.toContain('source:"in-host"');
  expect(receipt).not.toContain('status:"clean"');
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('~/.claude/skills/gstack/bin/gstack-wtree');
  expect(gate).toContain('git diff <reviewed-tree> <current-tree>');
  expect(gate).toContain('against the snapshot saved before Step 12');
  expect(gate).toContain('Missing snapshots block this comparison');
  expect(gate).toContain('Missing snapshots block this comparison, regardless of HEAD equality');
  expect(gate.indexOf('**Reuse a check when its inputs match.**')).toBeGreaterThan(gate.indexOf('### 4. Verify the frozen candidate'));
  expect(gate).toContain('**Check each test lane\'s receipt as well.**');
});

test.each(ALL_HOST_CONFIGS.map(({ name }) => name))('%s: late adversarial fixes have a bounded return path and preserve approvals', host => {
  const ctx = { host, skillName: 'ship', tmplPath: '', paths: HOST_PATHS[host] };
  const text = generateAdversarialStep(ctx);
  const finishPosition = text.indexOf('### Finish the adversarial phase');
  expect(finishPosition).toBeGreaterThanOrEqual(0);
  const finish = compact(text.slice(finishPosition));
  const outcomes = [
    '**Required native review incomplete:**',
    '**Fixes queued after native completion:**',
    '**Native complete with no queued fixes:**',
  ].map(outcome => finish.indexOf(outcome));
  expect(outcomes.every(position => position >= 0)).toBe(true);
  expect(outcomes).toEqual([...outcomes].sort((a, b) => a - b));
  expect(text).toContain('queued for the parent; do not edit during Step 11');
  expect(text).toContain('If A: queue the approved findings without editing here');
  expect(reviewFlow).toContain("Every repeat starts before the checklist read and captures a fresh REVIEW_START");
  expect(reviewFlow).toContain("Finish the complete review and QA before applying any fix in Step 9.4");
  expect(control).toContain('Keep the same attempt counts throughout the invocation');
  expect(finish).toContain('Insert Steps 9, 10 and 11 before the pending Step 11.5 in the work list. Step 9 completes full review before fixes');
  expect(compact(entryTemplate)).toContain('each approval\'s finding, files and authorized action');
  expect(reviewFlow).toContain('do not run a fourth fixing cycle');
  expect(reviewFlow).toContain('**Third fixing cycle reached (`CYCLES >= 3`):** STOP and report recurring findings with `converged:false`; do not run a fourth fixing cycle');
  expect(reviewFlow).toContain('**Fixes applied below the cap:** Insert Step 5, affected Steps 6–8 and all of Step 9 before the pending Step 10 in the work list');
  expect(finish).toContain('**Native complete with no queued fixes:**');
  expect(finish).toContain('**Native complete with no queued fixes:** Finish the memory updates below, then continue to Step 11.5');
  expect(text).toContain('retain the acknowledged findings and failed gate');
  expect(text).toContain('do not report a clean review');
  expect(finish).toContain('**Required native review incomplete:** STOP');
  expect(finish).toContain('Optional outside failures retain their own incomplete records');
  expect(compact(text)).toContain('native completion never credits outside coverage');
  const standalone = generateAdversarialStep({ ...ctx, skillName: 'review' });
  expect(standalone).not.toContain('Before Step 12:');
  expect(standalone).not.toContain('### Finish the adversarial phase');
  expect(standalone).toContain("queue the findings and this approval for Step 5's Fix-First handling");
  expect(standalone).toContain('After edits, the full re-review repeats this same structured invocation and diff scope');
  expect(standalone).toContain('do not start an inner repair loop');
});

test('existing release levels have an explicit recovery rule, not implicit rebump approval', () => {
  const root = entryTemplate;
  const version = compact(root.slice(root.indexOf('## Step 12:'), root.indexOf('## Step 14:')));
  expect(version).toContain('use the first changed component from `baseVersion` to `currentVersion` (major/minor/patch/micro');
  expect(version).toContain('an absent fourth component is zero');
  expect(version).toContain('Continue at item 3, not another automatic bump');
  expect(version).toContain('Only approval changes the existing version');
});

test('distribution setup asks for unknown targets and cannot release before review', () => {
  const root = entryTemplate;
  const distribution = root.slice(root.indexOf('## Step 2:'), root.indexOf('## Step 3:'));
  expect(distribution).toContain('git diff origin/<base> --diff-filter=A --name-only');
  expect(distribution).toContain('a new `package.json` or `Cargo.toml` alone does not establish a publishable');
  expect(distribution).toContain('Ask for unknown targets, registries or access first');
  expect(distribution).toContain('never invent credentials');
  expect(distribution).toContain('include the workflow in tests and review');
  expect(distribution).toContain('Do not publish a release during `/ship`');
});

test('ship entry decisions identify Swift app products and wait on ambiguous merge resolutions', () => {
  const apple = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 0.9:'), entryTemplate.indexOf('## Step 1:')));
  expect(apple).toContain('If the ask is App Store/TestFlight distribution');
  expect(apple).toContain("Read `Package.swift` and its entrypoint");
  expect(apple).toContain('distinguish an app from a library/CLI');
  expect(apple).toContain('If unclear, use AskUserQuestion');
  expect(apple).toContain('AskUserQuestion to identify the target and wait before choosing a release path');
  expect(apple).toContain('For a confirmed app, **STOP and Read');
  const merge = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 3:'), entryTemplate.indexOf('{{SECTION:tests}}')));
  expect(merge).toContain('Try to auto-resolve if they are simple');
  expect(merge).toContain('For complex or ambiguous conflicts, **STOP**, show the conflicting choices');
  expect(merge).toContain('use AskUserQuestion for the needed resolution decision');
  expect(merge).toContain('wait for the answer before editing or continuing');
});

test('ship final preparation discovers declared commands and verifies the versioned digest output', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  expect(gate).toContain('declared generation/build commands in project instructions, manifests, build files and CI');
  expect(gate).toContain('Run them and save results');
  expect(gate).toContain('If none exists, record not applicable and the inspected sources');
  expect(gate).toContain('A missing prerequisite or failed build stops shipping');
  const version = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 12:'), entryTemplate.indexOf('{{SECTION:changelog}}')));
  expect(version).toContain('only when it and committed `agents-digest/gstack-AGENTS.md` exist');
  expect(version).toContain('If `agentsDigest` is false, run `bun scripts/gen-agents-digest.ts` and stage the digest with the bump');
  expect(version).toContain("Before push, verify the committed digest matches generation for the selected VERSION");
});

test('ship publication metadata resolves open state before composing a title', () => {
  const title = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 18:'), entryTemplate.indexOf('{{SECTION:pr-body}}')));
  const query = title.indexOf('gh pr list --head <branch-name> --state open --json number,title,url');
  const prepare = title.indexOf('Prepare the title from that result');
  expect(query).toBeGreaterThanOrEqual(0);
  expect(prepare).toBeGreaterThan(query);
  expect(title).toContain('glab mr list --source-branch <branch-name> --output json` (defaults to open)');
  expect(title).toContain('A successful empty array means new');
  expect(title).toContain('one match supplies the existing title/identity');
  expect(title).toContain('Lookup failure or ambiguous matches **STOP** for resolution, never mean no PR');
  expect(title).toContain("Save the result for Step 19's recheck");
  expect(title).toContain('Save the result as `NEW_TITLE` for Step 19');
  expect(title).toContain('start with `v$NEW_VERSION `; never publish an unprefixed title');
});

test('ship explains receipts and genuine review tokens before selecting current invocation records', () => {
  const state = compact(entryTemplate.slice(entryTemplate.indexOf('### Keep state'), entryTemplate.indexOf('{{SECTION_INDEX:ship}}')));
  expect(state).toContain('A **receipt** is saved evidence of a check\'s command, result and consumed content');
  expect(state).toContain('**start token** is the opaque value returned by `gstack-review-log --start` before it reads the diff');
  expect(state).toContain('`REVIEW_START` for Step 9, a separate `PASS_START` for each Step 11 attempt, and `DESIGN_START` for design');
  expect(state).toContain('Finish each pass with its original token');
  expect(state).toContain('`gstack-wtree` prints a Git tree hash covering tracked and non-ignored untracked files');
});

test('ship documentation-only plan refresh preserves the original audit and routes unsupported classifications back to its gates', () => {
  const route = compact(entryTemplate.slice(entryTemplate.indexOf('### 2. Choose the change route'), entryTemplate.indexOf('### 3. Resolve documentation freshness')));
  expect(route).toContain("Keep Step 8's original child report and counts");
  expect(route).toContain("Recheck affected plan items using their recorded verification");
  expect(route).toContain("append current evidence to the invocation record");
  expect(route).toContain("run Step 8's audit and decision gates only, then return to Step 16 stage 1. Never edit the child's counts yourself");
  expect(route).toContain("**No changes, or the docs-only checks still support the plan:** Continue to stage 3 without a new code review");
});

test('ship recovery map preserves ordered review, build and push transitions without new allowances', () => {
  const gate = compact(controlTemplate + entryTemplate.slice(entryTemplate.indexOf('## Step 16:'), entryTemplate.indexOf('## Step 17:')));
  const rows = ['**Non-fast-forward push:**', '**Authentication, hook or network failure:**'].map(outcome => pushFlow.indexOf(outcome));
  expect(rows.every(position => position >= 0)).toBe(true);
  expect(rows).toEqual([...rows].sort((a, b) => a - b));
  expect(reviewFlow).toContain("Every repeat starts before the checklist read and captures a fresh REVIEW_START. Finish the complete review and QA before applying any fix in Step 9.4");
  expect(reviewFlow).toContain('Insert Step 5, affected Steps 6–8 and all of Step 9 before the pending Step 10 in the work list');
  expect(commentsFlow).toContain("Run Step 9's full review/fix loop, then return here");
  expect(gate).toContain('Insert `5–11.5 → 12–14 → 16` before the pending Step 17');
  expect(gate).toContain('Insert `5–11.5 → 12–14 → 16` before the pending Step 17, then stop this step. This repair excludes Step 14.5');
  expect(gate).toContain('A missing prerequisite or failed build stops shipping: report **Build failed or prerequisite missing**, with the command, error and needed repair');
  expect(gate).toContain('Repair the prerequisite or build, then repeat stage 1');
  expect(gate).toContain("After it passes, continue to stage 2; treat any content repair as a behavioral change there");
  expect(gate).toContain('Never invent a substitute command');
  expect(gate).toContain('A `package.json` version-only edit can qualify; scripts, dependencies and runtime configuration require live tests');
  expect(gate).toContain('Uncertain edits cannot be exempted');
  const commits = compact(entryTemplate.slice(entryTemplate.indexOf('## Step 15:'), entryTemplate.indexOf('## Step 16:')));
  expect(commits).toContain('Only the final VERSION/CHANGELOG commit gets the release version and co-author trailer');
});

test('ship r22 routes each late change to one restart point before final checks', () => {
  const route = compact(entryTemplate.slice(entryTemplate.indexOf('### 2. Choose the change route'), entryTemplate.indexOf('### 3. Resolve documentation freshness')));
  const cases = ['**Behavior, tests or build inputs changed:**',
    '**Only authored docs or release metadata changed:**', '**No changes, or the docs-only checks still support the plan:**'];
  const positions = cases.map(label => route.indexOf(label));
  expect(route).toContain("Classify the comparison in this order");
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  const behavior = route.slice(positions[0], positions[1]);
  expect(behavior).toContain('Insert `5–11.5 → 12–14 → 16` before the pending Step 17');
  expect(behavior).toContain('Insert `5–11.5 → 12–14 → 16` before the pending Step 17, then stop this step. This repair excludes Step 14.5');
  expect(behavior).toContain('This repair excludes Step 14.5 because the rebuild can change generated docs. Step 16 restarts at stage 1');
  expect(behavior).toContain('rebuild and compare again before stage 3 decides documentation freshness');
  const docs = route.slice(positions[1], positions[2]);
  expect(docs).toContain("run Step 8's audit and decision gates only, then return to Step 16 stage 1");
  expect(docs).toContain('Never edit the child\'s counts');
  expect(route.slice(positions[2])).toContain("**No changes, or the docs-only checks still support the plan:** Continue to stage 3 without a new code review");
});

test('ship r22 re-audits return through commit and regeneration rather than bypassing freshness', () => {
  const docs = compact(entryTemplate.slice(entryTemplate.indexOf('### 3. Resolve documentation freshness'), entryTemplate.indexOf('### 4. Verify the frozen candidate')));
  expect(docs).toContain("Compare the base and hashes of the selected release paths, generated outputs and docs/templates with Step 14.5\'s saved values");
  expect(docs).toContain("accepted audit matches all inputs | Continue to stage 4");
  const retryStart = docs.indexOf('**An attempt remains, with changed inputs or an available repair:**');
  const retryEnd = docs.indexOf('**Otherwise:**');
  expect(retryStart).toBeGreaterThanOrEqual(0);
  expect(retryEnd).toBeGreaterThan(retryStart);
  const retry = docs.slice(retryStart, retryEnd);
  expect(retry).toContain('Validate the outcome before Step 15');
  expect(compact(docsTemplate)).toContain('Only an actual user exception counts');
  expect(compact(docsTemplate)).toContain('Reconcile those before proceeding');
  expect(retry).toContain("Validate the outcome before Step 15, then restart Step 16 stage 1 to regenerate and compare again");
  expect(retry).not.toContain('Continue to stage 4');
  expect(finalGate).toContain("STOP unless the user accepts the specific named documentation risk and all unwaivable gates clear");
  expect(docs).toContain("retain `Documentation: blocked`, its reason and incomplete scope");
  expect(docs).toContain("covers the same approved scope and exact content");
  expect(docs).toContain('Never run a third audit');
});

test('ship plan audit resolves scope drift before learnings and stops on an unverified N', () => {
  const section = readTemplate('ship/sections/plan-completion.md');
  expect(section.indexOf('## Step 8.1:')).toBeLessThan(section.indexOf('## Step 8.2:'));
  expect(section.indexOf('## Step 8.2:')).toBeLessThan(section.indexOf('## Prior Learnings'));
  expect(section).toContain('N) Not done — block ship and report the item as NOT DONE; do not offer a second deferral choice');
  expect(section).toContain('Any N: STOP');
  expect(section).not.toContain('re-enter the priority-1 gate');
});

test('outside challenge and documentation reruns preserve their actual blocking owners', () => {
  const adversarial = readTemplate('ship/sections/adversarial.md');
  expect(adversarial).toContain('An unavailable outside challenge does not block shipping by itself');
  expect(adversarial).toContain('structured P1 and non-convergence gates still apply');
  expect(adversarial).toContain("Returning here never resets Step 9's three-cycle fix limit");
  const standaloneReview = readTemplate('review/sections/adversarial.md');
  expect(standaloneReview).toContain('supported findings still enter Step 5 Fix-First');
  expect(standaloneReview).not.toContain('supported findings still enter Step 11');
  const docs = readTemplate('ship/sections/pr-body.md');
  expect(docs).toContain('**Existing open PR/MR:** update');
  expect(docs).toContain('do not run the create commands below');
  expect(docs).not.toContain('no PR exists yet');
  expect(entryTemplate.indexOf('## Step 14.5:')).toBeLessThan(entryTemplate.indexOf('## Step 17:'));
});
