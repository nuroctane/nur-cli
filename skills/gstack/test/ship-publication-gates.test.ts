import { expect, test } from 'bun:test';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const template = readFileSync(new URL('../ship/sections/pr-body.md.tmpl', import.meta.url), 'utf8');
const scanner = resolve(import.meta.dir, '../bin/gstack-redact');

for (const failure of ['none', 'body', 'title'] as const) {
  test(`publication scan consumes the real scanner result: ${failure}`, () => {
    const root = mkdtempSync(join(tmpdir(), 'ship-scan-'));
    try {
      const bin = join(root, '.claude/skills/gstack/bin');
      mkdirSync(bin, { recursive: true });
      writeFileSync(join(bin, 'gstack-config'), '#!/bin/sh\nprintf "private\\n"\n', { mode: 0o755 });
      writeFileSync(join(bin, 'gstack-redact'), `#!/bin/sh
kind=title
case "$*" in *--from-file*) kind=body ;; esac
printf '%s\\n' "$kind" >> "$HOME/scans"
if [ "$kind" = "$FAIL_AT" ]; then
  exec ${JSON.stringify(process.execPath)} ${JSON.stringify(scanner)} --from-file "$HOME/missing-input" --json
fi
exec ${JSON.stringify(process.execPath)} ${JSON.stringify(scanner)} "$@"
`, { mode: 0o755 });
      const block = template.match(/```bash\n(: "\$\{NEW_TITLE:[\s\S]*?)\n```/)?.[1];
      expect(block).toBeDefined();
      const result = Bun.spawnSync(['bash', '-c', block!], {
        cwd: root,
        env: { ...process.env, HOME: root, NEW_TITLE: 'v1.2.3.4 fix: publication checks', FAIL_AT: failure },
        stdout: 'pipe', stderr: 'pipe', timeout: 10_000,
      });
      const output = result.stdout.toString();
      const error = result.stderr.toString();
      const calls = readFileSync(join(root, 'scans'), 'utf8').trim().split('\n');
      if (failure === 'none') {
        expect(result.exitCode).toBe(0);
        expect(calls).toEqual(['body', 'title']);
        expect(output).toContain('"HIGH": 0');
      } else {
        expect(result.exitCode).toBe(1);
        expect(error).toContain('missing-input');
        expect(calls).toEqual(failure === 'body' ? ['body'] : ['body', 'title']);
        if (failure === 'body') expect(output).toContain('BLOCKED');
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test('publication reports unavailable triage separately from an empty successful fetch', () => {
  const section = template.slice(template.indexOf('## Greptile Review'), template.indexOf('## Scope Drift'));
  expect(section).toContain('Greptile triage: UNAVAILABLE (dispatch failed)');
  expect(section).toContain('actual reason');
  expect(section).toContain('complete');
  expect(section).toContain('no_pr');
});

test('publication refreshes the open review and title without treating lookup failure as absence', () => {
  const lookup = template.slice(0, template.indexOf('### Resolve Linked Spec'));
  expect(lookup).toContain("Recheck Step 18's PR/MR lookup and record it");
  expect(lookup.replace(/\s+/g, ' ')).toContain('Errors or ambiguous matches STOP publication');
  expect(lookup.replace(/\s+/g, ' ')).toContain("If the open PR/MR or title changed, repeat Step 18's identity/title preparation");
  expect(lookup.replace(/\s+/g, ' ')).toContain('then return here for a new lookup, fresh body and both redaction scans before publishing');
  expect(lookup).not.toContain('Ship control flow');
  expect(lookup).not.toContain('|| echo "NO_PR"');
  expect(lookup).not.toContain('|| echo "NO_MR"');
  expect(template).toContain('Exit 1 or any other error blocks');
  expect(template).toContain('public-strict policy');
});
