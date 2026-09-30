import { describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { hasNativePlanTerminal, nativePlanCallFingerprint } from './helpers/claude-pty-runner';
import type { NativePlanQuestionCall } from './helpers/plan-count-transcript';
import captured from './fixtures/design-handoff-n-calls.json';
import capturedQ from './fixtures/design-handoff-q-calls.json';

const calls = () => structuredClone(captured.calls) as NativePlanQuestionCall[];
const handoff = () => calls().at(-1)!;
const fp = (call: NativePlanQuestionCall) => nativePlanCallFingerprint(call, 0, false);
describe('scored Design completion and required next gate', () => {
  test('the captured report predates only handoff; absent native Exit still cannot complete', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'design-scored-handoff-'));
    const file = path.join(dir, 'plan.md');
    try {
      fs.writeFileSync(file, captured.report.content);
      const reportAt = Date.parse(captured.report.successfulUpdateAt) / 1000;
      fs.utimesSync(file, reportAt, reportAt);
      const input = calls();
      const transcript = { status: 'ready' as const, calls: input, assistantMessages: [],
        planReadyRequests: structuredClone(captured.planReadyRequests) };
      const admin = new Set([fp(input.at(-1)!).signature]);
      const start = Date.parse('2026-09-09T01:06:22Z');
      expect(Date.parse(input.at(-2)!.answeredAt!)).toBeLessThan(reportAt * 1000);
      expect(Date.parse(input.at(-1)!.answeredAt!)).toBeGreaterThan(reportAt * 1000);
      expect(hasNativePlanTerminal(transcript, file, start, 'plan_ready', admin)).toBe(false);
      // A controlled later Exit exercises freshness without inventing historical evidence.
      const exit = { sessionId: input[0]!.sessionId, toolUseId: 'controlled-exit',
        timestamp: '2026-09-09T01:19:10Z', failed: false };
      transcript.planReadyRequests.push(exit as never);
      expect(hasNativePlanTerminal(transcript, file, start, 'plan_ready')).toBe(false);
      expect(hasNativePlanTerminal(transcript, file, start, 'plan_ready', admin)).toBe(true);
      exit.failed = true;
      expect(hasNativePlanTerminal(transcript, file, start, 'plan_ready', admin)).toBe(false);
      exit.failed = false;
      const stale = Date.parse(input.at(-2)!.answeredAt!) / 1000 - 1;
      fs.utimesSync(file, stale, stale);
      expect(hasNativePlanTerminal(transcript, file, start, 'plan_ready', admin)).toBe(false);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe('completed Design review with added decisions and an offered manual stop', () => {
  const qCalls = () => structuredClone(capturedQ.calls) as NativePlanQuestionCall[];
  const qHandoff = () => qCalls().at(-1)!;
});
