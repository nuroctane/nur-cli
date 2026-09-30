import { describe, test, expect } from 'bun:test';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'node:os';
import { spawnSync } from 'node:child_process';
import { generatePlanCompletionAuditReview, generatePlanCompletionAuditShip, generatePlanCompletionGateShip, generatePlanVerificationExec } from '../scripts/resolvers/review';
import { HOST_PATHS } from '../scripts/resolvers/types';

const SHIP_DIR = path.join(__dirname, '..', 'ship');

describe('authored ship-only plan verification handoff', () => {
  const ctx = { skillName: 'ship', tmplPath: 'ship/SKILL.md.tmpl', host: 'claude' as const, paths: HOST_PATHS.claude };
  test('local execution-only checks remain required in Step 8.1/9 outside implementation counts', () => {
    const audit = generatePlanCompletionAuditShip(ctx);
    const extraction = audit.slice(audit.indexOf('### Actionable Item Extraction'), audit.indexOf('### Verification Mode'));
    expect(extraction).toContain('Separate deliverables from execution-only verification');
    expect(extraction).toContain('implementation and test-creation requirements');
    expect(extraction).toContain('retain its command, expected outcome and source verbatim');
    expect(extraction).toContain('Step 8.1/9');
    expect(extraction).toContain('outside implementation counts');
    expect(extraction).toContain('never DONE from static inspection');
    expect(extraction).toContain('not EXTERNAL-STATE merely because it has not run');
    expect(extraction).toContain('Keep genuine external-state and human-only checks in this audit');
    expect(extraction).toContain('zero implementation counts do not waive those checks');
    expect(fs.readFileSync(path.join(SHIP_DIR, 'sections/plan-completion.md.tmpl'), 'utf8')).toContain('exactly these seven fields');
    const gate = generatePlanCompletionGateShip(ctx);
    expect(gate).toContain('Any NOT DONE items');
    expect(gate).toContain('Per-item confirmation is mandatory');
  });
  test('review-mode extraction retains its existing categories and has no ship-only routing', () => {
    const audit = generatePlanCompletionAuditReview({ ...ctx, skillName: 'review' });
    expect(audit).toContain('**Test requirements:** "Test that X", "Add test for Y", "Verify Z"');
    expect(audit).not.toContain('Separate deliverables from execution-only verification');
    expect(audit).not.toContain('Step 8.1/9');
    expect(audit).toContain('EXTERNAL-STATE');
  });
  test('preparation hands every explicit check to the report-only execution owner before Fix-First', () => {
    const text = generatePlanVerificationExec(ctx).replace(/\s+/g, ' ');
    expect(text).toContain('Collect now; execute in Step 9');
    expect(text).toContain('Do not invoke an entire QA skill or start probes here');
    for (const heading of ['Verification', 'Test plan', 'Testing', 'How to test', 'Manual testing']) {
      expect(text).toContain(`\`${heading}\``);
    }
    expect(text).toContain('any other explicit checks, including execution-only items retained by Step 8');
    expect(text).toContain('each exact expected outcome, source, surface, probe and safe prerequisites');
    expect(text).toContain('Clarify unknown outcomes');
    expect(text).toContain('Browser items use the declared project/plan dev URL and browser setup at execution');
    expect(text).toContain('functional items use native tools without discovering a web server');
    expect(text).toContain('An API URL is not automatically a page');
    expect(text).toContain('Only browser evidence needs screenshots');
    expect(text).toContain('If no verification section or no plan file exists, record no plan-specific items');
    expect(text).toContain('Automatic diff-scoped QA still runs. Continue to Step 8.2 Scope Drift below');
    expect(text).toContain('Handoff to Step 9.2.1');
    expect(text).toContain('parent-owned report-only explorer must execute this complete list before Fix-First');
    expect(text).toContain('prerequisite, permission, evidence and changed-input revalidation rules');
    expect(text).toContain('Share current-input proof for overlapping smoke probes');
    expect(text).toContain('plan checks beyond that smoke budget remain required');
    expect(text).toContain('At command/time limits, mark remaining checks not run');
    expect(text).toContain("Send failed, blocked or unrun checks through Step 9's required-probe gate, never silently waive them");
    expect(text).toContain('Noninteractive runs return blocked');
    expect(text).toContain('VERIFY_RESULT=pass only if all selected items pass, skipped only if none exist, otherwise fail');
    expect(text).toContain('Risk acceptance keeps the actual failed, blocked and unrun outcomes');
    expect(text).toContain("Report per-status counts, evidence and accepted risks in Step 19's `## Verification Results`, separately from automatic QA");
    expect(text.indexOf('Collect now')).toBeLessThan(text.indexOf('Handoff to Step 9.2.1'));
    expect(text.indexOf('Handoff to Step 9.2.1')).toBeLessThan(text.indexOf('After execution'));
  });
  test('the authored parent validates all seven fields and settles failed children before fallback', () => {
    const audit = fs.readFileSync(path.join(SHIP_DIR, 'sections/plan-completion.md.tmpl'), 'utf8');
    const parent = audit.slice(audit.indexOf('**Parent processing:**')).replace(/\s+/g, ' ');
    expect(parent).toContain("Check the task's terminal status");
    expect(parent).toContain('Without successful completion and valid LAST-line JSON, use the audit-failure fallback');
    expect(parent).toContain('exactly the seven declared fields');
    expect(parent).toContain('nonnegative integer counts whose classification sum equals `total_items`');
    expect(parent).toContain('a string `summary`');
    expect(parent).toContain('Missing, extra or invalid fields fail');
    expect(parent).toContain('Valid no-plan/no-actionable reports retain zero counts and their summary');
    expect(parent).toContain('no final output after ~10 minutes');
    expect(parent).toContain('stop any live child and confirm it stopped before an inline audit');
    expect(parent).toContain('same extraction/classification logic; never race a late result');
    expect(parent).toContain('If that also fails, AskUserQuestion');
    expect(parent).toContain('recording the reason in the PR body and Step 20 metrics');
    expect(parent).toContain('Stop and fix the audit (recommended/default)');
    const contract = audit.split('\n').find(line => line.startsWith('{"total_items":N,'))!;
    expect(Object.keys(JSON.parse(contract.replace(/:N([,}])/g, ':0$1'))).sort())
      .toEqual(['total_items', 'done', 'changed', 'partial', 'not_done', 'unverifiable', 'summary'].sort());
    expect(generatePlanCompletionGateShip(ctx)).toContain('Only PARTIAL items (no NOT DONE, no UNVERIFIABLE)');
    expect(generatePlanCompletionGateShip(ctx)).toContain('Continue with a note in the PR body. Not blocking');
  });
});

// Carved (v2 plan T9): the Plan Completion gate moved into sections/plan-completion.md.
// Read the skeleton + sections union so these invariants follow the content.
function readShipUnion(): string {
  let t = fs.readFileSync(path.join(SHIP_DIR, 'SKILL.md'), 'utf8');
  const secDir = path.join(SHIP_DIR, 'sections');
  if (fs.existsSync(secDir)) {
    for (const f of fs.readdirSync(secDir).sort()) {
      if (f.endsWith('.md')) t += '\n' + fs.readFileSync(path.join(secDir, f), 'utf8');
    }
  }
  return t;
}

describe('ship/SKILL.md — Plan Completion gate invariants (VAS-449 remediation)', () => {
  const skill = readShipUnion();

  test('Path concreteness rule: filesystem-pathed items must be test -f checked', () => {
    expect(skill).toContain('**Path concreteness rule.**');
    expect(skill).toMatch(/concrete filesystem path/);
    expect(skill).toMatch(/MUST be classified DONE or NOT DONE based on `\[ -f/);
  });

  test('Validator detection: project package.json validate-* scripts are auto-run', () => {
    expect(skill).toContain('**Validator detection.**');
    expect(skill).toMatch(/package\.json/);
    expect(skill).toMatch(/validate-\*/);
  });

  test('Per-item UNVERIFIABLE confirmation: blanket-confirm is forbidden', () => {
    expect(skill).toContain('**Per-item confirmation is mandatory.**');
    expect(skill).toMatch(/Do NOT use a single AskUserQuestion to blanket-confirm/);
    expect(skill).toMatch(/VAS-449/);
  });

  test('Subagent failure: fail-closed, not silent fail-open', () => {
    expect(skill).not.toMatch(/Never block \/ship on subagent failure\.\s*$/m);
    expect(skill.replace(/\s+/g, ' ')).toContain('Silent fail-open is the failure shape that VAS-449 surfaced');
    expect(skill).toMatch(/Stop and fix the audit/);
  });

  test('parent rejects audit errors and malformed counts instead of treating them as no plan', () => {
    const audit = fs.readFileSync(path.join(SHIP_DIR, 'sections/plan-completion.md'), 'utf8');
    const parent = audit.slice(audit.indexOf('**Parent processing:**'), audit.indexOf('**Audit-failure fallback:**')).replace(/\s+/g, ' ');
    expect(parent).toContain("Check the task's terminal status");
    expect(parent).toContain('Without successful completion and valid LAST-line JSON, use the audit-failure fallback');
    expect(parent).toContain('Require exactly the seven declared fields');
    expect(parent).toContain('Missing, extra or invalid fields fail');
    expect(parent).toContain('nonnegative integer');
    expect(parent).toContain('classification sum equals `total_items`');
    expect(parent).toContain('a string `summary`');
    expect(parent).toContain('audit-failure fallback');
    expect(parent).toContain('Valid no-plan/no-actionable reports retain zero counts');
  });

  test('successful plan audits use the declared seven-field contract without an error field', () => {
    const audit = fs.readFileSync(path.join(SHIP_DIR, 'sections/plan-completion.md'), 'utf8');
    const line = audit.split('\n').find(line => line.startsWith('{"total_items":N,'));
    expect(line).toBeDefined();
    const contract = JSON.parse(line!.replace(/:N([,}])/g, ':0$1'));
    expect(Object.keys(contract).sort()).toEqual(['total_items', 'done', 'changed', 'partial', 'not_done', 'unverifiable', 'summary'].sort());
    expect(contract).not.toHaveProperty('error');
    expect(audit).toContain('exactly these seven fields on the LAST LINE');
    expect(audit).not.toContain('A non-null `error`');
  });

  test('CONTENT-SHAPE dispatch invokes validator before falling back to UNVERIFIABLE', () => {
    expect(skill).toMatch(/CONTENT-SHAPE in another repo.*validator/s);
    expect(skill).toMatch(/passing validator promotes the item from UNVERIFIABLE to DONE/);
  });

  test('approved deferrals reach Step 14 without duplicating earlier P0 entries', () => {
    const entry = fs.readFileSync(path.join(SHIP_DIR, 'SKILL.md'), 'utf8');
    const todos = entry.slice(entry.indexOf('## Step 14:'), entry.indexOf('## Step 15:'));
    expect(todos).toContain('Add approved deferrals');
    expect(todos).toMatch(/Step 2[^\n]+P1/);
    expect(todos).toMatch(/Step 8[^\n]+P1[^\n]+plan/);
    expect(todos).toMatch(/Step 5[^\n]+P0[^\n]+deduplicate/);
    expect(todos.indexOf('Add approved deferrals')).toBeLessThan(todos.indexOf('Detect completed TODOs'));
    expect(todos.replace(/\s+/g, ' ')).toContain("If creation was declined or a write failed, warn and retain unsaved follow-ups in Step 19's PR summary");
  });

  test('CHANGELOG uses the normal workflow without checkpoint context or squash prerequisites', () => {
    const changelog = fs.readFileSync(path.join(SHIP_DIR, 'sections/changelog.md'), 'utf8');
    expect(changelog).toContain('**Write the CHANGELOG entry**');
    expect(changelog).not.toMatch(/WIP:|gstack-context|checkpoint|squash|Step 15\.0/);
  });

  test('live evidence recovery distinguishes bookkeeping failure from stale inputs', () => {
    const entry = fs.readFileSync(path.join(SHIP_DIR, 'SKILL.md'), 'utf8');
    const gate = entry.slice(entry.indexOf('## Step 16:'), entry.indexOf('## Step 17:'));
    const text = gate.replace(/\s+/g, ' ');
    expect(text).toContain('STALE/MISSING: changed content, command or age, or no proven run');
    expect(text).toContain('Only receipt storage/readback failed');
    expect(text).toContain('Independently prove unchanged final content, the same command and valid age');
    expect(text).toContain('from the successful run\'s evidence');
    expect(text).toContain('exact command, exit, timestamp and log');
    expect(text).toContain('as **ledger unavailable**');
    expect(text).toContain('**ledger unavailable**, never FRESH');
    const storage = text.slice(text.indexOf('| Only receipt storage/readback failed |')).split('|')[2];
    expect(storage).not.toContain('gstack-evidence run');
    expect(storage).toContain('from the successful run\'s evidence');
    expect(text).toContain("**New, changed or unwaived test failure:** STOP publication. Run Steps 5–15, starting with Step 5's triage, then return to Step 16 stage 1");
    expect(text).toContain('Reuse waivers only for the same verified pre-existing failures and approved scope');
    expect(text).toContain('This recovery also applies if a failure appears while reporting in stage 5');
    expect(text).toContain('Docs, TODO edits, new/generated tests and fixes make evidence STALE');
    expect(storage).toContain('Independently prove unchanged final content, the same command and valid age');
    expect(storage).toContain('Without that proof, use STALE/MISSING');
    expect(text).toContain('Without that proof, use STALE/MISSING');
  });

  test('ship contract precedes base detection and fresh remote facts precede distribution decisions', () => {
    const entry = fs.readFileSync(path.join(SHIP_DIR, 'SKILL.md'), 'utf8');
    expect(entry.indexOf('# Ship:')).toBeLessThan(entry.indexOf('## Step 0:'));
    const preflight = entry.slice(entry.indexOf('## Step 1:'), entry.indexOf('## Step 2:'));
    expect(preflight).toContain('git fetch origin <base>');
    expect(preflight).toMatch(/fetch fails[^\n]+STOP/);
    expect(entry).not.toContain('auto-generate and commit, or flag');
    expect(entry).toContain('Step 15 commits those tests');
  });

  test('bisectable commits proceed directly to verification without rewriting existing history', () => {
    const entry = fs.readFileSync(path.join(SHIP_DIR, 'SKILL.md'), 'utf8');
    const commit = entry.slice(entry.indexOf('## Step 15:'), entry.indexOf('## Step 16:'));
    expect(commit).toContain('Make bisectable commits');
    expect(commit).toContain('if already committed, continue to Step 16');
    expect(commit).toMatch(/Never create an empty commit/i);
    expect(commit).toContain('Each commit must work independently');
    expect(commit).not.toMatch(/checkpoint|WIP|squash|git rebase|git reset/);
    expect(entry).not.toMatch(/Step 15\.[012]/);
  });

  test('a rejected push stops publication and routes changed content back through verification', () => {
    const entry = fs.readFileSync(path.join(SHIP_DIR, 'SKILL.md'), 'utf8');
    const push = entry.slice(entry.indexOf('## Step 17:'), entry.indexOf('## Step 20:'));
    const recovery = push.replace(/\s+/g, ' ');
    expect(push).toMatch(/push fails[^\n]+STOP/);
    expect(recovery).toContain('**Non-fast-forward push:**');
    expect(recovery).toContain('Run Steps 5–16 before returning to Step 17. Never rewrite history');
    expect(recovery).toContain('**Authentication, hook or network failure:**');
    expect(recovery).toContain('repeat Step 16 even if content is unchanged before returning to Step 17');
    expect(push).toMatch(/never force.push/i);
    expect(push).toContain('Only a successful push');
  });
});

test('push idempotency requires the live remote SHA and fails closed on transport errors', () => {
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'ship-push-state-'));
  const env = { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1',
    GIT_AUTHOR_NAME: 'Test', GIT_AUTHOR_EMAIL: 'test@example.invalid',
    GIT_COMMITTER_NAME: 'Test', GIT_COMMITTER_EMAIL: 'test@example.invalid' };
  const git = (...args: string[]) => {
    const r = spawnSync('git', args, { cwd, env, encoding: 'utf8', timeout: 5000 });
    if (r.status !== 0) throw new Error(r.stderr || String(r.error));
  };
  try {
    git('init', '-b', 'feature');
    fs.writeFileSync(path.join(cwd, 'app'), 'base\n');
    git('add', 'app'); git('commit', '-m', 'base');
    const remote = path.join(cwd, '.git/remote.git');
    git('init', '--bare', remote); git('remote', 'add', 'origin', remote);
    const source = fs.readFileSync(path.join(SHIP_DIR, 'SKILL.md.tmpl'), 'utf8');
    const block = source.slice(source.indexOf('**Idempotency check:** Check if the branch'))
      .match(/```bash\n([\s\S]*?)\n```/)![1].replaceAll('<branch-name>', 'feature');
    const inspect = () => spawnSync('bash', ['-c', block], { cwd, env, encoding: 'utf8', timeout: 5000 });
    expect(inspect().stdout).toContain('PUSH_NEEDED');
    git('push', '-u', 'origin', 'feature');
    expect(inspect().stdout).toContain('ALREADY_PUSHED');
    fs.appendFileSync(path.join(cwd, 'app'), 'new\n'); git('commit', '-am', 'new');
    expect(inspect().stdout).toContain('PUSH_NEEDED');
    git('push', 'origin', 'feature');
    fs.renameSync(remote, remote + '-offline');
    const unavailable = inspect();
    expect(unavailable.status).toBe(1);
    expect(unavailable.stdout).not.toContain('ALREADY_PUSHED');
    expect(unavailable.stdout).toContain('BLOCKED');
  } finally { fs.rmSync(cwd, { recursive: true, force: true }); }
});
