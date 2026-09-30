import {test, expect} from 'bun:test';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import captured from './fixtures/review-count-markdown-6f.json';
import {nativePlanCallFingerprint, planCountQuestionPhase,
  engStep0Boundary, engFirstReviewAUQ, engSetupAUQ} from './helpers/claude-pty-runner';
import {createEngBatchingIssueCounter} from './helpers/eng-seeded-coverage';
import type {NativePlanQuestionCall} from './helpers/plan-count-transcript';

const design = captured.cases[0]!;
const calls = (entry: typeof design) => structuredClone(entry.calls) as NativePlanQuestionCall[];

function actualEngCaller(name: string, report: string) {
  const source = fs.readFileSync(path.join(import.meta.dir, name+'.test.ts'), 'utf8');
  const start=source.indexOf('const findings = createEngBatchingIssueCounter');
  const end=source.indexOf('const obs = await runPlanSkillCounting',start);
  expect(start).toBeGreaterThan(0);expect(end).toBeGreaterThan(start);
  return new Function('fs','planPath','createEngBatchingIssueCounter','engSetupAUQ',
    new Bun.Transpiler({loader:'ts'}).transformSync(source.slice(start,end))+'return findings;')(
      fs,report,createEngBatchingIssueCounter,engSetupAUQ) as ReturnType<typeof createEngBatchingIssueCounter>;
}
for(const [entry,expected] of [[captured.cases[1]!,['record:R1','record:R5','record:R3']]] as const) test('actual '+entry.case+' caller retains saved Markdown choice identities',()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'review-markdown-')),report=path.join(dir,'report.md');
  try {
    const counter=actualEngCaller(entry.case,report),input=calls(entry);let started=false;
    const classifications=input.map((call,index)=>{
      const planId=(entry.preAnswerPlanIds as Record<string,string>)[call.toolUseId];
      if(planId)fs.writeFileSync(report,(entry.plans as Record<string,string>)[planId]!);
      const fp=nativePlanCallFingerprint(call,0,!started);
      const phase=planCountQuestionPhase(fp,started,engStep0Boundary,engFirstReviewAUQ,engSetupAUQ);
      // This is the runner's actual explicit-counter override, including prior calls.
      fp.preReview=!counter.isReviewAUQ(fp,input.slice(0,index));started=phase.reviewStarted;
      return fp.preReview;
    });
    expect(counter.trace.map(row=>row.issue)).toEqual([...expected]);
    expect(classifications.filter(value=>!value)).toHaveLength(expected.length);
  } finally {fs.rmSync(dir,{recursive:true,force:true});}
});

// The seeded-coverage case now uses its native seed contract. This saved-brief
// defect still must be rejected directly by the strict batching parser.
test('a missing pre-answer brief never earns saved-ledger batching credit',()=>{
  const entry=captured.cases[2]!,input=calls(entry);let plan='';
  const counter=createEngBatchingIssueCounter(()=>plan,engSetupAUQ);
  for(const [index,call] of input.entries()){
    const id=(entry.preAnswerPlanIds as Record<string,string>)[call.toolUseId];
    if(id)plan=(entry.plans as Record<string,string>)[id]!;
    expect(counter.isReviewAUQ(nativePlanCallFingerprint(call,0,true),input.slice(0,index))).toBe(false);
  }
  expect(counter.trace).toEqual([]);
});
const batch=captured.cases[1]!;
function batchEntry(index=2){
  const call=calls(batch)[index]!;
  const id=(batch.preAnswerPlanIds as Record<string,string>)[call.toolUseId]!;
  return {call,plan:(batch.plans as Record<string,string>)[id]!};
}
function batchAccept(change:(entry:ReturnType<typeof batchEntry>)=>void,index=2){
  const entry=batchEntry(index);change(entry);
  return createEngBatchingIssueCounter(()=>entry.plan,engSetupAUQ).isReviewAUQ(nativePlanCallFingerprint(entry.call,0,true));
}
for(const delimiter of ['.',':','']) test('descriptive grid header keeps its letter with delimiter '+delimiter,()=>{
  expect(batchAccept(e=>{e.plan=e.plan.replace(/\| ([A-C])\) /g,`| $1${delimiter} `);})).toBe(true);
});
test('native option order does not change saved descriptive column identities',()=>{
  expect(batchAccept(e=>{e.call.questions[0]!.options.reverse();})).toBe(true);
});
for(const [name,change] of Object.entries({
  'duplicate header identity':(e:ReturnType<typeof batchEntry>)=>{e.plan=e.plan.replace('| B) Keep custom','| A) Keep custom');},
  'wrong header identity':(e:ReturnType<typeof batchEntry>)=>{e.plan=e.plan.replace('| B) Keep custom','| D) Keep custom');},
  'header contradicts saved brief':(e:ReturnType<typeof batchEntry>)=>{e.plan=e.plan.replace('C) Investigate: bounded read of the library\'s retry hook API before deciding','C) Delete customer records');},
  'native appended action':(e:ReturnType<typeof batchEntry>)=>{const q=e.call.questions[0]!;q.options[2]!.label+=' and delete customer records';e.call.answers={[q.question]:q.options[2]!.label};},
  'native negated action':(e:ReturnType<typeof batchEntry>)=>{const q=e.call.questions[0]!;q.options[2]!.label='C) Do not investigate hook API first';e.call.answers={[q.question]:q.options[2]!.label};},
  'foreign subrow owners':(e:ReturnType<typeof batchEntry>)=>{e.plan=e.plan.replace(/\| R5([a-d]) /g,'| R50$1 ');},
  'duplicate owned subrow':(e:ReturnType<typeof batchEntry>)=>{e.plan=e.plan.replace('| R5b max attempts','| R5a max attempts');},
  'empty owned dimension':(e:ReturnType<typeof batchEntry>)=>{e.plan=e.plan.replace('| R5b max attempts | unspecified |','| R5b max attempts | |');},
})) test('descriptive columns and owned dimensions reject '+name,()=>{
  expect(batchAccept(change,name.includes('subrow')||name.includes('dimension')?3:2)).toBe(false);
});
test('several owned grid dimensions remain one decision even when it is reopened',()=>{
  const first=batchEntry(3);let plan=first.plan;
  const counter=createEngBatchingIssueCounter(()=>plan,engSetupAUQ);
  expect(counter.isReviewAUQ(nativePlanCallFingerprint(first.call,0,true))).toBe(true);
  const later=structuredClone(first.call);later.toolUseId+='-reopened';
  const q=later.questions[0]!,answer=later.answers![q.question]!;q.question=q.question.replace('D4 —','D44 —');later.answers={[q.question]:answer};
  plan=plan.replaceAll('D4','D44');
  expect(counter.isReviewAUQ(nativePlanCallFingerprint(later,0,true),[first.call])).toBe(false);
  expect(counter.trace.map(row=>row.issue)).toEqual(['record:R5']);
});
