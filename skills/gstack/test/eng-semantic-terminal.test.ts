import { describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import captured from './fixtures/eng-fb10-count-public.json';
import { evaluateOwnedNativePlanTerminal, hasNativePlanTerminal } from './helpers/claude-pty-runner';
import { type PlanReviewDecisionJudgment } from './helpers/plan-review-decisions';
import type { PlanCountTranscript } from './helpers/plan-count-transcript';

const start = Date.parse(captured.windowStart);
const exit = captured.provenance.nativeExitUses[0]!.fields;
function native(): PlanCountTranscript {
  return { status: 'ready', calls: structuredClone(captured.calls), assistantMessages: [],
    planReadyRequests: [{ sessionId: captured.calls[0]!.sessionId, toolUseId: exit.toolUseId, timestamp: exit.timestamp, failed: false }] };
}
// Explicit diagnostic mutation only. The original six differently named
// columns remain a real writer failure; no model verdict is supplied here.
const correctedColumns = captured.report.replace('| Review | Skill | Runs | Status | Last run | Notes |',
  '| Review | Trigger | Runs | Status | Why | Findings |');
const line = (fragment: string) => captured.report.split('\n').find(value => value.includes(fragment))!;

function judgment(ids = captured.calls.map(c => `${c.sessionId}:${c.toolUseId}`)): PlanReviewDecisionJudgment {
  return { questions: captured.calls.map((call, i) => ({ toolUseId: ids[i]!, questionIndex: 1,
    kind: i === 11 ? 'workflow' : 'finding', independentDecisions: i === 11 ? 0 : 1,
    targetIds: ({ 0: ['sequential-idp'], 2: ['complexity'], 3: ['shared-cache'], 5: ['swallowed-errors'] } as Record<number,string[]>)[i] ?? [],
    evidence: [{ field: 'question', optionIndex: null, quote: call.questions[0]!.question.split('\n')[0]! }],
    reason: 'Supplied protocol response, not a model semantic judgment.', optionActions: [],
  })), engReview: { status: 'complete', reason: 'Supplied structural response; semantic approval is deliberately not claimed.',
    regression: [
      { role: 'critical', source: 'finalPlan', quote: line('**Regression contract (D7=A, CRITICAL)') },
      { role: 'baseline', source: 'finalPlan', quote: line('characterization tests pinning `legacyAuthFlow()` outputs on the same fixture table') },
      { role: 'replay', source: 'finalPlan', quote: line('For each case, run `legacyAuthFlow()`') },
      { role: 'assertions', source: 'finalPlan', quote: line('Verify: every fixture asserts equal decision') },
      { role: 'approved-differences', source: 'finalPlan', quote: line('Intentional differences in this PR:') },
    ], approvals: [{ toolUseId: ids[6]!, questionIndex: 1, selectedOptionIndex: 1, quote: line('Accepted scope: **CRITICAL regression contract.') }],
    navigation: [{ toolUseId: ids[11]!, questionIndex: 1, quote: line('Gating conditions before flag flip:') }],
  } };
}
function response(prompt: string) {
  const marker = /BEGIN_UNTRUSTED_([a-f0-9]{32})\n/.exec(prompt)!;
  const data = JSON.parse(prompt.slice(marker.index + marker[0].length, prompt.lastIndexOf(`\nEND_UNTRUSTED_${marker[1]}`)));
  expect(data.calls).toHaveLength(12);
  expect(data.engReview.finalPlan).toBe(correctedColumns);
  return judgment(data.calls.map((c: { toolUseId: string }) => c.toolUseId));
}
function fileFixture(report = correctedColumns) {
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'eng-native-terminal-')));
  const file = path.join(directory, 'report.md');
  fs.writeFileSync(file, report);
  fs.utimesSync(file, new Date(captured.provenance.reportMtimeMs), new Date(captured.provenance.reportMtimeMs));
  return { file, cleanup: () => fs.rmSync(directory, {recursive:true, force:true}) };
}

describe('Eng single terminal semantic assessment', () => {
  test('real native Exit can assess the older report, then enforces semantic navigation freshness', async () => {
    const f = fileFixture();
    try {
      expect(hasNativePlanTerminal(native(), f.file, start, 'plan_ready')).toBe(false);
      let calls=0;
      const result=await evaluateOwnedNativePlanTerminal(native(), f.file, start, Date.now()+60_000, async () => {
        calls++; return {administrativeCallIds:[captured.calls[11]!.sessionId+':'+captured.calls[11]!.toolUseId],
          substantiveCallIds:captured.calls.slice(0,11).map(call=>call.sessionId+':'+call.toolUseId)};
      });
      expect(calls).toBe(1); expect(result?.administrative.size).toBe(1); expect(result?.substantive.size).toBe(11);
      expect(hasNativePlanTerminal(native(), f.file, start, 'plan_ready', result?.administrative)).toBe(true);
    } finally { f.cleanup(); }
  });
  for (const [name, change] of Object.entries({
    'missing Exit': (t:PlanCountTranscript) => { t.planReadyRequests=[]; },
    'failed Exit': (t:PlanCountTranscript) => { t.planReadyRequests![0]!.failed=true; },
    'foreign Exit session': (t:PlanCountTranscript) => { t.planReadyRequests![0]!.sessionId='foreign'; },
    'Exit before latest answer': (t:PlanCountTranscript) => { t.planReadyRequests![0]!.timestamp=t.calls.at(-1)!.answeredAt!; },
    'pending native question': (t:PlanCountTranscript) => { t.calls[2]!.answered=false; },
    'failed native question': (t:PlanCountTranscript) => { t.calls[2]!.failed=true; },
    'foreign answer': (t:PlanCountTranscript) => { t.calls[2]!.answers!['another question']='A'; },
    'duplicate identity': (t:PlanCountTranscript) => { t.calls.push(structuredClone(t.calls[2]!)); },
    'unoffered answer': (t:PlanCountTranscript) => { t.calls[2]!.answers![t.calls[2]!.questions[0]!.question]='Never offered'; },
    'out-of-window answer': (t:PlanCountTranscript) => { t.calls[2]!.answeredAt=new Date(start-1).toISOString(); },
  })) test('owned native terminal rejects '+name+' before assessment', async () => {
    const f=fileFixture(); const t=native(); change(t);let calls=0;
    try { expect(await evaluateOwnedNativePlanTerminal(t,f.file,start,Date.now()+1000,async()=>{calls++;return {administrativeCallIds:[],substantiveCallIds:[]};})).toBeUndefined(); expect(calls).toBe(0); }
    finally {f.cleanup();}
  });
  test('a late substantive answer cannot be hidden by incomplete navigation evidence', async()=>{
    const f=fileFixture();try{
      await expect(evaluateOwnedNativePlanTerminal(native(),f.file,start,Date.now()+60_000,async()=>
        ({administrativeCallIds:[],substantiveCallIds:captured.calls.map(call=>call.sessionId+':'+call.toolUseId)})))
        .rejects.toThrow('fresh after every substantive native answer');
    }finally{f.cleanup();}
  });
  test('the assessment cannot mutate the native snapshot used for final freshness',async()=>{
    const f=fileFixture();try{
      const result=await evaluateOwnedNativePlanTerminal(native(),f.file,start,Date.now()+1000,async context=>{
        context.transcript.calls[0]!.answeredAt=new Date(Date.now()).toISOString();
        return {administrativeCallIds:[captured.calls[11]!.sessionId+':'+captured.calls[11]!.toolUseId],
          substantiveCallIds:captured.calls.slice(0,11).map(call=>call.sessionId+':'+call.toolUseId)};
      });expect(result?.substantive.size).toBe(11);
    }finally{f.cleanup();}
  });
  test('a changed file or stale file cannot borrow an earlier successful assessment',async()=>{
    const f=fileFixture();try{
      await expect(evaluateOwnedNativePlanTerminal(native(),f.file,start,Date.now()+1000,async()=>{
        fs.appendFileSync(f.file,'\nnew work');return {administrativeCallIds:[captured.calls[11]!.sessionId+':'+captured.calls[11]!.toolUseId], substantiveCallIds:[captured.calls[0]!.sessionId+':'+captured.calls[0]!.toolUseId]};
      })).rejects.toThrow('report changed');
      fs.utimesSync(f.file,new Date(start-1),new Date(start-1));let calls=0;
      expect(await evaluateOwnedNativePlanTerminal(native(),f.file,start,Date.now()+1000,async()=>{calls++;return {administrativeCallIds:[],substantiveCallIds:[]};})).toBeUndefined();expect(calls).toBe(0);
    }finally{f.cleanup();}
  });
  test('the original absolute deadline bounds a stuck semantic callback without another window',async()=>{
    const f=fileFixture();try{
      await expect(evaluateOwnedNativePlanTerminal(native(),f.file,start,Date.now()+10,async()=>new Promise(()=>{}))).rejects.toThrow('absolute case deadline');
    }finally{f.cleanup();}
  });
});
