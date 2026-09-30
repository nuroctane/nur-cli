import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import type { EvalTestEntry } from './eval-store';
import { buildCookieWorkflowJudgeInput } from './cookie-workflow-judge-input';
import { COOKIE_MANUAL_REVIEW_FILE } from './cookie-workflow-manual-review';

// Later documentation added to the judged BROWSER.md section after the approval
// was recorded (v1.91.4.0 Windows Opera wave). Reversing it reconstructs the
// exact historical prompt bytes, including the section's line range.
const LATER_BROWSER_BLOCK = /\*\*Windows: Opera and Opera GX\.\*\*[\s\S]*?appear in CLI output\.\n\n/;
const LATER_BROWSER_EDITS: Array<[string, string]> = [
  ['The picker recognizes Chrome, Chromium, Brave, Edge, Windows-only Opera and Opera GX, and macOS-only Comet, Arc, and Dia.', 'The picker recognizes Chrome, Chromium, Brave, Edge, and macOS-only Comet, Arc, and Dia.'],
  [' Opera and Opera GX are Windows-only and read from `%APPDATA%\\Opera Software\\Opera Stable` or `Opera GX Stable`, in `Default` or `Profile N` directories; legacy root-level layouts, Opera side profiles and portable or relocated installs are not detected. Opera has no native extraction, so its App-Bound cookies (if any) need manual sign-in.', ''],
];

export function approvedCookieWorkflowSource(source: string): string {
  let removedLines = 0;
  let historical = source
    .replace('sha256sum < "$tmpfile" | awk \'{print $(1)}\'', 'sha256sum "$tmpfile" | awk \'{print $1}\'')
    .replace('shasum -a 256 < "$tmpfile" | awk \'{print $(1)}\'', 'shasum -a 256 "$tmpfile" | awk \'{print $1}\'')
    .replace(LATER_BROWSER_BLOCK, block => { removedLines = block.split('\n').length - 1; return ''; });
  for (const [later, earlier] of LATER_BROWSER_EDITS) historical = historical.replace(later, earlier);
  return historical.replace(/(--- BEGIN FILE "BROWSER\.md" \(lines \d+-)(\d+)(; section\) ---)/, (_, head, end, tail) => `${head}${Number(end) - removedLines}${tail}`);
}

export function manualReviewFixture(root = resolve(import.meta.dir, '../..')): EvalTestEntry {
  const approval = JSON.parse(readFileSync(resolve(root, COOKIE_MANUAL_REVIEW_FILE), 'utf8'));
  const prompt = approvedCookieWorkflowSource(buildCookieWorkflowJudgeInput(root).prompt);
  if (createHash('sha256').update(prompt).digest('hex') !== approval.prompt_sha256
    || Buffer.byteLength(prompt) !== approval.prompt_bytes) {
    throw new Error('Historical cookie approval fixture no longer reconstructs the exact approved prompt');
  }
  return {
    name: 'setup-browser-cookies/SKILL.md workflow', suite: 'Cookie setup workflow quality', tier: 'llm-judge',
    passed: false, execution: 'executed', exit_reason: 'provider_refusal', attempt: 1, duration_ms: 1, cost_usd: 0,
    model: approval.model, prompt,
    error: 'Synthetic provider refusal fixture, not live model evidence',
    manual_review: { approval, refusal: { stop_reason: 'refusal', response_id: 'msg_synthetic_fixture',
      request_id: 'req_synthetic_fixture', model: approval.model, input_tokens: 1, output_tokens: 0, text_blocks: 0 } },
  };
}
