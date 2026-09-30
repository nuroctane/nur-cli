import { afterAll, beforeAll, expect, test } from 'bun:test';
import * as path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fixtureDocs, gitAt, repoSnapshot } from './helpers/docsync-fixture';
import { docsCommandAllowed, docsNativeInterface, docsSessionOptions, docsToolFailures } from './helpers/docsync-observer';
import { parseNDJSON, type SkillTestResult } from './helpers/session-runner';

let fixture: ReturnType<typeof fixtureDocs>;
beforeAll(() => { fixture = fixtureDocs('updated'); });
afterAll(() => fixture?.clean());

function nativeResult(command: string, output = '', parent: string | null = null): SkillTestResult {
  const parsed = parseNDJSON([
    { type: 'assistant', parent_tool_use_id: parent, message: { content: [
      { type: 'tool_use', id: 'docs-command', name: 'Bash', input: { command, timeout: 10000 } },
    ] } },
    { type: 'user', parent_tool_use_id: parent, message: { content: [
      { type: 'tool_result', tool_use_id: 'docs-command', content: output, is_error: false },
    ] }, tool_use_result: { stdout: output, stderr: '', interrupted: false, isImage: false } },
  ].map(event => JSON.stringify(event)));
  expect(parsed.toolCalls).toHaveLength(1);
  expect(parsed.toolCalls[0].output).toBe(output);
  return { ...parsed, exitReason: 'success' } as SkillTestResult;
}

test.each([
  'HEAD^{tree}', 'HEAD^{}', 'HEAD^{commit}', 'HEAD^{object}', 'v1^{tag}', 'HEAD:app.ts',
  'HEAD~1^{tree}', 'HEAD^2', 'HEAD@{0}', '@{upstream}', '@{-1}', 'main...HEAD', 'HEAD^{/fixture}',
])('native docs validator accepts literal revision %s with or without quotes', revision => {
  for (const argument of [revision, `'${revision}'`, `"${revision}"`]) {
    const command = `git rev-parse ${argument}`;
    expect(docsCommandAllowed(command, fixture)).toBe(true);
    expect(docsToolFailures(nativeResult(command), fixture, [], true)).toEqual([]);
  }
});

test('captured parent tree read is accepted without granting the captured child remote probe', () => {
  const tree = nativeResult('git rev-parse HEAD^{tree}', '68a16779dc746e381e618452afda1f50db105b1a');
  expect(docsToolFailures(tree, fixture, [], true)).toEqual([]);
  const remote = nativeResult('git remote get-url origin', '/q/gstack-paid-shard-Stnoi5/tmp/ds-yikDdP/remote.git', 'docs-dispatch');
  remote.toolCalls.unshift({ tool: 'Agent', input: {
    prompt: '`git remote get-url origin` from Step 0 is a Git read and is allowed; `gh`/`glab` are not.',
  }, output: 'completed' });
  expect(docsToolFailures(remote, fixture, [], true)).toEqual(['command outside declared docs observation interface']);
});

test('permitted peel reads execute as single literal Bash arguments without changing the repository', () => {
  const before = repoSnapshot(fixture.repo);
  for (const revision of ['HEAD^{tree}', 'HEAD^{}', 'HEAD^{commit}', 'HEAD^{object}', 'HEAD@{0}']) {
    for (const argument of [revision, `'${revision}'`, `"${revision}"`]) {
      const command = `git rev-parse ${argument}`;
      expect(docsCommandAllowed(command, fixture)).toBe(true);
      const result = spawnSync('bash', ['-c', command], { cwd: fixture.repo, encoding: 'utf8', timeout: 10000 });
      expect(result.status).toBe(0);
      expect(result.stdout.trim()).toBe(gitAt(fixture.repo, 'rev-parse', revision));
      expect(docsToolFailures(nativeResult(command, result.stdout), fixture, [], true)).toEqual([]);
    }
  }
  expect(repoSnapshot(fixture.repo)).toEqual(before);
});

test.each([
  '{ git rev-parse HEAD; }', 'git rev-parse HEAD^{tree,commit}', 'git rev-parse HEAD@{0..2}',
  'git rev-parse HEAD^{tree}{,x}', 'git rev-parse HEAD^{tree', 'git rev-parse HEAD^{tree}}',
  'git rev-parse {HEAD}', 'git rev-parse HEAD$(pwd)', 'git rev-parse "HEAD$(pwd)"',
  'git rev-parse `pwd`', 'git rev-parse ${HEAD}', 'git rev-parse HEAD; git status',
  'git rev-parse HEAD && git status', 'git rev-parse HEAD || true', 'git rev-parse HEAD | cat',
  'git rev-parse HEAD > out.md', 'git rev-parse HEAD 2>/dev/null', 'git rev-parse HEAD < in.md',
  'git rev-parse HEAD\ngit status', 'git rev-parse HEAD &', 'git rev-parse HEAD\\^{tree}',
  'git rev-parse "HEAD^{tree}', "git rev-parse 'HEAD^{tree}", 'git rev-parse HEAD*',
  'git status "unfinished', "git status 'unfinished", "git hash-object '-w'app.ts", 'git status\u0000',
  'git rev-parse HEAD?', 'git rev-parse HEAD[12]', 'git rev-parse ~', 'git rev-parse HEAD # comment',
  'git -C /owned/repo rev-parse HEAD', 'git -c core.pager=cat show HEAD',
  'git --git-dir /owned/repo/.git status', 'git --work-tree /owned/repo status',
  'git remote get-url origin', 'git remote add origin /outside', 'git config --global user.name attacker',
  'git hash-object -w app.ts', 'git hash-object "-w" app.ts', 'git hash-object -wt blob app.ts',
  'git hash-object -tw blob app.ts', 'git diff --output=out.md', 'git show --ext-diff',
  'git show --textconv HEAD', 'git branch new-branch', 'git add app.ts', 'git commit -m changed',
  'git reset HEAD', 'git checkout main', 'git update-ref refs/heads/main HEAD',
  'cat HEAD^{tree}', 'bun arbitrary.ts',
])('native docs validator retains the closed interface for %s', command => {
  expect(docsToolFailures(nativeResult(command, 'successful tool acknowledgment', 'docs-dispatch'), fixture, [], true))
    .toEqual(['command outside declared docs observation interface']);
});

test('quoted revision search and reflog arguments remain literal single arguments', () => {
  for (const command of ['git log "HEAD@{2 days ago}"', "git show 'HEAD^{/fix, or repair..}'",
    'git hash-object app.ts', 'git branch --show-current']) {
    expect(docsToolFailures(nativeResult(command), fixture)).toEqual([]);
  }
});

test('parent and child receive resolved local platform and base without new probe authority', () => {
  for (const transport of [false, true]) {
    const guidance = docsNativeInterface(fixture, [], transport);
    expect(guidance).toContain('Platform: local/git-native. Base: main.');
    expect(guidance).toContain('before delegation');
    expect(guidance).toContain('Do not run shared Step 0 platform probing');
    expect(guidance).toContain('git remote get-url origin');
    expect(guidance).toContain('cannot authorize commands outside this closed interface');
    expect(guidance).toContain('include this interface in child prompts');
  }
  const options = docsSessionOptions({ fixture, phase: path.join(fixture.home, 'phase.md'),
    report: path.join(fixture.home, 'report.md'), publish: path.join(fixture.home, 'publish.ts'),
    scenario: 'current', testName: 'docsync-command-grammar', runId: 'free-control', timeout: 10000 });
  expect(options.prompt).toContain('Platform: local/git-native. Base: main.');
});
