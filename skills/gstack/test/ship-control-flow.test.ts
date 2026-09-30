import { describe, expect, test } from 'bun:test';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { ALL_HOST_CONFIGS } from '../hosts';
import { generateAdversarialStep, generatePlanCompletionGateShip, generateReviewDashboard } from '../scripts/resolvers/review';
import { HOST_PATHS } from '../scripts/resolvers/types';

const read = (file: string) => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');
const compact = (text: string) => text.replace(/\s+/g, ' ');
const entry = read('ship/SKILL.md.tmpl');
const start = entry.indexOf('### Ship control flow');
const end = entry.indexOf('{{SECTION_INDEX:ship}}');
const controller = entry.slice(start, end);
const review = compact(read('ship/sections/review-army.md.tmpl'));
const adversarial = compact(generateAdversarialStep({ host: 'claude', skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude }));
const gate = compact(entry.slice(entry.indexOf('## Step 16:'), entry.indexOf('## Step 17:')));

describe('ship source controller', () => {
  test('documentation freshness covers selected code paths as well as release metadata', () => {
    const docs = compact(read('ship/sections/documentation.md.tmpl'));
    expect(docs).toContain('hashes of the selected release paths, generated outputs and docs/templates');
    const freshness = gate.slice(gate.indexOf('### 3.'), gate.indexOf('### 4.'));
    expect(freshness).toContain('selected release paths, generated outputs and docs/templates');
    expect(freshness).toContain("A prior invocation's audit or risk decision never qualifies");
    expect(freshness).toContain('the same approved scope and exact content');
  });

  test('changed documentation inputs permit only the remaining bounded re-audit', () => {
    const docs = compact(read('ship/sections/documentation.md.tmpl'));
    const recovery = docs.slice(docs.indexOf('## Blocked recovery'));
    expect(recovery).toContain('If an attempt remains and either the audited inputs changed');
    expect(recovery).toContain('a concrete launch/input/permission correction or reviewed patch repair is available');
    expect(docs).toContain('an initial audit plus ONE repair/re-audit');
    expect(docs).toContain('never a third attempt, even after Step 16 changes');
    const freshness = gate.slice(gate.indexOf('### 3.'), gate.indexOf('### 4.'));
    expect(freshness).toContain('**An attempt remains, with changed inputs or an available repair:**');
    expect(freshness).toContain('**Otherwise:** STOP unless the user accepts');
    expect(freshness).not.toContain('**No attempt remains or no repair is available:**');
  });

  test('repairs use one ordered work list without a return stack', () => {
    const text = compact(controller);
    expect(compact(entry)).toContain('**Next steps:** one ordered work list, with the current step marked');
    expect(text).toContain('Expand a repair into individual steps and insert them before the still-pending work');
    expect(text).toContain('This replaces the current item, whose actual result stays in the record');
    expect(text).toContain('Add its destination only if not already the next pending step');
    expect(text).toContain('The saved list takes precedence over ordinary next-step sentences inside a repair');
    expect(text).toContain('5 → 6 → 7 → 8 → 9 → 10 → 11 → 11.5');
    expect(text).not.toContain('finish the inner repair, then resume the unfinished outer range');
    expect(text).toContain('Keep the same attempt counts throughout the invocation');
    expect(text).toContain('initial-plus-ONE limit never resets');
  });

  test('the roadmap and recovery groups define ownership and zero-edit eligibility', () => {
    const text = compact(entry);
    expect(text).toContain('integrate (1–3) → test and review (4–11.5) → prepare the release (12–15) → verify frozen content (16) → push and publish (17–21)');
    expect(text).toContain('children return evidence, not permission to proceed');
    expect(review).toContain('**No edits in this pass:** Resolve the required-probe gate below');
    expect(review).toContain('Only after it clears may you continue to Step 10');
    expect(review).toContain('With completed checklist and dispatched reviewers, failed/unavailable required probes block continuation');
    expect(review).toContain('This cannot waive missing reviewer output, recurring fixes or independent test/security gates');
    expect(review).toContain('Undispatched gated/unsupported specialists do not block independently');
    expect(text).toContain('Reuse it only for that same scope; a repair never resets approvals or expands them');
    expect(text).not.toContain('eligible zero-edit pass');
    expect(text).not.toContain('the same waiver');
    expect(text).not.toContain("the controller's detour");
  });

  test('distribution discovery is a shortlist, not a manifest-only artifact decision', () => {
    const distribution = compact(entry.slice(entry.indexOf('## Step 2:'), entry.indexOf('## Step 3:')));
    expect(distribution).toContain('List candidate distribution paths');
    expect(distribution).toContain("Also inspect matching untracked files from Step 1's status");
    expect(distribution).toContain('a new `package.json` or `Cargo.toml` alone does not establish a publishable artifact');
    expect(distribution).toContain('inspect existing manifests for newly declared binaries or package exports');
    expect(distribution).toContain('New artifact without a pipeline');
    expect(distribution).toContain('AskUserQuestion');
    expect(distribution).toContain('Do not publish a release during `/ship`');
  });

  test('queue qualification exhaustively distinguishes online, git fallback and unusable output', () => {
    const version = compact(entry.slice(entry.indexOf('## Step 12:'), entry.indexOf('{{SECTION:changelog}}')));
    const qualify = version.indexOf('**Qualify first:**');
    const usable = version.indexOf('**Usable candidate:**');
    const missing = version.indexOf('**No usable candidate:**');
    expect(qualify).toBeGreaterThan(0);
    expect(usable).toBeGreaterThan(qualify);
    expect(missing).toBeGreaterThan(usable);
    expect(version).toContain('require successful utility output and a nonempty valid version');
    expect(version).toContain('`offline:false` qualifies; `offline:true` qualifies only with `fallback:"git"`');
    expect(version).toContain('Offline output without that fallback, failure, malformed output or an empty version is unusable, even if it contains a version-looking string');
    expect(version).toContain('FRESH sets `NEW_VERSION=CANDIDATE_VERSION`');
    expect(version).toContain('Only approval changes the existing version');
    expect(version).toContain('a sibling holding `>= NEW_VERSION` requires a choice: advance past it, or stop this attempt and sync');
    expect(version.slice(missing)).toContain('FRESH uses local `BUMP_LEVEL` arithmetic; ALREADY_BUMPED keeps `currentVersion`');
  });

  test.each(['home', 'override', 'plugin'])('the actual nudge shell honors %s roots, repeat suppression and enabled tuning', mode => {
    const root = mkdtempSync(join(tmpdir(), 'gstack-ship-nudge-'));
    try {
      const home = join(root, 'home');
      const state = mode === 'home' ? join(home, '.gstack') : join(root, 'state root');
      mkdirSync(home, { recursive: true });
      const nudge = entry.slice(entry.indexOf('## Step 21:'), entry.indexOf('## Section self-check'));
      const block = nudge.match(/```bash\n([\s\S]*?)\n```/);
      expect(block).not.toBeNull();
      const script = block![1].replaceAll('~/.claude/skills/gstack', JSON.stringify(resolve(import.meta.dir, '..')));
      const run = () => spawnSync('bash', ['-c', script], {
        cwd: root,
        env: {
          PATH: process.env.PATH,
          HOME: home,
          TMPDIR: join(root, 'tmp'),
          ...(mode === 'override' ? { GSTACK_HOME: state } : {}),
          ...(mode === 'plugin' ? { CLAUDE_PLUGIN_ROOT: join(root, 'gstack'), CLAUDE_PLUGIN_DATA: state } : {}),
        },
        encoding: 'utf8',
        timeout: 10_000,
      });
      const marker = join(state, '.plan-tune-nudge-shown');
      const first = run();
      expect(first.status).toBe(0);
      expect(first.stdout).toContain('Run /plan-tune to opt in');
      expect(existsSync(marker)).toBe(true);
      if (mode !== 'home') expect(existsSync(join(home, '.gstack', '.plan-tune-nudge-shown'))).toBe(false);
      const repeated = run();
      expect(repeated.status).toBe(0);
      expect(repeated.stdout).toBe('');
      rmSync(marker);
      writeFileSync(join(state, 'config.yaml'), 'question_tuning: true\n');
      const enabled = run();
      expect(enabled.status).toBe(0);
      expect(enabled.stdout).toBe('');
      expect(existsSync(marker)).toBe(false);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  test('nested repairs resume the unfinished outer range before its destination', () => {
    const text = compact(controller);
    expect(text).toContain('Expand a repair into individual steps and insert them before the still-pending work');
    expect(text).toContain('For another repair, repeat rule 2 without discarding pending work');
    expect(text).toContain('Step 11 fixes insert `9 → 10 → 11` before 11.5');
    expect(text).toContain('A further Step 9 fix affecting 6–8 makes the list `5 → 6 → 7 → 8 → 9 → 10 → 11 → 11.5`');
    expect(text).toContain('The unchanged release steps follow');
    expect(text).not.toContain('Enter Step 9 before REVIEW_START capture and full review');
  });

  test('shared foreground dispatch stays mandatory at all three owned dispatch sites', () => {
    const coverage = read('ship/sections/test-coverage.md.tmpl');
    const shared = coverage.indexOf('### Shared subagent dispatch');
    expect(shared).toBeGreaterThan(0);
    expect(shared).toBeLessThan(coverage.indexOf('Dispatch the audit through Agent'));
    const contract = compact(coverage.slice(shared, coverage.indexOf('Dispatch the audit through Agent')));
    expect(contract).toContain('use the Agent tool with `run_in_background: false`');
    expect(contract).toContain('Omitting the flag runs the subagent in the background');
    expect(contract).toContain('keeping a fresh context');
    expect(contract).toContain('Do not invoke the target as a Skill or run it inline instead');
    expect(contract).toContain("only under that section's documented fallback, after a failed subagent has stopped");
    for (const section of ['test-coverage', 'plan-completion', 'greptile']) {
      const text = read(`ship/sections/${section}.md.tmpl`);
      expect(text).toContain('shared foreground-dispatch rule');
      expect(text).toContain('run_in_background: false');
      expect(text).not.toContain('{{FOREGROUND_DISPATCH_NOTE}}');
    }
    expect(entry).not.toContain('### Shared subagent dispatch');
    for (const section of ['plan-completion', 'greptile']) {
      expect(read(`ship/sections/${section}.md.tmpl`)).toContain("Step 7's shared foreground-dispatch rule");
    }
  });

  test('ship dashboard displays actual historical records without a repeated sample panel', () => {
    const ctx = { host: 'claude' as const, skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude };
    const ship = generateReviewDashboard(ctx);
    expect(ship).toContain('REVIEW READINESS DASHBOARD');
    expect(ship).toContain('| Review | Runs | Last run | Status | Required |');
    expect(ship).toContain('Use one row for each entry in step 1');
    expect(ship).toContain('Only Eng Review is marked required');
    expect(ship).toContain('{actual status and reason}');
    expect(ship).toContain('VERDICT: {CLEARED or NOT CLEARED} — {reason}');
    expect(ship).not.toContain('+====================================================================+');
    const review = generateReviewDashboard({ ...ctx, skillName: 'review' });
    expect(review).toContain('+====================================================================+');
  });

  test('coverage fallback settles the child and still applies the coverage gate', () => {
    const text = compact(read('ship/sections/test-coverage.md.tmpl'));
    const fallback = text.slice(text.indexOf('**Audit failure:**'), text.indexOf('{{TEST_COVERAGE_GATE_SHIP}}'));
    expect(fallback).toContain('confirm it stopped before running the same audit inline');
    expect(fallback).toContain('does not pass or bypass the coverage gate');
    expect(fallback).toContain('including its undetermined-percentage and test-only rules');
    expect(text).not.toContain('partial results are better than none');
  });

  test('no-plan gate skips only the audit and retains verification and the remaining section', () => {
    const text = generatePlanCompletionGateShip({ host: 'claude', skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude });
    const noPlan = compact(text.slice(text.indexOf('**No plan file found:**')));
    expect(noPlan).toContain('Skip only the plan completion audit');
    expect(noPlan).toContain('Continue with Step 8.1, Scope Drift and Prior Learnings');
    expect(noPlan).toContain('Step 9 QA still runs');
    expect(noPlan).not.toContain('Skip entirely');
  });

  test('binding separates record selection, three snapshots and probe outcomes', () => {
    const text = compact(entry.slice(entry.indexOf('## Step 11.5:'), entry.indexOf('## Step 12:')));
    const steps = ['1. **Select', '2. **Compare', '3. **Preserve', '4. **Save'];
    const positions = steps.map(step => text.indexOf(step));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a,b) => a-b));
    expect(text).toContain('All three snapshots must match');
    expect(text).toContain('does not mean the failed or unrun probes passed');
    expect(text).not.toContain('equal snapshots do not pass waived probes');
  });

  test('late classification and docs freshness are ordered decisions, not metadata assumptions', () => {
    const text = compact(entry.slice(entry.indexOf('## Step 16:'), entry.indexOf('## Step 17:')));
    expect(text).toContain('**Behavior, tests or build inputs changed:**');
    expect(text).toContain('**Only authored docs or release metadata changed:**');
    expect(text).toContain('**No changes, or the docs-only checks still support the plan:**');
    expect(text).toContain('accepted audit matches all inputs | Continue to stage 4');
    expect(text).toContain("Use Blocked recovery with the existing count");
    expect(text).toContain('Use this example only after confirming that every allowed edit is release metadata');
    expect(text).not.toContain('Every listed change below is metadata:');
  });

  test('one early controller owns detours without replacing local gates', () => {
    expect(start).toBeGreaterThan(entry.indexOf('# Ship:'));
    expect(start).toBeLessThan(end);
    expect(end).toBeLessThan(entry.indexOf('{{BASE_BRANCH_DETECT}}'));
    expect(entry.match(/### Ship control flow/g)).toHaveLength(1);
    const text = compact(controller);
    expect(text).toContain('You, the **parent** running /ship, own advancement');
    expect(text).toContain('Follow the saved work list');
    expect(text).toContain('STOP and AskUserQuestion gates still apply during repairs');
    expect(text).toContain('The saved list takes precedence over ordinary next-step sentences inside a repair. A range never adds unlisted steps');
    expect(text).toContain('Keep the same attempt counts throughout the invocation');
    expect(text).toContain('A range ending at Step 14 does not enter Step 14.5');
    expect(text).toContain('its initial-plus-ONE limit never resets');
    expect(text).not.toContain('| At step |');
    expect(entry.match(/Ship control flow/g)).toHaveLength(1);
    expect(review).toContain('Every repeat starts before the checklist read and captures a fresh REVIEW_START');
  });

  test('distribution and bootstrap detours end at their exact forward resume', () => {
    const merge = compact(entry.slice(entry.indexOf('## Step 3:'), entry.indexOf('{{SECTION:tests}}')));
    expect(merge).toContain('repeat Step 2 on the merged content, including its decisions, then continue to Step 4');
    expect(merge).toContain('Otherwise continue to Step 4 directly');
    const tests = compact(read('ship/sections/tests.md.tmpl'));
    expect(tests).toContain('A runs Step 4 with this new bootstrap choice, then returns here to run the tests');
    expect(tests).toContain('A) Add tests (recommended)');
    expect(tests).toContain('declining bootstrap alone is not that approval');
  });

  test('missing dispatched output outranks the cycle cap, fixes and zero-edit continuation', () => {
    const decisions = review.slice(review.indexOf('### Decide whether to repeat Step 9'));
    const names = ['1. **Dispatched reviewer output missing:**', '2. **Third fixing cycle reached (`CYCLES >= 3`):**', '3. **Fixes applied below the cap:**', '4. **No edits in this pass:**'];
    const positions = names.map(name => decisions.indexOf(name));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a, b) => a - b));
    const missing = decisions.slice(positions[0], positions[1]);
    expect(missing).toContain('STOP and name each failed specialist or Red Team');
    expect(missing).toContain('Retain queued fixes and restore coverage');
    expect(missing).toContain('If this pass made edits, resume at the next decision; otherwise run a fresh complete Step 9');
    expect(missing).toContain('A successful peer or a QA exception cannot replace missing dispatched coverage');
    const cap = decisions.slice(positions[1], positions[2]);
    expect(cap).toContain('STOP');
    expect(cap).toContain('`converged:false`; do not run a fourth fixing cycle');
    const fixes = decisions.slice(positions[2], positions[3]);
    expect(fixes).toContain('Insert Step 5, affected Steps 6–8 and all of Step 9 before the pending Step 10 in the work list');
    expect(fixes).toContain('Tests must pass or retain approval for the same verified pre-existing failures and scope');
    expect(fixes).toContain('Keep CYCLES and scoped approvals across this repeat');
    expect(decisions.slice(positions[3])).toContain('Only after it clears may you continue to Step 10');
    expect(review).toContain('Undispatched host-unsupported/gated specialists do not block');
    expect(review).toContain('Failed, blocked, inconclusive or not-run required probes mean false, never clean');
    expect(review).toContain('This cannot waive missing reviewer output, recurring fixes or independent test/security gates');
  });

  test('comment fixes resume triage and preserve settled approvals and replies', () => {
    const section = compact(read('ship/sections/greptile.md.tmpl'));
    expect(section).toContain('If fixes were approved, save their approvals and comment references');
    expect(section).toContain("Run Step 9's full review/fix loop, then return here");
    expect(review).toContain('Finish the complete review and QA before applying any fix in Step 9.4');
    expect(section).toContain('Finish the saved replies without asking again about completed fixes, and classify new comments');
    expect(section).toContain('With no queued fixes, continue to Step 11');
    expect(section).toContain('This optional triage does not block ship');
    expect(section).toContain('unknown or missing status is unavailable');
    expect(section).not.toContain('return to Step 9');
  });

  test('native recovery has one corrected attempt and cannot borrow outside completion', () => {
    const finish = adversarial.slice(adversarial.indexOf('### Finish the adversarial phase'));
    const recovery = finish.slice(finish.indexOf('1. **Required native'), finish.indexOf('2. **Fixes queued'));
    expect(recovery).toContain('STOP and confirm the native task stopped');
    expect(recovery).toContain('Outside-provider output cannot replace this pass');
    expect(recovery).toContain('One recovery retry is allowed only after a concrete prerequisite correction and restored access');
    expect(recovery).toContain('count it in the invocation record before launch');
    expect(recovery).toContain('Capture a fresh PASS_START and persist the new attempt separately');
    expect(recovery).toContain('Without that correction, or if the recovery fails, ask for repair and remain blocked');
    const queued = finish.slice(finish.indexOf('2. **Fixes queued'), finish.indexOf('3. **Native complete'));
    expect(queued).toContain('Keep the findings and their approvals');
    expect(queued).toContain('Insert Steps 9, 10 and 11 before the pending Step 11.5 in the work list. Step 9 completes full review before fixes');
    expect(queued).toContain('any further repair inserts its checks ahead of the remaining items');
    expect(queued).toContain('not recovery retries');
    expect(finish.slice(finish.indexOf('3. **Native complete'))).toContain('then continue to Step 11.5');
  });

  test.each(ALL_HOST_CONFIGS.map(({ name }) => name))('%s cannot skip Step 11.5 or change standalone review control flow', host => {
    const ctx = { host, skillName: 'ship', tmplPath: '', paths: HOST_PATHS[host] };
    const ship = generateAdversarialStep(ctx);
    const finish = compact(ship.slice(ship.indexOf('### Finish the adversarial phase')));
    expect(finish).toContain('Apply these decisions in order before leaving Step 11');
    expect(finish).toContain('Required native review incomplete');
    expect(finish).toContain('Outside-provider output cannot replace this pass');
    expect(finish).toContain('Fixes queued after native completion');
    expect(finish).toContain('Native complete with no queued fixes');
    expect(finish).toContain('Step 11.5');
    expect(finish).not.toContain('proceed to Step 12');
    expect(finish).not.toContain('return to Step 9');
    const review = generateAdversarialStep({ ...ctx, skillName: 'review' });
    expect(review).not.toContain('Ship control flow');
    expect(review).not.toContain('Step 11.5');
    expect(review).toContain('Return all findings and structured-review decisions to Step 5');
    expect(review).toContain('do not start an inner repair loop');
  });

  test('Step 11.5 verifies original record identity before any release write', () => {
    const bindingStart = entry.indexOf('## Step 11.5:');
    expect(bindingStart).toBeGreaterThan(entry.indexOf('{{SECTION:adversarial}}'));
    expect(bindingStart).toBeLessThan(entry.indexOf('## Step 12:'));
    const binding = compact(entry.slice(bindingStart, entry.indexOf('## Step 12:')));
    for (const field of ['saved handle, original token and source', 'skill:"review"', 'via:"ship"', 'skill:"adversarial-review"',
      'review_binding.state', 'verified', 'review_binding.start_wtree', 'review_binding.end_wtree']) expect(binding).toContain(field);
    expect(binding).toContain('Never attach new tokens to old work');
    expect(binding).toContain('Keep Step 9.4\'s incomplete flags');
    expect(binding).toContain('does not mean the failed or unrun probes passed');
    expect(binding).toContain('insert `9 → 10 → 11 → 11.5` before Step 12. Bind the new records at 11.5');
  });

  test('late build and behavior changes complete bounded ranges before docs', () => {
    const build = gate.slice(gate.indexOf('### 1.'), gate.indexOf('### 2.'));
    expect(build).toContain('A missing prerequisite or failed build stops shipping');
    expect(build).toContain('Repair the prerequisite or build, then repeat stage 1');
    expect(build).toContain('After it passes, continue to stage 2; treat any content repair as a behavioral change there');
    const behavior = gate.slice(gate.indexOf('1. **Behavior'), gate.indexOf('2. **Only authored'));
    expect(behavior).toContain('Insert `5–11.5 → 12–14 → 16` before the pending Step 17');
    expect(behavior).toContain('before the pending Step 17, then stop this step. This repair excludes Step 14.5');
    expect(behavior).toContain('rebuild and compare again before stage 3 decides documentation freshness');
    const plan = gate.slice(gate.indexOf('2. **Only authored'), gate.indexOf('3. **No changes'));
    expect(plan).toContain("run Step 8's audit and decision gates only, then return to Step 16 stage 1");
    expect(plan).toContain("Never edit the child's counts yourself");
  });

  test('docs freshness has bounded re-audit and exact-content exception routes', () => {
    const docs = gate.slice(gate.indexOf('### 3.'), gate.indexOf('### 4.'));
    expect(docs).toContain("Use Blocked recovery with the existing count");
    expect(docs).toContain('**An attempt remains, with changed inputs or an available repair:** insert `14.5 → 15 → 16` before Step 17');
    expect(docs).toContain('restart Step 16 stage 1 to regenerate and compare again');
    expect(docs).toContain('STOP unless the user accepts the specific named documentation risk and all unwaivable gates clear');
    expect(docs).toContain('Never run a third audit');
    expect(docs).toContain('Unchanged approved content goes to stage 4; repaired content goes to stage 1');
    expect(docs).toContain('retain `Documentation: blocked`');
    expect(docs).toContain('Missing, stale or blocked | Use recovery below. Never silently refresh hashes');
  });

  test('tests, absent test approval and new writes reopen final verification', () => {
    const tests = gate.slice(gate.indexOf('### 4.'), gate.indexOf('### 5.'));
    expect(tests).toContain("**New, changed or unwaived test failure:** STOP publication. Run Steps 5–15, starting with Step 5's triage, then return to Step 16 stage 1");
    expect(tests).toContain('This recovery also applies if a failure appears while reporting in stage 5');
    expect(tests).toContain('run Steps 5–15, including the no-tests decision, then return to Step 16 stage 1');
    expect(tests).toContain('it does not authorize a third attempt');
    expect(gate).toContain('If content changes during or after verification, restart at stage 1 and complete all five stages before Step 17');
    expect(gate).toContain('Content-preserving commits keep valid evidence');
  });

  test('push failure kinds and publication races have distinct resumes', () => {
    const push = compact(entry.slice(entry.indexOf('## Step 17:'), entry.indexOf('## Step 18:')));
    expect(push).toContain('**If the push fails, STOP.** No Step 19 or publication claim');
    expect(push).toContain("fetch and inspect the remote, then merge under Step 3's conflict rules");
    expect(push).toContain('Run Steps 5–16 before returning to Step 17. Never rewrite history');
    expect(push).toContain('repeat Step 16 even if content is unchanged before returning to Step 17');
    expect(push).toContain('Never bypass failed guards');
    const publication = compact(read('ship/sections/pr-body.md.tmpl'));
    expect(publication).toContain("If the open PR/MR or title changed, repeat Step 18's identity/title preparation");
    expect(publication).toContain('then return here for a new lookup, fresh body and both redaction scans before publishing');
  });
});
