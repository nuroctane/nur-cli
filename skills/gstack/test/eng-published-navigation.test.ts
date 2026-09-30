import {expect,test} from 'bun:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import a from './fixtures/eng-published-navigation.json';
import currentMenu from './fixtures/eng-completed-navigation-cab3.json';
import retryPacket from './fixtures/eng-a689-retry-public.json';
import {nativePlanCallFingerprint,hasNativePlanTerminal} from './helpers/claude-pty-runner';
import type {NativePlanQuestionCall,PlanCountTranscript} from './helpers/plan-count-transcript';

const plan=a.plan;
const retryNavigation=()=>({plan:retryPacket.report,call:structuredClone(retryPacket.calls.at(-1)!) as NativePlanQuestionCall,priorCalls:structuredClone(retryPacket.calls.slice(0,-1)) as NativePlanQuestionCall[]});
const currentLedgerCf74 = currentMenu.currentLedgerCf74;
const ledgerNavigationCf74 = (name: 'first' | 'retry', reconcile = false) => {
  const x = structuredClone(currentLedgerCf74[name]);
  const call = x.transcript.calls.at(-1)!;
  if (reconcile) {
    expect((x.plan.match(/^State: pending$/gm) ?? []).length).toBe(6);
    x.plan = x.plan.replace(/^State: pending$/gm, 'State: approved');
  }
  return { ...x, call, priorCalls: x.transcript.calls.slice(0, -1) };
};

test('retry navigation binds its own fingerprint and independent native exit evidence',()=>{
 const x=retryNavigation(),fp=nativePlanCallFingerprint(x.call,0,false);
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'eng-retry-navigation-')),file=path.join(dir,'report.md');
 try{
  fs.writeFileSync(file,x.plan);const at=retryPacket.provenance.report.mtimeMs;fs.utimesSync(file,at/1000,at/1000);
  const transcript:PlanCountTranscript={status:'ready',calls:[...x.priorCalls,x.call],assistantMessages:[],planReadyRequests:structuredClone(retryPacket.planReadyRequests)};
  const admin=new Set([fp.signature]),start=Date.parse(retryPacket.windowStart);
  const check=(t=transcript,ids=admin)=>hasNativePlanTerminal(t,file,start,'plan_ready',ids);
  expect(check()).toBe(true);
  expect(check(transcript,new Set())).toBe(false);expect(check(transcript,new Set(['foreign:call']))).toBe(false);
  for(const edit of [
   (t:PlanCountTranscript)=>{t.planReadyRequests=[];},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.failed=true;},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.sessionId='foreign';},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.timestamp=new Date(start-1).toISOString();},
   (t:PlanCountTranscript)=>{t.calls[5]!.answeredAt=x.call.answeredAt;},
  ]){const t=structuredClone(transcript);edit(t);expect(check(t)).toBe(false);}
  fs.writeFileSync(file,'## GSTACK REVIEW REPORT\n');fs.utimesSync(file,at/1000,at/1000);expect(check()).toBe(false);
  // Replaying public events tests the detector; it cannot promote the paid run.
  expect(retryPacket.originalOutcome).toBe('timeout');
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});

test('the real completed navigation preserves freshness only for its own acknowledged answer',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'eng-published-navigation-')),file=path.join(dir,'review.md');
 try {
  fs.writeFileSync(file,plan);const at=Date.parse(a.reportWriteAt);fs.utimesSync(file,at/1000,at/1000);
  const call=structuredClone(a.call) as NativePlanQuestionCall;
  const fp=nativePlanCallFingerprint(call,0,false);
  const transcript:PlanCountTranscript={status:'ready',calls:[...structuredClone(a.priorCalls),call] as NativePlanQuestionCall[],assistantMessages:[],planReadyRequests:structuredClone(a.planReadyRequests)};
  const admin=new Set([fp.signature]),check=(t=transcript,ids=admin)=>hasNativePlanTerminal(t,file,Date.parse(a.startedAt),'plan_ready',ids);
  expect(check()).toBe(true);expect(check(transcript,new Set())).toBe(false);expect(check(transcript,new Set(['foreign:call']))).toBe(false);
  for(const mutate of [
   (t:PlanCountTranscript)=>{t.calls[0]!.answeredAt=call.answeredAt;},
   (t:PlanCountTranscript)=>{t.calls[1]!.answered=false;t.calls[1]!.unansweredQuestionIndices=[0];},
   (t:PlanCountTranscript)=>{t.planReadyRequests=[];},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.failed=true;},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.sessionId='foreign';},
  ]){const t=structuredClone(transcript);mutate(t);expect(check(t)).toBe(false);}
  fs.writeFileSync(file,'## GSTACK REVIEW REPORT\n');fs.utimesSync(file,at/1000,at/1000);expect(check()).toBe(false);
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});

test('current completed D19 alone is administrative; report and other answers retain exact freshness',()=>{
 const x=structuredClone(currentMenu), dir=fs.mkdtempSync(path.join(os.tmpdir(),'eng-current-menu-')),file=path.join(dir,'review.md');
 const now=Date.now;
 try {
  Date.now=()=>Date.parse(x.captureAt);fs.writeFileSync(file,x.plan);fs.utimesSync(file,x.reportMtimeMs/1000,x.reportMtimeMs/1000);
  const fp=nativePlanCallFingerprint(x.call,0,false),admin=new Set([fp.signature]);
  const transcript:PlanCountTranscript={status:'ready',calls:[...x.priorCalls,x.call],assistantMessages:[],planReadyRequests:x.planReadyRequests};
  const check=(t=transcript,ids=admin)=>hasNativePlanTerminal(t,file,x.startedAt,'plan_ready',ids);
  expect(check()).toBe(true);expect(check(transcript,new Set())).toBe(false);expect(check(transcript,new Set(['foreign:call']))).toBe(false);
  for(const mutate of [
   (t:PlanCountTranscript)=>{t.calls[0]!.answeredAt=x.call.answeredAt;},
   (t:PlanCountTranscript)=>{t.calls[0]!.answered=false;t.calls[0]!.unansweredQuestionIndices=[0];},
   (t:PlanCountTranscript)=>{t.planReadyRequests=[];},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.failed=true;},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.sessionId='foreign';},
  ]){const t=structuredClone(transcript);mutate(t);expect(check(t)).toBe(false);}
 }finally{Date.now=now;fs.rmSync(dir,{recursive:true,force:true});}
});

test('approved investigation recap preserves every independent native terminal requirement',()=>{
 const x=structuredClone(currentMenu.pendingInvestigationRetry),dir=fs.mkdtempSync(path.join(os.tmpdir(),'eng-investigation-menu-')),file=path.join(dir,'review.md'),now=Date.now;
 try{
  Date.now=()=>Date.parse(x.captureAt);fs.writeFileSync(file,x.plan);fs.utimesSync(file,x.reportMtimeMs/1000,x.reportMtimeMs/1000);
  const fp=nativePlanCallFingerprint(x.call,0,false),admin=new Set([fp.signature]);
  const transcript:PlanCountTranscript={status:'ready',calls:[...x.priorCalls,x.call],assistantMessages:[],planReadyRequests:x.planReadyRequests};
  const check=(t=transcript,ids=admin)=>hasNativePlanTerminal(t,file,x.startedAt,'plan_ready',ids);
  expect(check()).toBe(true);expect(check(transcript,new Set())).toBe(false);expect(check(transcript,new Set(['foreign:call']))).toBe(false);
  for(const mutate of [
   (t:PlanCountTranscript)=>{t.calls[1]!.answeredAt=x.call.answeredAt;}, // unrelated TODO is still modifying
   (t:PlanCountTranscript)=>{t.calls[0]!.answeredAt=x.call.answeredAt;},
   (t:PlanCountTranscript)=>{t.calls.at(-1)!.answered=false;t.calls.at(-1)!.unansweredQuestionIndices=[0];},
   (t:PlanCountTranscript)=>{t.planReadyRequests=[];},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.timestamp=x.priorCalls[0]!.answeredAt;},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.failed=true;},
   (t:PlanCountTranscript)=>{t.planReadyRequests![0]!.sessionId='foreign';},
  ]){const t=structuredClone(transcript);mutate(t);expect(check(t)).toBe(false);}
  expect(fs.statSync(file).mtimeMs).toBeCloseTo(x.reportMtimeMs,0);
  fs.writeFileSync(file,'## GSTACK REVIEW REPORT\n');fs.utimesSync(file,x.reportMtimeMs/1000,x.reportMtimeMs/1000);expect(check()).toBe(false);
 }finally{Date.now=now;fs.rmSync(dir,{recursive:true,force:true});}
});

test('cf74 reconciled navigation retains independent native freshness and exit requirements',()=>{
  const x=ledgerNavigationCf74('retry',true), dir=fs.mkdtempSync(path.join(os.tmpdir(),'eng-current-ledger-terminal-')), file=path.join(dir,'review.md'), now=Date.now;
  try {
    const t=x.transcript as PlanCountTranscript;
    Date.now=()=>Date.parse(t.planReadyRequests!.at(-1)!.timestamp)+1000;
    fs.writeFileSync(file,x.plan);fs.utimesSync(file,x.report.mtimeMs/1000,x.report.mtimeMs/1000);
    const fp=nativePlanCallFingerprint(x.call,0,false), admin=new Set([fp.signature]);
    const check=(v=t,ids=admin)=>hasNativePlanTerminal(v,file,x.startedAt,'plan_ready',ids);
    expect(check()).toBe(true);expect(check(t,new Set())).toBe(false);expect(check(t,new Set(['foreign:call']))).toBe(false);
    for(const change of [
      (v:PlanCountTranscript)=>{v.planReadyRequests=[];},
      (v:PlanCountTranscript)=>{v.planReadyRequests![0]!.failed=true;},
      (v:PlanCountTranscript)=>{v.planReadyRequests![0]!.sessionId='foreign';},
      (v:PlanCountTranscript)=>{v.planReadyRequests![0]!.timestamp=x.priorCalls[0]!.answeredAt!;},
      (v:PlanCountTranscript)=>{v.calls.at(-1)!.answered=false;v.calls.at(-1)!.unansweredQuestionIndices=[0];},
      (v:PlanCountTranscript)=>{v.calls[6]!.answeredAt=x.call.answeredAt;},
      (v:PlanCountTranscript)=>{v.calls[0]!.answeredAt=x.call.answeredAt;},
    ]){const v=structuredClone(t);change(v);expect(check(v)).toBe(false);}
    expect(fs.statSync(file).mtimeMs).toBeCloseTo(x.report.mtimeMs,0);
    fs.writeFileSync(file,'## GSTACK REVIEW REPORT\n');fs.utimesSync(file,x.report.mtimeMs/1000,x.report.mtimeMs/1000);expect(check()).toBe(false);
  } finally { Date.now=now;fs.rmSync(dir,{recursive:true,force:true}); }
});
