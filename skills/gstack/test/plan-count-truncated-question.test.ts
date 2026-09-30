import { describe, expect, test } from 'bun:test';
import fs from 'node:fs';
import path from 'node:path';
import { capturePlanCountQuestion, matchesNativePlanQuestion } from './helpers/claude-pty-runner';
import type { NativePlanQuestionCall } from './helpers/plan-count-transcript';
import completed from './fixtures/ceo-approach-z-call.json';

const screen = fs.readFileSync(path.join(import.meta.dir, 'fixtures/ceo-approach-z-screen.txt'), 'utf8');
function pending(): NativePlanQuestionCall {
  // No pending-only version survived the live capture. This is an explicit
  // projection for replay, not evidence of metadata availability at input time.
  const { answers, answeredAt, unansweredQuestionIndices, ...call } = structuredClone(completed);
  return { ...call, answered: false, failed: false };
}

describe('native question truncated before its routing id', () => {
  test('changed or incomplete panels cannot borrow pending metadata', () => {
    for (const visible of [
      screen.replace('☐ Approach', '☐ Other'),
      screen.replace('SQL injection bug', 'different defect'),
      screen.replace('Dela…', 'Different…'),
      screen.replace('Dela…', 'Dela'),
      screen.replace('B — Proper Integration (recommended)', 'B — Different Integration (recommended)'),
      screen.replace('3. C — Reject and Redesign', '3. Changed choice'),
      screen.replace('4. Type something.', '4. Extra operation'),
      screen.replace('↑/↓ to navigate', '↑/↓ to navigte'),
      screen.replace(' · Esc to cancel', ''),
      screen + '\nA later question?',
      'Quoted earlier menu:\n' + screen,
      screen.slice(screen.indexOf('❯ 1.')),
      screen.replace(/\n\n│[\s\S]*?\n\n❯/, '\n\n│ Which approach…\n\n❯'),
      screen.replace('☐ Approach', '← ☐ Approach ☐ Other ✔ Submit →'),
    ]) {
      expect(visible).not.toBe(screen);
      const call = pending();
      expect(matchesNativePlanQuestion(visible, call), visible).toBe(false);
      expect(capturePlanCountQuestion(visible, new Set(), 0, true, call)?.nativeCall).toBeUndefined();
    }
    const packet = pending(); packet.questions.push(structuredClone(packet.questions[0]!));
    expect(matchesNativePlanQuestion(screen, packet)).toBe(false);
  });
});
