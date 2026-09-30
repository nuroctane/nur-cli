import { expect, test } from 'bun:test';
import * as realFs from 'node:fs';
import * as path from 'node:path';
import { JUDGE_MS, CAPTURE_MS } from './helpers/eval-budgets';
import { SESSION_DRAIN_GRACE_MS, type SkillTestResult } from './helpers/session-runner';
import { sliceBetween, REVIEW_ARMY_E2E_SECTIONS } from './helpers/skill-fixture';

const REAL_ROOT = path.join(import.meta.dir, '..');
const source = realFs.readFileSync(path.join(import.meta.dir, 'skill-e2e-review-army.test.ts'), 'utf8');
const reviewArmyDoc = realFs.readFileSync(path.join(REAL_ROOT, 'review', 'sections', 'review-army.md'), 'utf8');

const CAPTURED_MERGED_1 = `{
  "findings": [
    {
      "severity": "CRITICAL",
      "confidence": 9,
      "path": "user_controller.rb",
      "line": 3,
      "category": "injection",
      "summary": "Interpolating params[:name] into SQL allows injection.",
      "fix": "Use a parameterized query.",
      "fingerprint": "user_controller.rb:3:injection",
      "specialists": ["security"],
      "source": "specialist",
      "advisory": false,
      "gate": "show",
      "verified": true,
      "validation_note": "Arrived as CRITICAL with advisory:true; advisory flag removed and CRITICAL severity retained per Stage 2."
    },
    {
      "severity": "INFORMATIONAL",
      "advisory": true,
      "confidence": 8,
      "path": "number_parser.rb",
      "line": 1,
      "category": "stdlib-wrapper",
      "summary": "The one-method NumberParser wrapper only forwards to Integer.",
      "fix": "Optionally call Integer directly in lookup and remove the wrapper.",
      "fingerprint": "number_parser.rb:1:stdlib-wrapper",
      "specialists": ["simplification"],
      "source": "specialist",
      "lines_removable": 5,
      "gate": "show",
      "verified": true
    }
  ],
  "critical_count": 1,
  "informational_count": 0,
  "issues_found": 1,
  "quality_score": 8,
  "lines_removable_total": 5,
  "specialists": {
    "security": {"dispatched": true, "findings": 1, "critical": 1, "informational": 0},
    "simplification": {"dispatched": true, "findings": 1, "critical": 0, "informational": 0}
  }
}
`;
const CAPTURED_MERGED_2 = `{
  "findings": [
    {
      "severity": "CRITICAL",
      "advisory": false,
      "confidence": 9,
      "path": "user_controller.rb",
      "line": 3,
      "category": "injection",
      "summary": "Interpolating params[:name] into SQL allows injection.",
      "fix": "Use a parameterized query.",
      "fingerprint": "user_controller.rb:3:injection",
      "specialists": ["security"],
      "source": "specialist",
      "partition": "defect",
      "gate": "show",
      "multi_specialist_confirmed": false,
      "verified": true,
      "verification_note": "user_controller.rb:3 interpolates params[:name] directly into a raw SQL string passed to User.where.",
      "validation_note": "Arrived as CRITICAL with advisory:true; advisory flag removed and CRITICAL severity retained per Stage 2. Treated as a defect."
    },
    {
      "severity": "INFORMATIONAL",
      "advisory": true,
      "confidence": 8,
      "path": "number_parser.rb",
      "line": 1,
      "category": "stdlib-wrapper",
      "summary": "The one-method NumberParser wrapper only forwards to Integer.",
      "fix": "Optionally call Integer directly in lookup and remove the wrapper.",
      "fingerprint": "number_parser.rb:1:stdlib-wrapper",
      "specialists": ["simplification"],
      "source": "specialist",
      "partition": "advisory",
      "lines_removable": 5,
      "gate": "show",
      "multi_specialist_confirmed": false,
      "verified": true,
      "verification_note": "NumberParser.parse returns Integer(value) unchanged; sole caller is UserController#lookup at user_controller.rb:7. Inlining is behavior-preserving."
    }
  ],
  "critical_count": 1,
  "informational_count": 0,
  "issues_found": 1,
  "quality_score": 8,
  "lines_removable_total": 5,
  "specialists": {
    "security": {"dispatched": true, "findings": 1, "critical": 1, "informational": 0},
    "simplification": {"dispatched": true, "findings": 1, "critical": 0, "informational": 0}
  }
}
`;
const CAPTURED_REPORT_1 = `# Merged Findings Report — feature/add-controller

Scope of this capture: Step 4.6 Collect and merge only (parse → validate severity →
identify/merge → confidence gates → score). No additional reviewers were dispatched,
no unrelated findings were discovered, and Fix-First was not entered.

## Source verification

Both specialist findings were checked against the source on the branch.

| Finding | Source evidence | Verdict |
|---|---|---|
| security / injection @ \`user_controller.rb:3\` | \`User.where("name = '#{params[:name]}'")\` — \`params[:name]\` is string-interpolated directly into the SQL fragment with no binding or sanitization. | CONFIRMED |
| simplification / stdlib-wrapper @ \`number_parser.rb:1\` | \`NumberParser\` is a 5-line class whose only method, \`self.parse(value)\`, returns \`Integer(value)\`. Sole caller is \`UserController#lookup\` (\`user_controller.rb:7\`). | CONFIRMED |

## Merge stage log

1. **Parse outputs** — 2 JSON lines read from \`specialist-findings.jsonl\`; both valid.
   Sources: \`security\` (1 finding), \`simplification\` (1 finding).
2. **Validate severity** — Line 1 arrived as \`severity:"CRITICAL"\` with \`advisory:true\`,
   which is contradictory. Per the rules, \`advisory\` was **removed** and \`CRITICAL\`
   severity **retained**; the finding is treated as a defect. Severity was not downgraded.
   Line 2 is a valid \`INFORMATIONAL\` advisory and remains advisory.
3. **Identify and merge** — Partitioned into 1 defect + 1 advisory before fingerprinting.
   Neither finding is \`shared-libs\`, so supplied fingerprints were used as-is:
   \`user_controller.rb:3:injection\`, \`number_parser.rb:1:stdlib-wrapper\`. No identity
   collisions within either partition → no merges, no multi-specialist confirmation boost.
4. **Confidence gates** — 9/10 and 8/10 → both at 7+, shown normally with no caveat.
5. **Score** — Only merged NON-advisory specialist findings enter the header and score.

## Findings

\`\`\`
SPECIALIST REVIEW: 1 findings (1 critical, 0 informational) from 2 specialists

[CRITICAL] (confidence: 9/10, specialist: security) user_controller.rb:3 — Interpolating params[:name] into SQL allows injection.
  Fix: Use a parameterized query.

[ADVISORY] (confidence: 8/10, specialist: simplification) number_parser.rb:1 — The one-method NumberParser wrapper only forwards to Integer.
  Fix: Optionally call Integer directly in lookup and remove the wrapper.
  (INFORMATIONAL advisory — excluded from counts and score; ASK-only, never auto-applied)

PR Quality Score: 8/10
net: -5 lines possible
\`\`\`

Score derivation: \`max(0, 10 - (1 * 2 + 0 * 0.5)) = 8\`.

## Specialist activity (for review-log Step 5.8)

Counts reflect findings each specialist actually returned, before deduplication.
Advisory findings count toward \`findings\` but not toward the defect counts.

| Specialist | Dispatched | findings | critical | informational |
|---|---|---|---|---|
| security | yes | 1 | 1 | 0 |
| simplification | yes | 1 | 0 | 0 |

Dispatch/skip status for the remaining specialists (testing, maintainability,
performance, data-migration, api-contract, design, red-team) is not present in
this capture and is not asserted here.

## Hand-off status

Not executed in this capture. When Fix-First runs, the CRITICAL injection defect
follows normal AUTO-FIX/ASK rules; the simplification advisory is ASK-only.
`;
const CAPTURED_REPORT_2 = `# Merged Findings Report — feature/add-controller

Scope of this capture: Step 4.6 Collect and merge only (parse → validate severity →
identify/merge → confidence gates → score → specialist activity). No additional
reviewers were dispatched, no unrelated findings were discovered, Fix-First was not
entered, and no application source was edited.

Input: \`specialist-findings.jsonl\` (2 lines). Branch diff vs \`main\`: \`user_controller.rb\`
(+9) and \`number_parser.rb\` (+5), both new files.

## Source verification

Both specialist findings were checked against the source on the branch.

| Finding | Source evidence | Verdict |
|---|---|---|
| security / injection @ \`user_controller.rb:3\` | \`User.where("name = '#{params[:name]}'")\` — \`params[:name]\` is string-interpolated directly into a raw SQL fragment with no bind parameter, hash condition, or sanitization. Genuine SQL injection. | CONFIRMED |
| simplification / stdlib-wrapper @ \`number_parser.rb:1\` | \`NumberParser\` is a 5-line class whose only method, \`self.parse(value)\`, returns \`Integer(value)\` unchanged. Sole caller is \`UserController#lookup\` (\`user_controller.rb:7\`). Inlining \`Integer(params[:id])\` is behavior-preserving (same ArgumentError/TypeError on bad input). \`lines_removable: 5\` matches the file length. | CONFIRMED |

## Merge stage log

1. **Parse outputs** — 2 JSON lines read; both valid, none skipped. Sources tagged:
   \`security\` (1 finding), \`simplification\` (1 finding). No missing/unusable output
   among the sources present in this capture.
2. **Validate severity** — Line 1 arrived as \`severity:"CRITICAL"\` with \`advisory:true\`,
   which is contradictory. Per the rules, \`advisory\` was **removed** and \`CRITICAL\`
   severity **retained**; the finding is treated as a defect from this point on.
   Severity was not downgraded to match the advisory flag. Line 2 is a valid
   \`INFORMATIONAL\` advisory and remains advisory.
3. **Identify and merge** — Partitioned into 1 defect + 1 advisory **before** any
   fingerprint grouping. Neither finding is \`shared-libs\` (category or fingerprint
   prefix), so \`sharedLibsFingerprint\` was not invoked and the supplied fingerprints
   were used as-is: \`user_controller.rb:3:injection\`, \`number_parser.rb:1:stdlib-wrapper\`.
   No identity collisions within either partition → no merges, no
   MULTI-SPECIALIST CONFIRMED boost. \`advisory\` and \`lines_removable\` preserved.
4. **Confidence gates** — security 9/10 and simplification 8/10 → both ≥ 7, shown
   normally with no caveat; nothing sent to appendix or suppressed.
5. **Score** — Only merged NON-advisory specialist findings enter the header and score:
   critical_count = 1, informational_count = 0.

## Findings

\`\`\`
SPECIALIST REVIEW: 1 findings (1 critical, 0 informational) from 2 specialists

[CRITICAL] (confidence: 9/10, specialist: security) user_controller.rb:3 — Interpolating params[:name] into SQL allows injection.
  Fix: Use a parameterized query.

[ADVISORY] (confidence: 8/10, specialist: simplification) number_parser.rb:1 — The one-method NumberParser wrapper only forwards to Integer.
  Fix: Optionally call Integer directly in lookup and remove the wrapper.
  (INFORMATIONAL advisory — excluded from counts, score and unresolved-defect totals; ASK-only, never auto-applied)

PR Quality Score: 8/10
net: -5 lines possible
\`\`\`

Score derivation: \`max(0, 10 - (1 * 2 + 0 * 0.5)) = 8\`.
Simplification footer: specialist dispatched and returned findings → \`lines_removable\` sum = 5.

## Specialist activity (for review-log Step 5.8)

Counts reflect findings each specialist actually returned, before deduplication.
Advisory findings count toward \`findings\` but not toward the defect counts.

| Specialist | Dispatched | findings | critical | informational |
|---|---|---|---|---|
| security | yes | 1 | 1 | 0 |
| simplification | yes | 1 | 0 | 0 |

Dispatch/skip status for the remaining specialists (testing, maintainability,
performance, data-migration, api-contract, design, red-team) is not present in
this capture and is not asserted here.

## Hand-off status

Not executed in this capture. When Fix-First runs, the CRITICAL injection defect
follows normal AUTO-FIX/ASK rules; the simplification advisory is ASK-only.
`;

const NATIVE_MERGED = JSON.parse(CAPTURED_MERGED_1) as any;
const MINIMAL_RESULT = {
  exitReason: 'success', toolCalls: [{ tool: 'Read', input: {}, output: '' }], browseErrors: [],
  output: 'merged review written', duration: 0, transcript: [], model: 'free-callback-replay',
  firstResponseMs: 0, maxInterTurnMs: 0,
  costEstimate: { inputChars: 0, outputChars: 0, estimatedTokens: 0, estimatedCost: 0, turnsUsed: 1 },
} satisfies SkillTestResult;

type Plan = {
  result?: Record<string, unknown>;
  merged?: unknown | null;
  mergedRaw?: string;
  report?: string | null;
  editSource?: boolean;
};

const nativeFindings = () => (NATIVE_MERGED.findings as any[]).map((f: any) => ({ ...f, specialists: [...f.specialists] }));
function mergedWith(mutate: (findings: any[]) => void, top: Record<string, unknown> = {}) {
  const findings = nativeFindings();
  mutate(findings);
  return { ...NATIVE_MERGED, ...top, findings };
}

async function exercise(plans: Plan[]) {
  const files = new Map<string, string>();
  let mkdtempCounter = 0;
  const artifactNames = () =>
    [...files.keys()]
      .filter(k => /(review-output\.md|merged-review\.json)$/.test(k))
      .map(k => k.split('/').pop()!)
      .sort();

  const fsMock = {
    mkdtempSync: (p: string) => p + (mkdtempCounter++),
    writeFileSync: (p: string, s: unknown) => { files.set(String(p), typeof s === 'string' ? s : String(s)); },
    readFileSync: (p: string) => {
      if (String(p).endsWith('review-army.md')) return reviewArmyDoc;
      if (!files.has(String(p))) { const error: any = new Error('ENOENT: no such file, ' + p); error.code = 'ENOENT'; throw error; }
      return files.get(String(p))!;
    },
    existsSync: (p: string) => files.has(String(p)),
    rmSync: (p: string) => { files.delete(String(p)); },
    mkdirSync: () => {},
    readdirSync: () => [] as string[],
    copyFileSync: () => {},
  };

  const setups: Array<() => unknown> = [];
  const finals: Array<() => unknown> = [];
  const callbacks: Array<() => Promise<unknown>> = [];
  const registrations: Array<{ title: string; names: string[] }> = [];
  const firedTitles: string[] = [];
  const rows: any[] = [];
  const calls: any[] = [];
  const preRun: string[][] = [];
  let registeredName = '';
  let outerTimeout = 0;
  let index = 0;

  const runSkillTest = async (opts: any) => {
    calls.push(opts);
    const plan = plans[index++] ?? {};
    preRun.push(artifactNames());
    const dir = opts.workingDirectory as string;
    if (plan.report !== null) files.set(path.join(dir, 'review-output.md'), plan.report === undefined ? CAPTURED_REPORT_1 : plan.report);
    if (plan.mergedRaw !== undefined) files.set(path.join(dir, 'merged-review.json'), plan.mergedRaw);
    else if (plan.merged !== null) files.set(path.join(dir, 'merged-review.json'), JSON.stringify(plan.merged === undefined ? NATIVE_MERGED : plan.merged));
    if (plan.editSource) files.set(path.join(dir, 'user_controller.rb'), '# edited by the model\n');
    return { ...MINIMAL_RESULT, ...(plan.result ?? {}) };
  };

  const args: Record<string, unknown> = {
    expect, JUDGE_MS, CAPTURE_MS, SESSION_DRAIN_GRACE_MS, sliceBetween, REVIEW_ARMY_E2E_SECTIONS,
    ROOT: REAL_ROOT, runId: 'synthetic-army-run',
    describe: (_t: string, fn: () => void) => { fn(); },
    test: () => {}, beforeAll: (fn: any) => setups.push(fn), afterAll: (fn: any) => finals.push(fn),
    describeIfSelected: (title: string, names: string[], fn: any) => { registrations.push({ title, names }); if (names.includes('review-army-quality-score')) { firedTitles.push(title); fn(); } },
    testConcurrentIfSelected: (name: string, fn: any, timeout: number) => { registeredName = name; callbacks.push(fn); outerTimeout = timeout; },
    logCost: () => {}, recordE2E: (_c: any, name: string, title: string, result: any, extra: any) => rows.push({ name, title, result, ...extra }),
    createEvalCollector: () => ({}), finalizeEvalCollector: () => {},
    extractSkillSections: () => 'review skeleton', resolveEvalModel: () => undefined,
    runRecordedOfficeHoursAttempt: async () => ({}), OFFICE_HOURS_BUN_GRACE_MS: 0,
    runSkillTest, spawnSync: () => ({ status: 0, stdout: '', stderr: '' }),
    fs: fsMock, path, os: { tmpdir: () => '/tmp' },
  };

  let body = source;
  for (const match of source.matchAll(/^import[\s\S]*?;\n/gm)) body = body.replace(match[0], '');
  new Function(...Object.keys(args), new Bun.Transpiler({ loader: 'ts' }).transformSync(body))(...Object.values(args));

  expect(callbacks).toHaveLength(1);
  for (const setup of setups) await setup();
  const errors: Array<unknown> = [];
  for (const _ of plans) { try { await callbacks[0](); errors.push(undefined); } catch (error) { errors.push(error); } }
  for (const finalizer of finals) await finalizer();
  return { rows, calls, errors, preRun, registrations, firedTitles, registeredName, outerTimeout };
}

test('both captured native attempts complete the callback with their real merged JSON and report payloads', async () => {
  const x = await exercise([
    { mergedRaw: CAPTURED_MERGED_1, report: CAPTURED_REPORT_1 },
    { mergedRaw: CAPTURED_MERGED_2, report: CAPTURED_REPORT_2 },
  ]);
  expect(x.errors).toEqual([undefined, undefined]);
  expect(x.rows.map(r => r.passed)).toEqual([true, true]);
  expect(x.preRun).toEqual([[], []]);
});

test('the merge prompt pins the specialists-array contract and read-only merge scope', async () => {
  const x = await exercise([{ mergedRaw: CAPTURED_MERGED_1, report: CAPTURED_REPORT_1 }]);
  const opts = x.calls[0];
  expect(opts.timeout).toBe(JUDGE_MS);
  expect(opts.maxTurns).toBe(15);
  expect(opts.model).toBeUndefined();
  expect(opts.testName).toBe('review-army-quality-score');
  expect(opts.prompt).toContain('give each finding record a specialists array naming every specialist source that reported it (an array even when a single specialist did)');
  expect(opts.prompt).toContain('do not dispatch additional reviewers');
  expect(opts.prompt).toContain('Do not edit application source');
  expect(x.outerTimeout).toBe(CAPTURE_MS);
});

test('merged provenance must be the exact specialists array, not scalar or loose', async () => {
  const x = await exercise([
    { merged: mergedWith(f => { delete (f[0] as any).specialists; }) },
    { merged: mergedWith(f => { (f[0] as any).specialists = 'security'; }) },
    { merged: mergedWith(f => { f[0].specialists = ['testing']; }) },
    { merged: mergedWith(f => { f[0].specialists = ['security', 'testing']; }) },
    { merged: mergedWith(f => { f[1].specialists = ['security']; }) },
  ]);
  expect(x.errors.every(Boolean)).toBe(true);
  expect(x.rows.map(r => r.passed)).toEqual([false, false, false, false, false]);
});

test('counts, defect total, and quality score must match the merged findings', async () => {
  const x = await exercise([
    { merged: mergedWith(() => {}, { critical_count: 2 }) },
    { merged: mergedWith(() => {}, { informational_count: 1 }) },
    { merged: mergedWith(() => {}, { issues_found: 2 }) },
    { merged: mergedWith(() => {}, { quality_score: 5 }) },
    { merged: mergedWith(f => { f.push({ ...f[0], fingerprint: 'x:1:injection' }); }) },
  ]);
  expect(x.errors.every(Boolean)).toBe(true);
  expect(x.rows.map(r => r.passed)).toEqual([false, false, false, false, false]);
});

test('critical defects and informational advisories cannot swap their severity or advisory flag', async () => {
  const x = await exercise([
    { merged: mergedWith(f => { f[0].advisory = true; }) },
    { merged: mergedWith(f => { f[0].severity = 'INFORMATIONAL'; }) },
    { merged: mergedWith(f => { f[1].advisory = false; }) },
    { merged: mergedWith(f => { f[1].severity = 'CRITICAL'; }) },
  ]);
  expect(x.errors.every(Boolean)).toBe(true);
  expect(x.rows.map(r => r.passed)).toEqual([false, false, false, false]);
});

test('the report header count, quality score, and advisory marker are enforced', async () => {
  const x = await exercise([
    { report: CAPTURED_REPORT_1.replace('1 findings (1 critical, 0 informational)', '1 findings (0 critical, 0 informational)') },
    { report: CAPTURED_REPORT_1.replace('PR Quality Score: 8/10', 'PR Quality Score: 5/10') },
    { report: CAPTURED_REPORT_1.split('[ADVISORY]').join('[NOTE]') },
  ]);
  expect(x.errors.every(Boolean)).toBe(true);
  expect(x.rows.map(r => r.passed)).toEqual([false, false, false]);
});

test('missing artifacts, edited source, and non-success runs all fail', async () => {
  const x = await exercise([
    { merged: null },
    { report: null },
    { editSource: true },
    { result: { exitReason: 'timeout' } },
  ]);
  expect(x.errors.every(Boolean)).toBe(true);
  expect(x.rows.map(r => r.passed)).toEqual([false, false, false, false]);
});

test('a stale prior report cannot satisfy a retry', async () => {
  const x = await exercise([
    { mergedRaw: CAPTURED_MERGED_1, report: CAPTURED_REPORT_1 },
    { merged: null, report: null },
  ]);
  expect(x.errors[0]).toBeUndefined();
  expect(x.errors[1]).toBeDefined();
  expect(x.rows.map(r => r.passed)).toEqual([true, false]);
  expect(x.preRun[1]).toEqual([]);
});

test('the quality-score id is the sole selection owner', async () => {
  const x = await exercise([{ mergedRaw: CAPTURED_MERGED_1, report: CAPTURED_REPORT_1 }]);
  const owners = x.registrations.filter(r => r.names.includes('review-army-quality-score'));
  expect(owners).toEqual([{ title: 'Review Army: Quality Score', names: ['review-army-quality-score'] }]);
  expect(x.firedTitles).toEqual(['Review Army: Quality Score']);
  expect(x.registeredName).toBe('review-army-quality-score');
});
