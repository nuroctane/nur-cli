import { describe, expect, test } from 'bun:test';
import type { NativePlanQuestionCall } from './helpers/plan-count-transcript';
import { isEngBatchingIssueAUQ } from './helpers/eng-seeded-coverage';
import { nativePlanCallFingerprint } from './helpers/claude-pty-runner';

function question(call: NativePlanQuestionCall, text: string) {
  const answer = call.answers![call.questions[0]!.question]!;
  call.questions[0]!.question = text; call.answers = { [text]: answer };
}

describe('Eng seeded coverage from completed native decisions', () => {
});


describe('batching caller counts completed issue decisions across setup boundaries', () => {
  const issue = (number: number, header = 'Architecture'): NativePlanQuestionCall => {
    const question = `D${number + 2} — Issue ${number}: Choose the component contract\nELI10: The current contract leaves behavior unspecified.`;
    return { sessionId: 'owned-session', toolUseId: `toolu_issue_${number}`, answered: true, failed: false,
      answeredAt: '2026-01-01T00:00:00.000Z', unansweredQuestionIndices: [],
      questions: [{ header, question, multiSelect: false, options: [
        { label: `${number}A: Define the contract`, description: 'Specify the behavior.' },
        { label: `${number}B: Retain the current contract`, description: 'Keep the documented risk.' },
      ] }], answers: { [question]: `${number}A: Define the contract` } };
  };
  const check = (call: NativePlanQuestionCall, prior: readonly NativePlanQuestionCall[] = []) =>
    isEngBatchingIssueAUQ(nativePlanCallFingerprint(call, 0, true), prior);

  test('six completed calls count six; unrelated wording and either offered choice do not change identity', () => {
    const history: NativePlanQuestionCall[] = [];
    for (const [i, header] of ['Architecture', 'Architecture', 'Architecture', 'Code quality', 'Tests', 'Performance'].entries()) {
      const call = issue(i + 1, header);
      if (i % 2) call.answers![call.questions[0]!.question] = call.questions[0]!.options[1]!.label;
      expect(check(call, history)).toBe(true); history.push(call);
    }
    expect(history).toHaveLength(6);
    expect(check(history[0]!, history)).toBe(false);
    const repeated = structuredClone(history[0]!); repeated.toolUseId = 'toolu_repeated';
    expect(check(repeated, history)).toBe(false);
    const scope = issue(1, 'Scope');
    expect(check(repeated, [scope])).toBe(true);
    const batch = issue(1), second = issue(2);
    batch.questions.push(...second.questions); Object.assign(batch.answers!, second.answers);
    expect(check(repeated, [batch])).toBe(true);
  });

  test('one batched native call cannot satisfy a three-call floor', () => {
    const batch = issue(1);
    for (const number of [2, 3, 4]) {
      const next = issue(number); batch.questions.push(...next.questions); Object.assign(batch.answers!, next.answers);
    }
    const count = [batch].filter(call => check(call)).length;
    expect(count).toBeLessThanOrEqual(1); expect(count).toBeLessThan(3);
  });

  test('incomplete, foreign, inconsistent or non-issue packets cannot inflate the counter', () => {
    const variants: Array<(call: NativePlanQuestionCall) => void> = [
      c => { c.answered = false; }, c => { c.failed = true; }, c => { c.unansweredQuestionIndices = [0]; },
      c => { delete c.answeredAt; }, c => { c.answers = {}; }, c => { c.questions[0]!.multiSelect = true; },
      c => { c.answers![c.questions[0]!.question] = 'Not offered'; },
      c => { c.questions[0]!.options[1]!.label = '2B: Foreign issue'; },
      c => { c.questions[0]!.options[1]!.label = '1A: Duplicate option ID'; },
      c => { c.questions[0]!.header = 'Scope'; }, c => { c.questions[0]!.header = 'Next steps'; },
      c => { c.questions[0]!.header = 'TODO'; }, c => { c.questions[0]!.header = 'Design'; },
      c => { question(c, c.questions[0]!.question.replace('Issue 1:', 'TODO 1:')); },
      c => { question(c, '> ' + c.questions[0]!.question); },
      c => { question(c, 'Historical note:\n' + c.questions[0]!.question); },
      c => { question(c, c.questions[0]!.question + '\nThis decision is "withdrawn".'); },
      c => { question(c, c.questions[0]!.question + '\nIssue 1 is no longer current.'); },
      c => { question(c, c.questions[0]!.question + '\nThis decision is `no longer current`.'); },
    ];
    for (const change of variants) { const call = issue(1); change(call); expect(check(call)).toBe(false); }
    const fp = nativePlanCallFingerprint(issue(1), 0, true); fp.signature = 'foreign:identity';
    expect(isEngBatchingIssueAUQ(fp)).toBe(false);
    expect(check(issue(2), [{ ...issue(1), sessionId: 'foreign-session' }])).toBe(false);
    const call = issue(1); question(call, call.questions[0]!.question + '\n> This decision is withdrawn.');
    expect(check(call)).toBe(true);
    const quoted = issue(1); question(quoted, quoted.questions[0]!.question + '\n`This decision is withdrawn.`');
    expect(check(quoted)).toBe(true);
  });
});
