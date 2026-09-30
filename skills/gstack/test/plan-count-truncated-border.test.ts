import { expect, test } from 'bun:test';
import captured from './fixtures/eng-d2-truncated-border-0bcd.json';
import { capturePlanCountQuestion, matchesNativePlanQuestion } from './helpers/claude-pty-runner';
import type { NativePlanQuestionCall } from './helpers/plan-count-transcript';

const screen = captured.screen;
const pending = (): NativePlanQuestionCall => structuredClone(captured.call);
const withoutBorder = screen.slice(screen.indexOf('\n') + 1);
test('removing only native top chrome preserves the previously supported complete prefix', () => {
  expect(screen.endsWith(withoutBorder)).toBe(true);
  expect(matchesNativePlanQuestion(withoutBorder, pending())).toBe(true);
});

for (const [name, visible] of [
  ['CRLF transport', screen.replaceAll('\n', '\r\n')],
  ['blank viewport padding', '\n \n' + screen],
  ['blank rail padding', '│\n' + screen],
] as const) test(`native truncated border preserves ${name}`, () => {
  const call = pending();
  expect(matchesNativePlanQuestion(visible, call)).toBe(true);
  expect(capturePlanCountQuestion(visible, new Set(), 0, true, call)?.nativeCall).toBe(call);
});

const negatives: [string, (value: string) => string][] = [
  ['foreign introduction', value => 'Quoted earlier menu:\n' + value],
  ['foreign introduction inside border', value => value.replace('\n', '\nQuoted earlier menu:\n')],
  ['fenced copy', value => '```text\n' + value + '\n```'],
  ['competing header', value => value.replace(' ☐ Layout', ' ☐ Other\n ☐ Layout')],
  ['foreign header', value => value.replace('☐ Layout', '☐ Other')],
  ['changed visible question prefix', value => value.replace('D2 — Complexity gate', 'D2 — Different gate')],
  ['missing native elision', value => value.replace('❌ two …', '❌ two')],
  ['changed offered label', value => value.replace('1. Simplify class layout', '1. Expand feature scope')],
  ['extra offered action', value => value.replace('4. Type something.', '4. New action')],
  ['missing footer', value => value.replace(' · Esc to cancel', '')],
  ['later prose', value => value + '\nAnother unrelated prompt'],
  ['duplicate border', value => value.split('\n')[0] + '\n' + value],
];
for (const [name, mutate] of negatives) test(`native border cannot authorize ${name}`, () => {
  const changed = mutate(screen), call = pending();
  expect(changed).not.toBe(screen);
  expect(matchesNativePlanQuestion(changed, call)).toBe(false);
  expect(capturePlanCountQuestion(changed, new Set(), 0, true, call)?.nativeCall).toBeUndefined();
});
