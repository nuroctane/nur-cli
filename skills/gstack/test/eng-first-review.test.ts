/**
 * Eng first-review classification (engStep0Boundary / engSetupAUQ / engFirstReviewAUQ) over captured native calls.
 */
import { describe } from 'bun:test';
import { expect } from 'bun:test';
import { test } from 'bun:test';
import captured_eng_annotated_cache_au from './fixtures/eng-annotated-cache-au.json';
import { engFirstReviewAUQ } from './helpers/claude-pty-runner';
import { engSetupAUQ } from './helpers/claude-pty-runner';
import { engStep0Boundary } from './helpers/claude-pty-runner';
import { nativePlanCallFingerprint } from './helpers/claude-pty-runner';
import { planCountQuestionPhase } from './helpers/claude-pty-runner';
import type { NativePlanQuestionCall } from './helpers/plan-count-transcript';
import fixture_eng_architecture_cache_av from './fixtures/eng-architecture-cache-av-calls.json';
import captured_eng_binding_retry_z from './fixtures/eng-binding-retry-z-calls.json';
import captured_eng_binding_z from './fixtures/eng-binding-z-calls.json';
import type { AskUserQuestionFingerprint as FP } from './helpers/claude-pty-runner';
import fixture_eng_cache_brief_am from './fixtures/eng-cache-brief-am.json';
import fixture_eng_cache_owner_an from './fixtures/eng-cache-owner-an.json';
import type { AskUserQuestionFingerprint as Fingerprint } from './helpers/claude-pty-runner';
import captured_eng_cache_writes_as from './fixtures/eng-cache-writes-as.json';
import captured_eng_count_ad_v2 from './fixtures/eng-count-ad-v2.json';
import captured_eng_declarative_as from './fixtures/eng-declarative-as.json';
import captured_eng_declared_retry_at from './fixtures/eng-declared-retry-at.json';
import captured_eng_first_category_af from './fixtures/eng-first-category-af.json';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import fixture_eng_injected_export_aq from './fixtures/eng-injected-export-aq.json';
import fixture_eng_library_hooks_aq from './fixtures/eng-library-hooks-aq.json';
import captured_eng_scope_y from './fixtures/eng-scope-y-calls.json';

describe('eng-annotated-cache-au', () => {
const captured = captured_eng_annotated_cache_au;
const fresh=()=>structuredClone(captured.call) as NativePlanQuestionCall;
const fp=(c=fresh())=>nativePlanCallFingerprint(c,Date.parse(c.answeredAt!),true);
const first=(c=fresh())=>engFirstReviewAUQ(fp(c));
function change(edit:(q:NativePlanQuestionCall['questions'][number])=>void){const c=fresh(),q=c.questions[0]!,picked=q.options.findIndex(o=>o.label===c.answers[q.question]);edit(q);c.answers={[q.question]:q.options[picked]!.label};return c;}
test('exact acknowledged annotated cache finding opens review without changing native ownership',()=>{
 const c=fresh(),before=JSON.stringify(c);expect(first(c)).toBe(true);expect(engSetupAUQ(fp(c))).toBe(false);expect(planCountQuestionPhase(fp(c),false,engStep0Boundary,engFirstReviewAUQ,engSetupAUQ)).toMatchObject({preReview:false,reviewStarted:true});expect(JSON.stringify(c)).toBe(before);expect(captured.provenance.retrospectivePass).toBe(false);
});
test('incidental metadata and all offered choices retain substantive review identity',()=>{
 for(const edit of [
  (q:any)=>{q.question=q.question.replace('D5 — Issue 1','D15 — Issue 11');q.header='Arch 11';},
  (q:any)=>{q.question=q.question.replace('PLAN.md:19-20 + :10','docs/plan.md:42');},
  (q:any)=>{q.question=q.question.replace('[P1] (confidence 8/10)','[P2] (confidence 10/10)');},
  (q:any)=>{q.question=q.question.replaceAll('AuthCache','TenantStore').replaceAll('SessionMint','SessionWriter').replaceAll('AuthBroker','AuthReader');q.options=q.options.map((o:any)=>({...o,description:o.description.replaceAll('AuthCache','TenantStore').replaceAll('SessionMint','SessionWriter').replaceAll('AuthBroker','AuthReader')}));},
  (q:any)=>{q.question+='\n"Historical note: This finding is withdrawn."';},
  (q:any)=>{q.options[0].description+='\n"This option is withdrawn."';},
 ])expect(first(change(edit))).toBe(true);
 for(const reversed of [false,true])for(let i=0;i<3;i++){const c=fresh(),q=c.questions[0]!;if(reversed)q.options.reverse();c.answers={[q.question]:q.options[i]!.label};expect(first(c)).toBe(true);}
});
const changes:Array<[string,(q:NativePlanQuestionCall['questions'][number])=>void]>=[
 ['foreign issue header',q=>{q.header='Arch 2';}],['missing issue',q=>{q.question=q.question.replace('Issue 1 ','');}],['missing annotation',q=>{q.question=q.question.replace('[P1] (confidence 8/10) ','');}],['missing source location',q=>{q.question=q.question.replace('PLAN.md:19-20 + :10 — ','');}],['invalid confidence',q=>{q.question=q.question.replace('confidence 8/10','confidence 11/10');}],
 ['conditional defect',q=>{q.question=q.question.replace('both mutate','might both mutate');}],['same actor twice',q=>{q.question=q.question.replace('AuthBroker and SessionMint','AuthBroker and AuthBroker');}],['serialized title',q=>{q.question=q.question.replace('does not serialize mutations','serializes mutations');}],
 ['source title',q=>{q.question='Source: '+q.question;}],['quoted title',q=>{const lines=q.question.split('\n');lines[0]='"'+lines[0]+'"';q.question=lines.join('\n');}],['source context',q=>{q.question=q.question.replace('Project/branch/task:','Source:');}],['historical context',q=>{q.question=q.question.replace('Project/branch/task:','Project/branch/task: Historical assessment:');}],
 ['no own explanation',q=>{q.question=q.question.replace(/^ELI10:.*$/m,'');}],['quoted explanation',q=>{q.question=q.question.replace(/^ELI10: (.*)$/m,'ELI10: "$1"');}],['competing explanation',q=>{q.question+='\nELI10: There is no race.';}],['hypothetical explanation',q=>{q.question=q.question.replace('ELI10:','ELI10: If approved,');}],['missing race consequence',q=>{q.question=q.question.replace('the mint can land after the invalidation and a suspended tenant keeps a live session','the tenant always loses the session');}],
 ['repair wrong cache',q=>{q.options[0]!.description=q.options[0]!.description!.replace('AuthCache passed','OtherCache passed');}],['same writer and reader',q=>{q.options[0]!.description=q.options[0]!.description!.replace('AuthBroker reads','SessionMint reads');}],['missing invalidation rejection',q=>{q.options[0]!.description=q.options[0]!.description!.replace('are rejected if the entry was invalidated since read','are accepted even when invalidated');}],['missing owned repair',q=>{q.options[0]!.description='Choose later.';}],['missing opposed risk',q=>{q.options[2]!.description='The race is closed.';}],['opposition now serialized',q=>{q.options[2]!.description+='\nThe writers are now serialized.';}],['reader also writes',q=>{q.options[0]!.description+='\nAuthBroker also writes.';}],
];
test.each(changes)('%s cannot open review',(_,edit)=>expect(first(change(edit))).toBe(false));
test('current statuses, framing and conditional approval are enforced on finding and offered outcomes',()=>{
 for(const status of ['withdrawn','no longer current','hypothetical','optional'])for(const [open,close]of [['',''],['"','"'],["'","'"],['“','”'],['‘','’'],['`','`']]){
  for(const owner of ['This finding','D5','Issue 1'])expect(first(change(q=>{q.question+=`\n**${owner}** is ${open}${status}${close}.`;})),`${owner} ${open}${status}`).toBe(false);
  for(const i of [0,1,2])expect(first(change(q=>{q.options[i]!.description+=`\n**This option** is ${open}${status}${close}.`;}))).toBe(false);
 }
 for(const prefix of ['Source:','Historical assessment:','If approved,','Once approved,','Pending approval:'])for(const i of [0,1,2])expect(first(change(q=>{q.options[i]!.description=prefix+'\n'+q.options[i]!.description;})),prefix).toBe(false);
});
test('native completion, timestamp, exact answer, session and visible menu stay mandatory',()=>{
 const edits:Array<(c:NativePlanQuestionCall)=>void>=[c=>{c.answered=false;},c=>{c.failed=true;},c=>{c.answers={};},c=>{c.answers[c.questions[0]!.question]='not offered';},c=>{c.unansweredQuestionIndices=[0];},c=>{c.answeredAt='invalid';},c=>{c.sessionId='';},c=>{c.toolUseId='';},c=>{c.questions[0]!.multiSelect=true;},c=>{c.questions.push(structuredClone(c.questions[0]!));},c=>{c.questions[0]!.options[1]!.label=c.questions[0]!.options[0]!.label;}];
 for(const edit of edits){const c=fresh();edit(c);expect(first(c)).toBe(false);}const f=fp();expect(engFirstReviewAUQ({...f,signature:'foreign'})).toBe(false);expect(engFirstReviewAUQ({...f,options:f.options.slice().reverse()})).toBe(false);expect(engFirstReviewAUQ({...f,nativeQuestionIndex:1})).toBe(false);
});
test('current approval conditions and same-option effort boundaries cannot hide withdrawals',()=>{
 for(const phrase of ['requires approval','is conditional on approval','is contingent on acceptance']) for(const target of ['finding','option']) expect(first(change(q=>{if(target==='finding')q.question+='\nThis finding '+phrase+'.';else q.options[0]!.description+='\nThis option '+phrase+'.';}))).toBe(false);
 for(const status of ['withdrawn','no longer current']) for(const [open,close]of [['',''],['"','"'],["'","'"],['“','”'],['‘','’']]) expect(first(change(q=>{q.options[0]!.description=q.options[0]!.description!.replace(/\.$/,'')+` This option is ${open}${status}${close}.`;}))).toBe(false);
 expect(first(change(q=>{q.options[2]!.description+='\nOnly SessionMint writes.';}))).toBe(false);
 expect(first(change(q=>{q.options[0]!.description+='\nDo not inject the cache.';}))).toBe(false);
});

test('the injection-only alternative must retain its stated unresolved race',()=>{
 for(const text of ['AuthCache is now serialized.','Only SessionMint writes.']) expect(first(change(q=>{q.options[1]!.description+='\n'+text;}))).toBe(false);
 for(const text of ['"AuthCache is now serialized."',"'Only SessionMint writes.'",'ArchiveCache is now serialized.']) expect(first(change(q=>{q.options[1]!.description+='\n'+text;}))).toBe(true);
});
});

describe('eng-architecture-cache-av', () => {
const fixture = fixture_eng_architecture_cache_av;
const fresh=()=>structuredClone(fixture.call) as NativePlanQuestionCall;
const fp=(c:NativePlanQuestionCall)=>nativePlanCallFingerprint(c,0,true);
const accepted=(c:NativePlanQuestionCall)=>engFirstReviewAUQ(fp(c));
type Q=NativePlanQuestionCall['questions'][number];
function edit(change:(q:Q,c:NativePlanQuestionCall)=>void){const c=fresh(),q=c.questions[0]!;change(q,c);c.answers={[q.question]:q.options[0]!.label};return c;}

describe('declarative architecture issue owns the current cache mutation decision',()=>{
 test('the exact completed public decision establishes review before the counter records it',()=>{
  const c=fresh(),before=JSON.stringify(c);
  expect(c.toolUseId).toBe('toolu_0147MKgbsvnFruWMDXzQUGVv');
  expect(c.answeredAt).toBe('2026-09-10T23:03:42.025Z');
  expect(accepted(c)).toBe(true);
  expect(engSetupAUQ(fp(c))).toBe(false);
  expect(planCountQuestionPhase(fp(c),false,engStep0Boundary,engFirstReviewAUQ,engSetupAUQ)).toEqual({preReview:false,reviewStarted:true});
  expect(JSON.stringify(c)).toBe(before);
 });
 test('actor and cache renaming, citation changes, decision ordinals and offered deferral keep meaning',()=>{
  const rename=JSON.parse(JSON.stringify(fresh()).replaceAll('AuthBroker','CredentialReader').replaceAll('SessionMint','SessionWriter').replaceAll('AuthCache','TenantCache'));
  expect(accepted(rename)).toBe(true);
  expect(accepted(edit(q=>{q.question=q.question.replaceAll('PLAN.md:19-20','docs/REVISED.md:31-33').replaceAll('PLAN.md:10','docs/REVISED.md:12');}))).toBe(true);
  expect(accepted(edit(q=>{q.header='Arch 7';q.question=q.question.replace('D3 — Architecture issue 1','D22 — Architecture issue 7').replace(/\b1([ABC])\b/g,'7$1');q.options.forEach(o=>{o.label=o.label.replace(/^1/,'7');});}))).toBe(true);
  expect(accepted(edit(q=>{q.question=q.question.replace('unserialized mutations\n','unserialized mutations.\n');}))).toBe(true);
  expect(accepted(edit(q=>q.options.reverse()))).toBe(true);
  for(const option of fresh().questions[0]!.options){const c=fresh();c.answers={[c.questions[0]!.question]:option.label};expect(accepted(c)).toBe(true);}
 });
 test('the common native completion and identity gates remain necessary',()=>{
  for(const mutation of [
   (c:NativePlanQuestionCall)=>{c.answered=false;},(c:NativePlanQuestionCall)=>{c.failed=true;},
   (c:NativePlanQuestionCall)=>{delete c.answeredAt;},(c:NativePlanQuestionCall)=>{c.answeredAt='invalid';},
   (c:NativePlanQuestionCall)=>{c.unansweredQuestionIndices=[0];},(c:NativePlanQuestionCall)=>{c.answers={};},
   (c:NativePlanQuestionCall)=>{c.answers={[c.questions[0]!.question]:'unoffered'};},
   (c:NativePlanQuestionCall)=>{c.questions[0]!.multiSelect=true;},
   (c:NativePlanQuestionCall)=>{c.questions.push(structuredClone(c.questions[0]!));},
  ]){const c=fresh();mutation(c);expect(accepted(c)).toBe(false);}
  for(const mutation of [
   (f:ReturnType<typeof fp>)=>{f.signature='foreign:request';},(f:ReturnType<typeof fp>)=>{f.nativeCall!.sessionId='foreign';},
   (f:ReturnType<typeof fp>)=>{f.nativeCall!.toolUseId='foreign';},(f:ReturnType<typeof fp>)=>{f.nativeQuestionIndex=1;},
   (f:ReturnType<typeof fp>)=>{f.options.reverse();},
  ]){const f=fp(fresh());mutation(f);expect(engFirstReviewAUQ(f)).toBe(false);}
 });
 test('finding metadata and the current shared-cache premise must agree',()=>{
  for(const mutation of [
   (q:Q)=>{q.header='Arch 2';},(q:Q)=>{q.header='Scope';},
   (q:Q)=>{q.options[0]!.label=q.options[0]!.label.replace(/^1A/,'2A');},
   (q:Q)=>{q.question=q.question.replace('global mutable AuthCache','global mutable OtherCache');},
   (q:Q)=>{q.question=q.question.replace('has AuthBroker and SessionMint','has AuthBroker and AuthBroker');},
   (q:Q)=>{q.question=q.question.replace('nothing serializes','the queue serializes');},
   (q:Q)=>{q.question=q.question.replace('ELI10: PLAN.md','ELI10: If approved, PLAN.md');},
   (q:Q)=>{q.question=q.question.replace(/^ELI10: (.+)$/m,'ELI10: "$1"');},
   (q:Q)=>{q.question=q.question.replace(/^ELI10: (.+)$/m,'> ELI10: $1');},
   (q:Q)=>{q.question='Source example:\n'+q.question;},
   (q:Q)=>{q.question='```text\n'+q.question+'\n```';},
   (q:Q)=>{q.question+='\nELI10: No current race remains.';},
   (q:Q)=>{q.question=q.question.replace('Picture SessionMint','Picture OtherWriter');},
   (q:Q)=>{q.question=q.question.replace('refreshed token for tenant A','refreshed token for tenant B');},
  ])expect(accepted(edit(mutation))).toBe(false);
 });
 test('the same offered remedy must inject the named cache, serialize its writes and require tenant identity',()=>{
  for(const mutation of [
   (q:Q)=>{q.options[0]!.label=q.options[0]!.label.replace('Inject AuthCache','Inject OtherCache');},
   (q:Q)=>{q.options[0]!.label=q.options[0]!.label.replace('by constructor','through a global export');},
   (q:Q)=>{q.options[0]!.label=q.options[0]!.label.replace('owns all writes','accepts unowned writes');},
   (q:Q)=>{q.options[0]!.label=q.options[0]!.label.replace('serializes per tenant key','leaves writes unordered');},
   (q:Q)=>{q.options[0]!.label=q.options[0]!.label.replace('requires tenant context','allows missing tenant context');},
   (q:Q)=>{q.options[0]!.description=q.options[0]!.description!.replace('run in order through one owner','run concurrently through both services');},
   (q:Q)=>{q.options[0]!.description=q.options[0]!.description!.replace('fresh AuthCache per case','shared AuthCache for all cases');},
   (q:Q)=>{q.options[0]!.description=q.options[0]!.description!.replace('No method accepts a call without','Every method accepts a call without');},
   (q:Q)=>{q.options[0]!.description='Historical example: '+q.options[0]!.description;},
   (q:Q)=>{q.options[0]!.description='If approved: '+q.options[0]!.description;},
   (q:Q)=>{q.options[0]!.description='> '+q.options[0]!.description;},
   (q:Q)=>{q.options[0]!.description+='\nAuthBroker still writes directly.';},
   (q:Q)=>{q.options[0]!.description+='\nSerialization is optional.';},
  ])expect(accepted(edit(mutation))).toBe(false);
 });
 test('the opposed choice must actually leave the current race open',()=>{
  for(const mutation of [
   (q:Q)=>{q.options[2]!.label='1C: Resolve the race';},
   (q:Q)=>{q.options[2]!.description='The cache is already safe and serialized.';},
   (q:Q)=>{q.options[2]!.description='Historical example: '+q.options[2]!.description;},
   (q:Q)=>{q.options[2]!.description+='\nAuthCache is already serialized.';},
   (q:Q)=>{q.options[2]!.description+='\nOnly AuthBroker writes.';},
   (q:Q)=>{q.options[1]!.description+='\nOnly AuthBroker writes.';},
   (q:Q)=>{q.question+='\nAuthCache now serializes all writes.';},
   (q:Q)=>{q.question+='\nOnly SessionMint writes.';},
   (q:Q)=>{q.question+='\nDo not inject this cache.';},
  ])expect(accepted(edit(mutation))).toBe(false);
 });
 test('owned current statuses and approvals override the earlier finding across scalar quote forms',()=>{
  for(const target of [-1,0,1,2])for(const owner of ['This finding','D3','Architecture issue 1'])for(const suffix of [" is 'withdrawn'.",' is “no longer current”.',' is `unproven`.',' is optional.',' requires approval.']){
   const c=edit(q=>{const text='\nAssessment complete; '+owner+suffix;if(target<0)q.question+=text;else q.options[target]!.description+=text;});
   expect(accepted(c)).toBe(false);
  }
  for(const target of [-1,0,2])for(const text of ['\nPrior note: "This finding is withdrawn."','\n> This finding is withdrawn.','\nA previous reviewer said `This finding is withdrawn.`','\nOtherCache is already serialized.']){
   expect(accepted(edit(q=>{if(target<0)q.question+=text;else q.options[target]!.description+=text;}))).toBe(true);
  }
 });
});
});

describe('eng-binding-retry-z', () => {
const captured = captured_eng_binding_retry_z;
const fresh = () => structuredClone(captured[1]!) as NativePlanQuestionCall;
const fp = (c: NativePlanQuestionCall) => nativePlanCallFingerprint(c, 0, true);
const first = (c: NativePlanQuestionCall) => engFirstReviewAUQ(fp(c));
function question(c: NativePlanQuestionCall, transform: (s: string) => string) {
  const q = c.questions[0]!; const answer = c.answers![q.question]!;
  q.question = transform(q.question); c.answers = {[q.question]: answer}; return c;
}

describe('Z Eng shared mutable cache starts substantive review', () => {
  test('the actual shared mutable cache risk starts review without an issue label', () => {
    expect(engSetupAUQ(fp(fresh()))).toBe(false);
    expect(first(fresh())).toBe(true);
    expect(planCountQuestionPhase(fp(fresh()), false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ))
      .toEqual({preReview: false, reviewStarted: true});
  });

  test('the exact six native calls preserve one setup and all five review obligations', () => {
    let started = false;
    const phases = captured.map(c => {
      const p = planCountQuestionPhase(fp(structuredClone(c) as NativePlanQuestionCall), started, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ);
      started = p.reviewStarted; return p.preReview;
    });
    expect(phases).toEqual([true, false, false, false, false, false]);
    expect(first(structuredClone(captured[0]!) as NativePlanQuestionCall)).toBe(false);
    expect(captured[5]!.questions[0]!.header).toBe('TODO: E2E test');
  });

  test('either offered choice, reordering and a different component retain issue identity', () => {
    const c = fresh(); c.questions[0]!.options.reverse();
    for (const option of c.questions[0]!.options) {
      c.answers = {[c.questions[0]!.question]: option.label}; expect(first(c)).toBe(true);
    }
    const varied = question(fresh(), s => s.replace('AuthCache', 'SessionCache').replace('D2', 'D7'));
    for (const option of varied.questions[0]!.options) option.description = option.description.replaceAll('AuthCache', 'SessionCache');
    expect(first(varied)).toBe(true);
  });

  test('requires a complete native call and exact offered answer and fingerprint', () => {
    for (const mutate of [
      (c: NativePlanQuestionCall) => { c.answered = false; },
      (c: NativePlanQuestionCall) => { c.failed = true; },
      (c: NativePlanQuestionCall) => { delete c.failed; },
      (c: NativePlanQuestionCall) => { delete c.unansweredQuestionIndices; },
      (c: NativePlanQuestionCall) => { c.unansweredQuestionIndices = [0]; },
      (c: NativePlanQuestionCall) => { c.questions[0]!.multiSelect = true; },
      (c: NativePlanQuestionCall) => { c.questions.push(structuredClone(c.questions[0]!)); },
      (c: NativePlanQuestionCall) => { c.questions[0]!.options.push(structuredClone(c.questions[0]!.options[0]!)); },
      (c: NativePlanQuestionCall) => { c.questions[0]!.options[1]!.label = c.questions[0]!.options[0]!.label; },
      (c: NativePlanQuestionCall) => { c.answers = {}; },
      (c: NativePlanQuestionCall) => { c.answers = {[c.questions[0]!.question]: 'Foreign answer'}; },
    ]) { const c = fresh(); mutate(c); expect(first(c)).toBe(false); }
    expect(engFirstReviewAUQ({...fp(fresh()), signature: 'foreign:call'})).toBe(false);
    expect(engFirstReviewAUQ({...fp(fresh()), nativeCall: undefined})).toBe(false);
    expect(engFirstReviewAUQ({...fp(fresh()), options: []})).toBe(false);
    const mismatch = fp(fresh()); mismatch.options[0]!.label = 'foreign'; expect(engFirstReviewAUQ(mismatch)).toBe(false);
    const wrongIndex = fp(fresh()); wrongIndex.options[0]!.index = 2; expect(engFirstReviewAUQ(wrongIndex)).toBe(false);
  });

  test('requires an affirmative direct risk, not setup, denial, qualification or quotations', () => {
    for (const header of ['Scope', 'Approach', 'Next review', 'Onboarding']) {
      const c = fresh(); c.questions[0]!.header = header; expect(first(c)).toBe(false);
    }
    for (const transform of [
      (s: string) => s.replace('Architecture:', 'Approach:'),
      (s: string) => s.replace('Two services share', 'If two services share'),
      (s: string) => s.replace('Two services share', 'Two services do not share'),
      (s: string) => s.replace('can corrupt tenant isolation', 'cannot corrupt tenant isolation'),
      (s: string) => s.replace('can corrupt tenant isolation', 'never corrupt tenant isolation'),
      (s: string) => s.replace('This is the #1 reliability risk', 'This is not the #1 reliability risk'),
      (s: string) => s.replace('plan-eng-shared-mutable-cache', 'plan-eng-setup'),
      (s: string) => s.replace('plan-eng-shared-mutable-cache', 'foreign-shared-mutable-cache'),
      (s: string) => s.replace(/ <gstack-qid:[^>]+>/, ''),
      (s: string) => s + ' <gstack-qid:plan-eng-shared-mutable-cache>',
      (s: string) => s + ' Run the next review too.',
      (s: string) => '> ' + s,
      (s: string) => '```text\n' + s + '\n```',
    ]) expect(first(question(fresh(), transform))).toBe(false);
  });

  test('the complete offered remedies stay tied to the same dependency and affirmative risk', () => {
    for (const [index, transform] of [
      [0, (s: string) => s.replace('The plan is updated', 'The plan is not updated')],
      [0, (s: string) => s.replace('pass AuthCache', 'pass DifferentCache')],
      [0, (s: string) => s.replace('No module-level mutable export.', 'Keep the module-level mutable export.')],
      [1, (s: string) => s.replace('still couples both services', 'does not couple both services')],
      [2, (s: string) => s.replace('as a known risk', 'as a dismissed risk')],
      [0, (s: string) => s + ' Also grant every tenant access.'],
      [1, (s: string) => s + ' Also approve the missing timeout policy.'],
      [0, (s: string) => '> ' + s],
      [2, (s: string) => '```text\n' + s + '\n```'],
    ] as const) {
      const c = fresh(); const option = c.questions[0]!.options[index]!;
      option.description = transform(option.description ?? ''); expect(first(c)).toBe(false);
    }
    const c = fresh(); c.questions[0]!.options[0]!.label = 'Run /office-hours';
    c.answers = {[c.questions[0]!.question]: 'Run /office-hours'}; expect(first(c)).toBe(false);
  });
});
});

describe('eng-binding-z', () => {
const captured = captured_eng_binding_z;
const fresh = () => structuredClone(captured[1]!) as NativePlanQuestionCall;
const fp = (c: NativePlanQuestionCall) => nativePlanCallFingerprint(c, 0, true);
const first = (c: NativePlanQuestionCall) => engFirstReviewAUQ(fp(c));
function question(c: NativePlanQuestionCall, transform: (s: string) => string) {
  const q = c.questions[0]!; const answer = c.answers![q.question]!;
  q.question = transform(q.question); c.answers = {[q.question]: answer}; return c;
}

describe('Z Eng dependency binding starts substantive review', () => {
  test('the actual cache dependency decision starts review without an issue label', () => {
    expect(engSetupAUQ(fp(fresh()))).toBe(false);
    expect(first(fresh())).toBe(true);
    expect(planCountQuestionPhase(fp(fresh()), false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ))
      .toEqual({preReview: false, reviewStarted: true});
  });

  test('the exact six native calls preserve one setup and all five review obligations', () => {
    let started = false;
    const phases = captured.map(c => {
      const p = planCountQuestionPhase(fp(structuredClone(c) as NativePlanQuestionCall), started, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ);
      started = p.reviewStarted; return p.preReview;
    });
    expect(phases).toEqual([true, false, false, false, false, false]);
    expect(first(structuredClone(captured[0]!) as NativePlanQuestionCall)).toBe(false);
    expect(captured[5]!.questions[0]!.header).toBe('TODO: Timeout');
  });

  test('either offered choice, reordering and a different component retain issue identity', () => {
    const c = fresh(); c.questions[0]!.options.reverse();
    for (const option of c.questions[0]!.options) {
      c.answers = {[c.questions[0]!.question]: option.label}; expect(first(c)).toBe(true);
    }
    const varied = question(fresh(), s => s.replace('AuthBroker', 'SessionGateway').replace('D2', 'D7'));
    for (const option of varied.questions[0]!.options) option.description = option.description.replaceAll('AuthBroker', 'SessionGateway');
    expect(first(varied)).toBe(true);
  });

  test('requires a complete native call and exact offered answer and fingerprint', () => {
    for (const mutate of [
      (c: NativePlanQuestionCall) => { c.answered = false; },
      (c: NativePlanQuestionCall) => { c.failed = true; },
      (c: NativePlanQuestionCall) => { delete c.failed; },
      (c: NativePlanQuestionCall) => { delete c.unansweredQuestionIndices; },
      (c: NativePlanQuestionCall) => { c.unansweredQuestionIndices = [0]; },
      (c: NativePlanQuestionCall) => { c.questions[0]!.multiSelect = true; },
      (c: NativePlanQuestionCall) => { c.questions.push(structuredClone(c.questions[0]!)); },
      (c: NativePlanQuestionCall) => { c.questions[0]!.options.push(structuredClone(c.questions[0]!.options[0]!)); },
      (c: NativePlanQuestionCall) => { c.questions[0]!.options[1]!.label = c.questions[0]!.options[0]!.label; },
      (c: NativePlanQuestionCall) => { c.answers = {}; },
      (c: NativePlanQuestionCall) => { c.answers = {[c.questions[0]!.question]: 'Foreign answer'}; },
    ]) { const c = fresh(); mutate(c); expect(first(c)).toBe(false); }
    expect(engFirstReviewAUQ({...fp(fresh()), signature: 'foreign:call'})).toBe(false);
    expect(engFirstReviewAUQ({...fp(fresh()), nativeCall: undefined})).toBe(false);
    expect(engFirstReviewAUQ({...fp(fresh()), options: []})).toBe(false);
    const mismatch = fp(fresh()); mismatch.options[0]!.label = 'foreign'; expect(engFirstReviewAUQ(mismatch)).toBe(false);
    const wrongIndex = fp(fresh()); wrongIndex.options[0]!.index = 2; expect(engFirstReviewAUQ(wrongIndex)).toBe(false);
  });

  test('setup, foreign, hypothetical, quoted and mixed questions cannot open review', () => {
    for (const header of ['Scope', 'Approach', 'Next review', 'Onboarding']) {
      const c = fresh(); c.questions[0]!.header = header; expect(first(c)).toBe(false);
    }
    for (const transform of [
      (s: string) => s.replace('Architecture:', 'Approach:'),
      (s: string) => s.replace('How should AuthBroker', 'If needed, how should AuthBroker'),
      (s: string) => s.replace('AuthBroker access', 'the whole plan access'),
      (s: string) => s.replace('plan-eng-cache-binding', 'plan-eng-setup'),
      (s: string) => s.replace('plan-eng-cache-binding', 'foreign-cache-binding'),
      (s: string) => s.replace(/ <gstack-qid:[^>]+>/, ''),
      (s: string) => s + ' <gstack-qid:plan-eng-cache-binding>',
      (s: string) => s + ' Approve the release too.',
      (s: string) => '> ' + s,
      (s: string) => '```text\n' + s + '\n```',
    ]) expect(first(question(fresh(), transform))).toBe(false);
  });

  test('both descriptions must affirm the existing dependency and remedy without extra obligations', () => {
    for (const [index, transform] of [
      [0, (s: string) => s.replace('Eliminates module-level mutable state entirely.', 'Does not eliminate module-level mutable state.')],
      [0, (s: string) => s.replace('Eliminates module-level mutable state entirely.', 'If shared state exists, eliminates it.')],
      [0, (s: string) => s.replace('AuthBroker receives', 'DifferentComponent receives')],
      [1, (s: string) => s.replace('same pattern as the current plan', 'unlike the current plan')],
      [1, (s: string) => s.replace('makes tests require module-level mocking', 'does not make tests require module-level mocking')],
      [1, (s: string) => s.replace('AuthBroker imports', 'DifferentComponent imports')],
      [0, (s: string) => s + ' Also grant every tenant access.'],
      [1, (s: string) => s + ' Also approve the missing timeout policy.'],
      [0, (s: string) => '> ' + s],
      [1, (s: string) => '```text\n' + s + '\n```'],
    ] as const) {
      const c = fresh(); const option = c.questions[0]!.options[index]!;
      option.description = transform(option.description ?? ''); expect(first(c)).toBe(false);
    }
    const c = fresh(); c.questions[0]!.options[0]!.label = 'Run /office-hours';
    c.answers = {[c.questions[0]!.question]: 'Run /office-hours'}; expect(first(c)).toBe(false);
  });
});
});

describe('eng-cache-brief-am', () => {
const fixture = fixture_eng_cache_brief_am;
const calls=fixture.calls as FP[];
function edit(change:(q:any,f:FP)=>void):FP { const f=structuredClone(calls[1]!),c=f.nativeCall!,q=c.questions[0]!,selected=q.options.findIndex(o=>o.label===c.answers?.[q.question]);change(q,f);c.answers={[q.question]:q.options[selected]!.label};return nativePlanCallFingerprint(c,f.observedAtMs,f.preReview); }
test('the completed current cache ownership brief starts the engineering review',()=>expect(engFirstReviewAUQ(calls[1]!)).toBe(true));
test('the earlier whole-plan scope choice does not become a finding',()=>expect(engFirstReviewAUQ(calls[0]!)).toBe(false));
test('equivalent decision ordinal and current wording retain the owned finding',()=>{
 expect(engFirstReviewAUQ(edit(q=>{q.question=q.question.replace(/^D2/,'D17');q.header='D17 DI';}))).toBe(true);
 expect(engFirstReviewAUQ(edit(q=>{q.question=q.question.replace('Right now both services grab','Today both services import').replace('and both write to it.','and both mutate it.');}))).toBe(true);
});
test('unrelated current decisions remain outside this dependency branch',()=>{
 expect(engFirstReviewAUQ(edit(q=>{q.header='D3 DI';}))).toBe(false);
 expect(engFirstReviewAUQ(edit(q=>{q.question=q.question.replace('Architecture finding A1','Architecture finding A2');}))).toBe(false);
});
const negative:Array<[string,(q:any,f:FP)=>void]>=[
 ['source preface',q=>q.question=q.question.replace('\nELI10:','\nSource excerpt:\nELI10:')],
 ['historical current clause',q=>q.question=q.question.replace('ELI10: Right now','ELI10: Previously')],
 ['hypothetical current clause',q=>q.question=q.question.replace('ELI10: Right now','ELI10: If approved, right now')],
 ['negated current writes',q=>q.question=q.question.replace('and both write to it.','and neither writes to it.')],
 ['quoted current assessment',q=>q.question=q.question.replace('ELI10: Right now','ELI10: "Right now').replace('it. Nobody','it." Nobody')],
 ['withdrawn finding',q=>q.question+='\nCorrection: this finding is withdrawn.'],
 ['resolved current finding',q=>q.question+='\nNo current gap remains.'],
 ['foreign cache title',q=>q.question=q.question.replace('Module-level AuthCache','Module-level OtherCache')],
 ['source remedy preface',q=>q.options[0].description='Source excerpt:\n'+q.options[0].description],
 ['conditional writer ownership',q=>q.options[0].description=q.options[0].description.replace('✅ SessionMint','✅ If SessionMint')],
 ['quoted writer ownership',q=>q.options[0].description=q.options[0].description.replace('✅ SessionMint','✅ "SessionMint').replace('not convention.','not convention."')],
 ['read-only claim only in con',q=>q.options[0].description=q.options[0].description.replace('✅ SessionMint','❌ SessionMint')],
 ['same writable and read-only actor',q=>q.options[0].description=q.options[0].description.replace('AuthBroker gets','SessionMint gets')],
 ['withdrawn remedy',q=>q.options[0].description+=' This remedy is withdrawn.'],
 ['foreign opposing finding',q=>q.options[2].description=q.options[2].description.replace('Both A1','Both A9')],
 ['opposed gap resolved',q=>q.options[2].description+=' No current gap remains.'],
 ['conditional opposed gap',q=>q.options[2].description=q.options[2].description.replace('❌ Both A1','❌ If Both A1')],
 ['unoffered recommendation',q=>q.question=q.question.replace('Recommendation: A','Recommendation: D')],
 ['multiple recommendations',q=>q.options[1].label+=' (recommended)'],
 ['unlettered choice',q=>q.options[1].label=q.options[1].label.slice(3)],
 ['unanswered',(_,f)=>f.nativeCall!.answered=false],
 ['failed',(_,f)=>f.nativeCall!.failed=true],
 ['incomplete member',(_,f)=>f.nativeCall!.unansweredQuestionIndices=[0]],
 ['invalid completion time',(_,f)=>f.nativeCall!.answeredAt='invalid'],
];
test.each(negative)('%s cannot supply current owned engineering review',(_,change)=>expect(engFirstReviewAUQ(edit(change))).toBe(false));
test('whole quoted history and consistent identifiers preserve the current decision',()=>{
 expect(engFirstReviewAUQ(edit(q=>q.question+='\nPrior note: "This finding is withdrawn."'))).toBe(true);
 expect(engFirstReviewAUQ(edit(q=>{q.question=q.question.replaceAll('AuthCache','SessionCache').replaceAll('A1','A9');q.options.forEach((o:any)=>o.description=o.description.replaceAll('A1','A9'));}))).toBe(true);
 expect(engFirstReviewAUQ(edit(q=>{q.question=q.question.replace(/^D2/,'d2');q.header='d2 DI';}))).toBe(true);
});
test('current-owner withdrawals and conditional metadata cannot lend review evidence',()=>{
 for(const text of ['Correction: this finding is rejected.','Correction: this remedy is cancelled.','Correction: this finding is "withdrawn".','Correction: this explanation is not current.']) expect(engFirstReviewAUQ(edit(q=>q.question+='\n'+text))).toBe(false);
 expect(engFirstReviewAUQ(edit(q=>q.question=q.question.replace('Architecture finding A1','If approved, Architecture finding A1')))).toBe(false);
});
});

describe('eng-cache-owner-an', () => {
const fixture = fixture_eng_cache_owner_an;
type Question = NonNullable<Fingerprint['nativeCall']>['questions'][number];
const original = () => structuredClone(fixture.fingerprint) as Fingerprint;
function edit(change: (question: Question) => void): Fingerprint {
  const fp = original(), call = fp.nativeCall!, question = call.questions[0]!;
  const selected = question.options.findIndex(option => option.label === call.answers![question.question]);
  change(question);
  call.answers = { [question.question]: question.options[selected]!.label };
  fp.options = question.options.map((option, index) => ({ index: index + 1, label: option.label }));
  return fp;
}

test('an owned cache-ownership decision starts review with actors named in the current assessment', () => {
  expect(engFirstReviewAUQ(original())).toBe(true);
  expect(planCountQuestionPhase(original(), false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ))
    .toMatchObject({ preReview: false, reviewStarted: true });
  expect(fixture.fingerprint.preReview).toBe(true);
});

test('review identity survives equivalent headers, actor names and an offered opposing answer', () => {
  for (const header of ['Cache owner', 'Cache ownership', 'Shared cache', 'Issue 1', 'Architecture 1'])
    expect(engFirstReviewAUQ(edit(question => { question.header = header; }))).toBe(true);
  expect(engFirstReviewAUQ(edit(question => {
    question.question = question.question.replaceAll('AuthBroker', 'SessionOwner').replaceAll('SessionMint', 'TokenMinter');
    question.options = question.options.map(option => ({ ...option,
      description: option.description?.replaceAll('AuthBroker', 'SessionOwner').replaceAll('SessionMint', 'TokenMinter') }));
  }))).toBe(true);
  for (const option of original().nativeCall!.questions[0]!.options) {
    const fp = original(), call = fp.nativeCall!;
    call.answers = { [call.questions[0]!.question]: option.label };
    expect(engFirstReviewAUQ(fp)).toBe(true);
  }
  expect(engFirstReviewAUQ(edit(question => { question.question += '\nHistorical note: "This finding is withdrawn."'; }))).toBe(true);
});

const rejected: Array<[string, (question: Question) => void]> = [
  ['setup header', q => { q.header = 'Outside voices'; }],
  ['foreign issue identity', q => { q.header = 'Issue 2'; }],
  ['historical title', q => { q.question = 'Historical example:\n' + q.question; }],
  ['source assessment', q => { q.question = q.question.replace('\nELI10:', '\nSource:\nELI10:'); }],
  ['conditional project', q => { q.question = q.question.replace('Project/branch/task: ', 'Project/branch/task: If approved: '); }],
  ['quoted current premise', q => { q.question = q.question.replace(/ELI10: ([^\n]+)/, 'ELI10: "$1"'); }],
  ['conditional current premise', q => { q.question = q.question.replace('ELI10: AuthBroker', 'ELI10: If AuthBroker'); }],
  ['only one actual actor', q => { q.question = q.question.replace('AuthBroker and SessionMint', 'AuthBroker and AuthBroker'); }],
  ['current writes negated', q => { q.question = q.question.replace('both write into', 'neither writes into'); }],
  ['withdrawn finding', q => { q.question += '\nThis finding is withdrawn.'; }],
  ['rejected numbered issue', q => { q.question += '\nIssue 1 is rejected.'; }],
  ['assessment no longer current', q => { q.question += '\nThis assessment is not current.'; }],
  ['foreign writer', q => { q.options[0]!.description = q.options[0]!.description!.replace('Only AuthBroker writes', 'Only OtherService writes'); }],
  ['foreign producer', q => { q.options[0]!.description = q.options[0]!.description!.replace('SessionMint returns', 'OtherService returns'); }],
  ['same writer and producer', q => { q.options[0]!.description = q.options[0]!.description!.replace('SessionMint returns', 'AuthBroker returns'); }],
  ['quoted remedy', q => { q.options[0]!.description = '> ' + q.options[0]!.description; }],
  ['conditional remedy', q => { q.options[0]!.description = 'If approved: ' + q.options[0]!.description; }],
  ['cancelled remedy', q => { q.options[0]!.description += ' This remedy is cancelled.'; }],
  ['explicitly rejected injection', q => { q.options[0]!.description += ' Correction: do not inject the adapter.'; }],
  ['no opposed action', q => { q.options[2]!.label = 'C) Run another review'; }],
  ['quoted deferral', q => { q.options[2]!.description = '> ' + q.options[2]!.description; }],
  ['conditional deferral', q => { q.options[2]!.description = 'If approved: ' + q.options[2]!.description; }],
  ['no retained race', q => { q.options[2]!.description = q.options[2]!.description!.replace('Race stays open', 'Race is closed'); }],
  ['rejected opposed action', q => { q.options[2]!.description += ' This option is rejected.'; }],
  ['withdrawn single-writer requirement', q => { q.options[0]!.description += ' The single-writer requirement is withdrawn.'; }],
  ['producer also writes', q => { q.options[0]!.description += ' Correction: SessionMint will also write directly to the cache.'; }],
  ['retained race closed', q => { q.options[2]!.description += ' Correction: the race is now closed.'; }],
];
test.each(rejected)('%s does not establish the first review decision', (_, change) => {
  expect(engFirstReviewAUQ(edit(change))).toBe(false);
});

test('native ownership, completion, answer alignment and dense menus remain required', () => {
  const invalid: Array<(fp: Fingerprint) => void> = [
    fp => { fp.nativeCall!.answered = false; }, fp => { fp.nativeCall!.failed = true; },
    fp => { fp.signature = 'foreign:call'; }, fp => { fp.nativeQuestionIndex = 1; },
    fp => { fp.nativeCall!.unansweredQuestionIndices = [0]; },
    fp => { delete fp.nativeCall!.answeredAt; }, fp => { fp.nativeCall!.answers = {}; },
    fp => { fp.options.reverse(); },
  ];
  for (const change of invalid) {
    const fp = original(); change(fp);
    expect(engFirstReviewAUQ(fp)).toBe(false);
  }
});
});

describe('eng-cache-writes-as', () => {
const captured = captured_eng_cache_writes_as;
const actual = () => structuredClone(captured.call) as NativePlanQuestionCall;
function answered(c: NativePlanQuestionCall, index = 0) {
  c.answers = { [c.questions[0]!.question]: c.questions[0]!.options[index]!.label };
  return nativePlanCallFingerprint(c, 0, true);
}
function allText(edit: (s: string) => string) {
  const c = actual(), q = c.questions[0]!; q.question = edit(q.question);
  for (const o of q.options) { o.label = edit(o.label); o.description = edit(o.description ?? ''); }
  return c;
}

test('the exact completed retry starts review with the current cache ownership decision', () => {
  const c = actual(), before = JSON.stringify(c), fp = nativePlanCallFingerprint(c, 0, true);
  expect(engFirstReviewAUQ(fp)).toBe(true); expect(engSetupAUQ(fp)).toBe(false);
  expect(planCountQuestionPhase(fp, false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ)).toMatchObject({ preReview: false, reviewStarted: true });
  expect(JSON.stringify(c)).toBe(before); expect(captured.provenance.retrospectivePass).toBe(false);
});

test('all offered choices and consistently renamed actors retain the same review identity', () => {
  for (const reverse of [false, true]) for (let i = 0; i < 3; i++) {
    const c = actual(); if (reverse) c.questions[0]!.options.reverse();
    expect(engFirstReviewAUQ(answered(c, i))).toBe(true);
  }
  for (const [one, two] of [['One', 'Two'], ['$Reader', '_Writer'], ['SessionMint', 'AuthBroker']]) {
    const c = allText(t => t.replaceAll('AuthBroker', '__one__').replaceAll('SessionMint', two).replaceAll('__one__', one));
    expect(engFirstReviewAUQ(answered(c))).toBe(true);
  }
  const c = allText(t => t.replace(/^D2 /, 'D17 ').replace(/\b2([A-C])\b/g, '17$1'));
  expect(engFirstReviewAUQ(answered(c))).toBe(true);
});

test('native completion, session, exact answer and option binding remain mandatory', () => {
  const mutations: Array<(c: NativePlanQuestionCall) => void> = [
    c => { c.answered = false; }, c => { c.failed = true; }, c => { c.answers = {}; },
    c => { c.answers = { [c.questions[0]!.question]: 'unoffered' }; }, c => { c.answers!['foreign'] = 'answer'; },
    c => { c.answeredAt = 'invalid'; }, c => { c.unansweredQuestionIndices = [0]; },
    c => { c.sessionId = ''; }, c => { c.toolUseId = ''; }, c => { c.questions[0]!.multiSelect = true; },
    c => { c.questions.push(structuredClone(c.questions[0]!)); }, c => { c.questions[0]!.options[1]!.label = c.questions[0]!.options[0]!.label; },
  ];
  for (const mutate of mutations) { const c = actual(); mutate(c); expect(engFirstReviewAUQ(nativePlanCallFingerprint(c, 0, true))).toBe(false); }
  const fp = answered(actual());
  expect(engFirstReviewAUQ({ ...fp, signature: 'foreign' })).toBe(false);
  expect(engFirstReviewAUQ({ ...fp, nativeQuestionIndex: 1 })).toBe(false);
  expect(engFirstReviewAUQ({ ...fp, options: [...fp.options].reverse() })).toBe(false);
});

test('title, own context, current assessment and two distinct writers are required', () => {
  for (const edit of [
    (t: string) => 'Source.\n' + t, (t: string) => '> ' + t, (t: string) => '```\n' + t + '\n```',
    (t: string) => t.replace('Who is allowed', 'Who was allowed'),
    (t: string) => t.replace('the auth cache?', 'the billing cache?'),
    (t: string) => t.replace('Project/branch/task:', 'Earlier review:'),
    (t: string) => t.replace('AuthBroker and SessionMint both', 'AuthBroker and AuthBroker both'),
    (t: string) => t.replace('both mutating one backing cache', 'both previously mutating one backing cache'),
    (t: string) => t.replace('ELI10: Two services', 'ELI10: Source. Two services'),
    (t: string) => t.replace('ELI10: Two services', 'ELI10: If approved, two services'),
    (t: string) => t.replace('nothing orders their writes.', 'their writes are serialized.'),
    (t: string) => t.replace('Project/branch/task: ', 'Project/branch/task: Assuming approval, '),
    (t: string) => t.replace('Project/branch/task: ', 'Project/branch/task: Source. '),
    (t: string) => t.replace('Multi-tenant Auth Refactor,', 'Multi-tenant Auth Refactor if approved,'),
  ]) { const c = actual(); c.questions[0]!.question = edit(c.questions[0]!.question); expect(engFirstReviewAUQ(answered(c))).toBe(false); }
  for (const header of ['Scope', 'Issue 1', 'Report', 'Cache examples']) { const c = actual(); c.questions[0]!.header = header; expect(engFirstReviewAUQ(answered(c))).toBe(false); }
});

test('owned current status beats a matching assertion while archived and foreign status does not', () => {
  for (const status of ['withdrawn', 'superseded', 'rejected', 'cancelled', 'closed', 'hypothetical', 'not current', 'no longer current']) {
    for (const [open, close] of [['', ''], ['"', '"'], ["'", "'"], ['“', '”'], ['‘', '’'], ['`', '`']]) {
      for (const target of [-1, 0, 2]) for (const owner of ['This finding', 'D2']) {
        const c = actual(), q = c.questions[0]!, suffix = `\nCorrection: ${owner} is ${open}${status}${close}.`;
        if (target < 0) q.question += suffix; else q.options[target]!.description += suffix;
        expect(engFirstReviewAUQ(answered(c)), `${target}: ${owner} ${open}${status}${close}`).toBe(false);
      }
    }
  }
  for (const tail of ['D29 is withdrawn.', '> This finding is withdrawn.', 'The prior report said "This finding is withdrawn."', 'An archived review recorded this finding is "withdrawn".', 'An archived review recorded this finding is \'withdrawn\'.', '```\nThis finding is withdrawn.\n```']) {
    for (const target of [-1, 0, 2]) { const c = actual(), q = c.questions[0]!;
      if (target < 0) q.question += '\n' + tail; else q.options[target]!.description += '\n' + tail;
      expect(engFirstReviewAUQ(answered(c)), `${target}: ${tail}`).toBe(true);
    }
  }
});

test('a remedy and opposed choice must bind the same current writers and active race', () => {
  for (const index of [0, 2]) for (const prefix of ['Source. ', 'If approved, ', 'Assuming approval, ', 'Historical assessment: ', '> ', '"']) {
    const c = actual(), o = c.questions[0]!.options[index]!; o.description = prefix + o.description + (prefix === '"' ? '"' : '');
    expect(engFirstReviewAUQ(answered(c))).toBe(false);
  }
  for (const index of [0, 1]) for (const name of ['Foreign', 'AuthBroker']) {
    const c = actual(), o = c.questions[0]!.options[index]!; o.description = o.description!.replace('SessionMint', name);
    expect(engFirstReviewAUQ(answered(c))).toBe(false);
  }
  for (const [target, tail] of [
    [-1, 'The services no longer mutate the cache.'], [-1, 'Correction: AuthBroker no longer writes to the cache.'],
    [-1, 'The writes are now serialized.'], [-1, 'The writers are now serialized.'], [2, 'Correction: The writers are now serialized.'], [0, 'Correction: AuthBroker also writes to the cache.'],
    [0, 'The adapter accepts stale writes.'], [0, 'The version check is optional.'],
    [2, 'The race is resolved.'], [2, 'Correction: Do not keep both writers.'],
    [2, 'Only SessionMint writes to the cache.'], [2, 'Both writers no longer mutate the cache.'],
  ] as const) {
    const c = actual(), q = c.questions[0]!; if (target < 0) q.question += '\n' + tail; else q.options[target]!.description += '\n' + tail;
    expect(engFirstReviewAUQ(answered(c)), `${target}: ${tail}`).toBe(false);
  }
  for (const i of [0, 2]) { const c = actual(); c.questions[0]!.options[i]!.label = `2${i ? 'C' : 'A'} Record the report`; expect(engFirstReviewAUQ(answered(c))).toBe(false); }
});
});

describe('eng-count-ad-v2', () => {
const captured = captured_eng_count_ad_v2;
const firstCalls = captured.cases.first.calls as NativePlanQuestionCall[];
const retryCalls = captured.cases.retry.calls as NativePlanQuestionCall[];
const issue = () => structuredClone(retryCalls[3]!);
const fp = (call: NativePlanQuestionCall) => nativePlanCallFingerprint(call, 0, true);
const isFirst = (call: NativePlanQuestionCall) => engFirstReviewAUQ(fp(call));
function setupPacket(): NativePlanQuestionCall {
  const c = issue();
  c.questions = [
    { header: 'Design doc', question: 'No design doc found for this branch. /office-hours produces sharper review input. Run it first?',
      multiSelect: false, options: [{ label: 'Skip — proceed with standard review (recommended)' }, { label: 'Run /office-hours now' }] },
    { header: 'Learnings', question: 'Search learnings from your other projects on this machine?',
      multiSelect: false, options: [{ label: 'Enable cross-project learnings (recommended)' }, { label: 'Keep learnings project-scoped only' }] },
  ];
  c.answers = Object.fromEntries(c.questions.map(q => [q.question, q.options[0]!.label]));
  return c;
}
function changeQuestion(call: NativePlanQuestionCall, change: (s: string) => string) {
  const q = call.questions[0]!, answer = call.answers?.[q.question];
  q.question = change(q.question); call.answers = answer ? { [q.question]: answer } : {}; return call;
}
function census(calls: NativePlanQuestionCall[]) {
  let reviewStarted = false;
  const counts = { setup: 0, review: 0, administrative: 0 };
  const phases = calls.map(call => {
    const phase = planCountQuestionPhase(fp(call), reviewStarted, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ);
    reviewStarted = phase.reviewStarted;
    counts[phase.administrative ? 'administrative' : phase.preReview ? 'setup' : 'review']++;
    return phase;
  });
  return { counts, phases };
}

describe('Eng AD v2 completed native count evidence', () => {
  test('a completed prerequisite and learnings packet closes setup without counting it as a finding', () => {
    for (const reverse of [false, true]) {
      const c = setupPacket(); if (reverse) c.questions.reverse();
      for (const answer of c.questions.find(q => q.header === 'Learnings')!.options) {
        const learning = c.questions.find(q => q.header === 'Learnings')!;
        c.answers![learning.question] = answer.label;
        const phase = planCountQuestionPhase(fp(c), false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ);
        expect(phase).toEqual({ preReview: true, reviewStarted: true });
        expect(planCountQuestionPhase(fp(issue()), phase.reviewStarted,
          engStep0Boundary, engFirstReviewAUQ, engSetupAUQ).preReview).toBe(false);
      }
    }
  });

  test('partial, ambiguous, foreign and prerequisite-running packets cannot close setup', () => {
    for (const mutate of [
      (c: NativePlanQuestionCall) => { c.answered = false; },
      (c: NativePlanQuestionCall) => { c.failed = true; },
      (c: NativePlanQuestionCall) => { c.unansweredQuestionIndices = [1]; },
      (c: NativePlanQuestionCall) => { delete c.answers![c.questions[1]!.question]; },
      (c: NativePlanQuestionCall) => { c.answers![c.questions[1]!.question] = 'unoffered'; },
      (c: NativePlanQuestionCall) => { c.answers![c.questions[0]!.question] = c.questions[0]!.options[1]!.label; },
      (c: NativePlanQuestionCall) => { c.questions[0]!.multiSelect = true; },
      (c: NativePlanQuestionCall) => { c.questions[1]!.options.push({ ...c.questions[1]!.options[0]! }); },
      (c: NativePlanQuestionCall) => { c.questions.push(structuredClone(issue().questions[0]!)); },
      (c: NativePlanQuestionCall) => { c.answeredAt = 'invalid'; },
    ]) {
      const c = setupPacket(); mutate(c); expect(engStep0Boundary(fp(c))).toBe(false);
    }
    expect(engStep0Boundary({ ...fp(setupPacket()), signature: 'foreign:call' })).toBe(false);
    expect(engStep0Boundary({ ...fp(setupPacket()), options: [] })).toBe(false);
    const c = setupPacket();
    c.questions[1]!.header = 'Issue 1';
    expect(engStep0Boundary(fp(c))).toBe(false);
  });

  test('retry ordinary Issue identity starts review without qids, retaining its later TODO', () => {
    const { counts, phases } = census(retryCalls);
    expect(counts).toEqual({ setup: 3, review: 6, administrative: 0 });
    expect(phases.slice(3).every(p => !p.preReview && !p.administrative)).toBe(true);
    for (const call of retryCalls.slice(3, 8)) expect(isFirst(call)).toBe(true);
    expect(retryCalls[8]!.questions[0]!.question).toContain('TODO 1');
    expect(captured.cases.retry.actual.reviewCount).toBe(0);
  });

  test('ordinary issue presentation can vary while completed identity and section number remain bound', () => {
    for (const title of ['Issue 1', 'Finding 1.2 (D17)', 'D42 — Issue 1']) {
      const call = changeQuestion(issue(), s => s.replace('Issue 1 (D4)', title).replace('AuthCache', 'SessionCache'));
      call.questions[0]!.header = title.includes('1.2') ? 'Architecture 1.2' : 'Architecture 1';
      call.questions[0]!.options.reverse();
      for (const option of call.questions[0]!.options) {
        call.answers = { [call.questions[0]!.question]: option.label };
        expect(isFirst(call)).toBe(true);
      }
    }
  });

  test('setup, quoted or mismatched section identities do not start review', () => {
    for (const header of ['Scope', 'Approach', 'Next steps', 'Arch 2', 'Example Arch 1', 'TODO 1']) {
      const call = issue(); call.questions[0]!.header = header; expect(isFirst(call)).toBe(false);
    }
    for (const change of [
      (s: string) => '> ' + s,
      (s: string) => 'Example: ' + s,
      (s: string) => '```text\n' + s + '\n```',
      (s: string) => s.replace('Issue 1 (D4)', 'Issue 2 (D4)'),
      (s: string) => s + ' <gstack-qid:plan-eng-setup>',
    ]) expect(isFirst(changeQuestion(issue(), change))).toBe(false);
    for (const call of [...firstCalls.slice(0, 4), ...retryCalls.slice(0, 3)]) expect(isFirst(call)).toBe(false);
  });

  test('an Issue heading alone cannot turn a confirmation or report action into a finding', () => {
    for (const body of [
      'No defect remains in the cache. Proceed with the next section?',
      'The cache already serializes writes. Confirm this is accurate?',
      'Should I save the reviewed plan now?',
      'Add a section to the reviewed plan?',
      'Serialize the reviewed plan as JSON for the handoff?',
      'Add the completed tests to this report?',
    ]) {
      const c = changeQuestion(issue(), () => 'Issue 1 (D4) — ' + body);
      c.questions[0]!.options = [
        { label: 'Yes', description: 'Confirm this statement; no new implementation work.' },
        { label: 'No', description: 'Do not confirm; no new implementation work.' },
      ];
      c.answers = { [c.questions[0]!.question]: 'Yes' };
      expect(isFirst(c)).toBe(false);
    }
    const c = issue();
    c.questions[0]!.options = [{ label: 'Yes', description: 'Confirm; no new work.' }, { label: 'No', description: 'Decline; no new work.' }];
    c.answers = { [c.questions[0]!.question]: 'Yes' };
    expect(isFirst(c)).toBe(false);
  });

  test('new first-finding path requires exact completed native identity and answer', () => {
    for (const factory of [issue]) {
      const classify = isFirst;
      for (const mutate of [
        (c: NativePlanQuestionCall) => { c.answered = false; },
        (c: NativePlanQuestionCall) => { c.failed = true; },
        (c: NativePlanQuestionCall) => { delete c.failed; },
        (c: NativePlanQuestionCall) => { c.sessionId = ''; },
        (c: NativePlanQuestionCall) => { c.toolUseId = ''; },
        (c: NativePlanQuestionCall) => { delete c.answeredAt; },
        (c: NativePlanQuestionCall) => { c.answeredAt = 'invalid'; },
        (c: NativePlanQuestionCall) => { c.unansweredQuestionIndices = [0]; },
        (c: NativePlanQuestionCall) => { delete c.unansweredQuestionIndices; },
        (c: NativePlanQuestionCall) => { c.questions[0]!.multiSelect = true; },
        (c: NativePlanQuestionCall) => { c.questions.push(structuredClone(c.questions[0]!)); },
        (c: NativePlanQuestionCall) => { c.questions[0]!.options[1]!.label = c.questions[0]!.options[0]!.label; },
        (c: NativePlanQuestionCall) => { c.answers = {}; },
        (c: NativePlanQuestionCall) => { c.answers = { [c.questions[0]!.question]: 'Unoffered' }; },
        (c: NativePlanQuestionCall) => { c.answers!.foreign = 'Foreign'; },
      ]) { const c = factory(); mutate(c); expect(classify(c)).toBe(false); }
      const classifyFp = engFirstReviewAUQ;
      expect(classifyFp({ ...fp(factory()), signature: 'foreign:call' })).toBe(false);
      expect(classifyFp({ ...fp(factory()), nativeCall: undefined })).toBe(false);
      expect(classifyFp({ ...fp(factory()), nativeQuestionIndex: 1 })).toBe(false);
      expect(classifyFp({ ...fp(factory()), options: [] })).toBe(false);
      const wrong = fp(factory()); wrong.options[0]!.index = 2; expect(classifyFp(wrong)).toBe(false);
    }
  });
});
});

describe('eng-declarative-as', () => {
const captured = captured_eng_declarative_as;
const actual = () => structuredClone(captured.call) as NativePlanQuestionCall;
function answered(c: NativePlanQuestionCall, index = 0) {
  c.answers = { [c.questions[0]!.question]: c.questions[0]!.options[index]!.label };
  return nativePlanCallFingerprint(c, 0, true);
}
function edit(replace: (text: string) => string) {
  const c = actual(), q = c.questions[0]!;
  q.question = replace(q.question);
  q.options.forEach(o => { o.label = replace(o.label); o.description = replace(o.description ?? ''); });
  return c;
}

test('a completed declarative cache issue starts review without a question mark or qid', () => {
  const fp = answered(actual());
  expect(engFirstReviewAUQ(fp)).toBe(true);
  expect(engSetupAUQ(fp)).toBe(false);
  expect(planCountQuestionPhase(fp, false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ)).toMatchObject({ preReview: false, reviewStarted: true });
  expect(captured.provenance.retrospectivePass).toBe(false);
});

test('all answers, option orders, identifiers and consistent ordinals qualify', () => {
  for (const reverse of [false, true]) for (let i = 0; i < 3; i++) {
    const c = actual(); if (reverse) c.questions[0]!.options.reverse();
    expect(engFirstReviewAUQ(answered(c, i))).toBe(true);
  }
  for (const name of ['TenantCache', '$Shared', '_Store']) expect(engFirstReviewAUQ(answered(edit(t => t.replaceAll('AuthCache', name))))).toBe(true);
  for (const [one, two] of [['First', 'Second'], ['Z_store', '$Reader'], ['SessionMint', 'AuthBroker']]) {
    expect(engFirstReviewAUQ(answered(edit(t => t.replaceAll('AuthBroker', '__first__').replaceAll('SessionMint', two).replaceAll('__first__', one))))).toBe(true);
  }
  for (const kind of ['Issue', 'Finding']) expect(engFirstReviewAUQ(answered(edit(t => t.replace('Issue 1 ', `${kind} 17 `).replace(/\b1([A-C])\b/g, '17$1'))))).toBe(true);
});

test('native completion and matching answered menu remain required', () => {
  const mutations: Array<(c: NativePlanQuestionCall) => void> = [
    c => { c.answered = false; }, c => { c.failed = true; }, c => { c.answers = {}; },
    c => { c.answers = { [c.questions[0]!.question]: 'unoffered' }; }, c => { c.answers!['foreign'] = 'answer'; },
    c => { c.answeredAt = 'invalid'; }, c => { c.unansweredQuestionIndices = [0]; },
    c => { c.sessionId = ''; }, c => { c.toolUseId = ''; }, c => { c.questions[0]!.multiSelect = true; },
    c => { c.questions.push(structuredClone(c.questions[0]!)); },
    c => { c.questions[0]!.options[1]!.label = c.questions[0]!.options[0]!.label; },
  ];
  for (const mutate of mutations) { const c = actual(); mutate(c); expect(engFirstReviewAUQ(nativePlanCallFingerprint(c, 0, true))).toBe(false); }
  const fp = answered(actual());
  expect(engFirstReviewAUQ({ ...fp, signature: 'foreign' })).toBe(false);
  expect(engFirstReviewAUQ({ ...fp, nativeQuestionIndex: 1 })).toBe(false);
  expect(engFirstReviewAUQ({ ...fp, options: [...fp.options].reverse() })).toBe(false);
});

test('the brief must own a current architecture assessment and distinct writers', () => {
  const changes = [
    (t: string) => 'Example: ' + t, (t: string) => '> ' + t, (t: string) => '```\n' + t + '\n```',
    (t: string) => t.replace('Section 1 Architecture', 'Section 1 Administration'),
    (t: string) => t.replace('ELI10: Two services', 'ELI10: If two services'),
    (t: string) => t.replace('ELI10: Two services', 'ELI10: Source example: Two services'),
    (t: string) => t.replace('AuthBroker and SessionMint share', 'AuthBroker and AuthBroker share'),
    (t: string) => t.replace('share a global mutable', 'used to share a global mutable'),
    (t: string) => t.replace('writes can interleave', 'writes are serialized').replace('no ordering', 'per-key ordering'),
    (t: string) => t.replace('Project/branch/task:', 'Historical assessment:'),
  ];
  for (const change of changes) { const c = actual(); c.questions[0]!.question = change(c.questions[0]!.question); expect(engFirstReviewAUQ(answered(c))).toBe(false); }
  for (const header of ['Setup', 'TODOs', 'Issue 2', 'Review report']) { const c = actual(); c.questions[0]!.header = header; expect(engFirstReviewAUQ(answered(c))).toBe(false); }
});

test('same-decision withdrawals and contrary current state invalidate the issue', () => {
  for (const status of ['withdrawn', 'superseded', 'rejected', 'cancelled', 'resolved', 'closed', 'not current', 'no longer current']) {
    for (const literal of [status, `"${status}"`, `“${status}”`, `'${status}'`, `‘${status}’`, '`' + status + '`']) for (const target of ['question', 'remedy', 'unchanged']) {
      const c = actual(), q = c.questions[0]!, suffix = ` This finding is ${literal}.`;
      if (target === 'question') q.question += suffix; else q.options[target === 'remedy' ? 0 : 2]!.description += suffix;
      expect(engFirstReviewAUQ(answered(c))).toBe(false);
    }
  }
  for (const contradiction of ['The cache is no longer global.', 'The services no longer mutate shared state.', 'No current risk remains.']) {
    const c = actual(); c.questions[0]!.question += '\n' + contradiction; expect(engFirstReviewAUQ(answered(c))).toBe(false);
  }
});

test('technical options cannot be quoted, hypothetical, mismatched or cancelled', () => {
  for (const index of [0, 2]) for (const frame of ['Example: ', 'If approved: ', 'Source excerpt: ', '> ']) {
    const c = actual(); c.questions[0]!.options[index]!.description = frame + c.questions[0]!.options[index]!.description;
    expect(engFirstReviewAUQ(answered(c))).toBe(false);
  }
  for (const [index, suffix] of [[0, ' Correction: Do not remove the module-level export.'], [0, ' The module export remains.'], [0, ' Writes remain unordered.'], [2, ' Correction: Do not proceed as written.'], [2, ' The race is resolved.']] as const) {
    const c = actual(); c.questions[0]!.options[index]!.description += suffix; expect(engFirstReviewAUQ(answered(c))).toBe(false);
  }
  for (const index of [0, 2]) { const c = actual(); c.questions[0]!.options[index]!.label = '1' + (index ? 'C' : 'A') + ') Record in report'; expect(engFirstReviewAUQ(answered(c))).toBe(false); }
  const c = actual(); c.questions[0]!.options[0]!.description = c.questions[0]!.options[0]!.description!.replaceAll('AuthCache', 'UnrelatedCache');
  expect(engFirstReviewAUQ(answered(c))).toBe(false);
});
test('current named-owner contradictions are distinct from foreign and archived references', () => {
  for (const statement of ['AuthCache is no longer global.', 'AuthCache is no longer mutable.', 'AuthBroker no longer mutates the cache.', 'SessionMint no longer writes to the cache.', 'D4 is withdrawn.', 'Correction: This finding is withdrawn.', 'Correction: Issue 1 is "withdrawn".', 'Correction: AuthCache is no longer global.', "Issue 1 is 'withdrawn'.", 'This finding is “withdrawn”.']) {
    for (const boundary of ['\n', '; ']) {
      const c = actual(); c.questions[0]!.question += boundary + statement;
      expect(engFirstReviewAUQ(answered(c))).toBe(false);
    }
  }
  for (const statement of ['Issue 19 is withdrawn.', 'D42 is withdrawn.', 'AnotherCache is no longer global.', 'An archived review recorded this finding is "withdrawn".', "An archived review recorded this finding is 'withdrawn'.", 'The prior report said "This finding is withdrawn."', '> This finding is withdrawn.']) {
    const c = actual(); c.questions[0]!.question += '\n' + statement;
    expect(engFirstReviewAUQ(answered(c))).toBe(true);
  }
  for (const replacement of ['an unrelated billing cache', 'a different cache', 'an OtherCache']) {
    const c = actual(); c.questions[0]!.options[2]!.description = c.questions[0]!.options[2]!.description!.replace('an auth cache', replacement);
    expect(engFirstReviewAUQ(answered(c))).toBe(false);
  }
  const c = actual(); c.questions[0]!.options[2]!.description = c.questions[0]!.options[2]!.description!.replace('an auth cache', 'an AuthCache');
  expect(engFirstReviewAUQ(answered(c))).toBe(true);
});


test('the unchanged option cannot contradict its own remaining cache risk', () => {
  for (const statement of ['Correction: The writers are now serialized.', 'AuthCache is no longer global.', 'The cache is removed.']) {
    const c = actual(); c.questions[0]!.options[2]!.description += '\n' + statement;
    expect(engFirstReviewAUQ(answered(c))).toBe(false);
  }
});
});

describe('eng-declared-retry-at', () => {
const captured = captured_eng_declared_retry_at;
const first=()=>structuredClone(captured) as NativePlanQuestionCall;
const fp=(c=first())=>nativePlanCallFingerprint(c,0,true);
const classify=(c=first())=>engFirstReviewAUQ(fp(c));
function mutated(fn:(c:NativePlanQuestionCall)=>void){const c=first();fn(c);return c;}
function text(fn:(s:string)=>string){return mutated(c=>{const q=c.questions[0]!,answer=c.answers![q.question]!;q.question=fn(q.question);c.answers={[q.question]:answer};});}
describe('declarative engineering retry choice',()=>{
 test('recognizes the actual answered finding without requiring a question mark',()=>{
  expect(classify()).toBe(true);expect(engSetupAUQ(fp())).toBe(false);
 });
 test('consistent issue numbers, option order and chosen option may vary',()=>{
  const c=first(),q=c.questions[0]!;q.question=q.question.replace('D2 — Issue 1:','D8 — Issue 4:').replace('Recommendation: 1A','Recommendation: 4A');q.header='Issue 4';q.options.forEach(o=>{o.label=o.label.replace(/^1/,'4');});q.options.reverse();
  for(const o of q.options){c.answers={[q.question]:o.label};expect(classify(c)).toBe(true);}
 });
 test('native completion, original menu and response ownership remain mandatory',()=>{
  for(const change of [
   (c:NativePlanQuestionCall)=>{c.answered=false;},(c:NativePlanQuestionCall)=>{c.failed=true;},(c:NativePlanQuestionCall)=>{delete c.failed;},(c:NativePlanQuestionCall)=>{c.answeredAt='invalid';},(c:NativePlanQuestionCall)=>{c.sessionId='';},(c:NativePlanQuestionCall)=>{c.toolUseId='';},(c:NativePlanQuestionCall)=>{c.answers={};},(c:NativePlanQuestionCall)=>{c.answers={[c.questions[0]!.question]:'unoffered'};},(c:NativePlanQuestionCall)=>{c.unansweredQuestionIndices=[0];},(c:NativePlanQuestionCall)=>{c.questions.push(structuredClone(c.questions[0]!));},(c:NativePlanQuestionCall)=>{c.questions[0]!.multiSelect=true;},
  ])expect(classify(mutated(change))).toBe(false);
  for(const f of [{...fp(),signature:'foreign:call'},{...fp(),nativeQuestionIndex:1},{...fp(),options:[...fp().options].reverse()}])expect(engFirstReviewAUQ(f)).toBe(false);
 });
 test('issue identity and report administration cannot substitute for the finding',()=>{
  for(const [a,b] of [['Issue 1:','Issue 0:'],['Issue 1:','Issue 01:'],['Issue 1:','Finding 1:'],['PLAN.md:6-8','PLAN.md:0-8'],['D2 —','D02 —']])expect(classify(text(s=>s.replace(a!,b!)))).toBe(false);
  for(const h of ['Issue 2','Routing','Report','Scope'])expect(classify(mutated(c=>{c.questions[0]!.header=h;}))).toBe(false);
  expect(classify(text(s=>s.replace(/^Project\/branch\/task:.*\n/m,'')))).toBe(false);
  expect(classify(text(s=>s.replace('ELI10:','Project/branch/task: unrelated\nELI10:')))).toBe(false);
  expect(classify(mutated(c=>{c.questions[0]!.options[0]!.label='1A) Continue the review';c.answers={[c.questions[0]!.question]:c.questions[0]!.options[0]!.label};}))).toBe(false);
 });
 test('quoted, hypothetical and closed current findings remain excluded',()=>{
  for(const prefix of ['Source excerpt: ','Earlier review assessment: ','If approved, ','Provided approval, ']){
   expect(classify(text(s=>s.replace('ELI10: ','ELI10: '+prefix)))).toBe(false);
   for(const i of [0,2])expect(classify(mutated(c=>{c.questions[0]!.options[i]!.description=prefix+c.questions[0]!.options[i]!.description;}))).toBe(false);
  }
  for(const status of ['withdrawn','resolved','"closed"','“superseded”']){
   expect(classify(text(s=>s+` This finding is ${status}.`))).toBe(false);
   for(const i of [0,2])expect(classify(mutated(c=>{c.questions[0]!.options[i]!.description+=` This option is ${status}.`;}))).toBe(false);
  }
  expect(classify(text(s=>s+'\n"Earlier review assessment: This finding is withdrawn."'))).toBe(true);
  expect(classify(text(s=>s+'\nCorrection: retry scheduling no longer runs inside each worker.'))).toBe(false);
 });
 test('remedy and unchanged choice each retain their own current consequence',()=>{
  for(const [i,a,b] of [[0,'come from the library','might be evaluated later'],[0,'a pure function, trivially unit-tested','five separate implementations'],[2,'a crash or deploy mid-backoff drops the retry','a crash or deploy preserves every retry']] as const)expect(classify(mutated(c=>{const o=c.questions[0]!.options[i]!;o.description=o.description!.replace(a,b);}))).toBe(false);
  expect(classify(mutated(c=>{c.questions[0]!.options[0]!.description+='\nCorrection: the library will not own persistence.';}))).toBe(false);
  expect(classify(mutated(c=>{c.questions[0]!.options[0]!.description+='\nCorrection: do not use the library retry hook.';}))).toBe(false);
  expect(classify(mutated(c=>{c.questions[0]!.options[2]!.description+='\nCorrection: the per-worker scheduler is now crash-safe.';}))).toBe(false);
 });
});

describe('current owner status and approval boundaries',()=>{
const ownedStatusCases:Array<{name:string,expected:boolean,edit:(c:any)=>void}>=[];const add=(name:string,expected:boolean,edit:(c:any)=>void)=>ownedStatusCases.push({name,expected,edit});
const question=(c:any,suffix:string)=>{const q=c.questions[0],answer=c.answers[q.question];q.question+=suffix;c.answers={[q.question]:answer}};
add('exact completed declarative choice',true,()=>{});
for(const owner of ['This finding','Issue 1','D2'])for(const status of ['withdrawn','not current','no longer current'])for(const quote of ['',"'",'‘'])add(`current ${owner} ${quote}${status}`,false,c=>question(c,`\n${owner} is ${quote}${status}${quote==='‘'?'’':quote}.`));
for(const i of [0,2])for(const status of ['withdrawn','not current','no longer current'])for(const quote of ['',"'",'‘'])add(`option ${i} ${quote}${status}`,false,c=>{c.questions[0].options[i].description+=`\nThis option is ${quote}${status}${quote==='‘'?'’':quote}.`});
for(const i of [0,2])for(const condition of ['This option applies only if approved.','This option is conditional on approval.','If approved, proceed with this option.'])add(`option ${i} condition ${condition}`,false,c=>{c.questions[0].options[i].description+='\n'+condition});
for(const condition of ['This finding applies only if approved.','This finding is conditional on approval.'])add('finding condition '+condition,false,c=>question(c,'\n'+condition));
for(const owner of ['Issue 2','D3'])add('foreign closed owner '+owner,true,c=>question(c,`\n${owner} is withdrawn.`));
for(const i of [0,2])add(`quoted historical option${i}`,true,c=>{c.questions[0].options[i].description+='\nEarlier review assessment: "This option is withdrawn."';});
add('quoted historical finding',true,c=>question(c,'\n"Earlier review assessment: This finding is withdrawn."'));
add('quoted title',false,c=>{const q=c.questions[0],a=c.answers[q.question];q.question=q.question.replace(/^(.*)\n/,'"$1"\n');c.answers={[q.question]:a}});
add('absent completion',false,c=>{c.answered=false});add('failed native result',false,c=>{c.failed=true});add('invalid answer time',false,c=>{c.answeredAt='missing'});
add('remedy and unchanged outcomes reversed',false,c=>{const o=c.questions[0].options;[o[0].description,o[2].description]=[o[2].description,o[0].description]});
add('remedy actually declines library persistence',false,c=>{c.questions[0].options[0].description+='\nThe library will not own persistence.'});
add('unchanged is now crash safe',false,c=>{c.questions[0].options[2].description+='\nThe scheduler is now crash-safe.'});
for(const control of ownedStatusCases)test(control.name,()=>{const call=first();control.edit(call);expect(classify(call)).toBe(control.expected);});
});

 test('bold current owners keep their scalar status before source quotes are removed',()=>{
  for(const owner of ['This finding','D2']){
   expect(classify(text(s=>s+`\n**${owner}** is 'withdrawn'.`))).toBe(false);
   expect(classify(text(s=>s+`\n**${owner}** is ‘withdrawn’.`))).toBe(false);
  }
  for(const i of [0,2])expect(classify(mutated(c=>{c.questions[0]!.options[i]!.description+=`\n**This option** is 'withdrawn'.`;}))).toBe(false);
  expect(classify(text(s=>s+'\n"Earlier review assessment: **This finding** is withdrawn."'))).toBe(true);
 });
});

describe('eng-first-category-af', () => {
const captured = captured_eng_first_category_af;
const actual = () => structuredClone(captured.fingerprint.nativeCall) as NativePlanQuestionCall;

test('actual completed Architecture issue starts review', () => {
  const fp = nativePlanCallFingerprint(actual(), 0, true);
  expect(engFirstReviewAUQ(fp)).toBe(true);
  expect(engSetupAUQ(fp)).toBe(false);
  expect(planCountQuestionPhase(fp, false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ))
    .toMatchObject({ preReview: false, reviewStarted: true });
  expect(captured.provenance.retrospectivePass).toBe(false);
});

function answer(c: NativePlanQuestionCall, index = 0) {
  c.answers = {[c.questions[0]!.question]: c.questions[0]!.options[index]!.label};
  return nativePlanCallFingerprint(c, 0, true);
}

test('all offered choices and menu orders remain substantive decisions', () => {
  for (const reverse of [false, true]) for (let index = 0; index < 3; index++) {
    const c = actual(); if (reverse) c.questions[0]!.options.reverse();
    expect(engFirstReviewAUQ(answer(c, index))).toBe(true);
  }
});

test('identifier spelling, writer order and matching issue numbers are incidental', () => {
  for (const [left, right] of [['TenantReader', 'SessionWriter'], ['Z_store', '$AStore'], ['SessionMint', 'AuthBroker']]) {
    const c = actual(); const q = c.questions[0]!;
    q.question = q.question.replace('AuthBroker and SessionMint', `${left} and ${right}`);
    expect(engFirstReviewAUQ(answer(c))).toBe(true);
  }
  for (const kind of ['Issue', 'Finding']) {
    const c = actual(); c.questions[0]!.question = c.questions[0]!.question.replace('D4 — Issue 1', `D87 — ${kind} 12.3`);
    c.questions[0]!.header = `${kind} 12.3`;
    expect(engFirstReviewAUQ(answer(c))).toBe(true);
  }
});

test('native completion, timestamp, answer and menu identity remain mandatory', () => {
  const mutations: Array<(c: NativePlanQuestionCall) => void> = [
    c => { c.answered = false; }, c => { c.failed = true; }, c => { c.answers = {}; },
    c => { c.answers = {[c.questions[0]!.question]: 'not offered'}; },
    c => { c.questions[0]!.question += ' changed'; },
    c => { c.unansweredQuestionIndices = [0]; }, c => { c.answeredAt = 'invalid'; },
    c => { c.sessionId = ''; }, c => { c.toolUseId = ''; },
    c => { c.questions[0]!.multiSelect = true; },
    c => { c.questions.push(structuredClone(c.questions[0]!)); },
    c => { c.questions[0]!.options[1]!.label = c.questions[0]!.options[0]!.label; },
  ];
  for (const mutate of mutations) {
    const c = actual(); mutate(c); expect(engFirstReviewAUQ(nativePlanCallFingerprint(c, 0, true))).toBe(false);
  }
  const fp = nativePlanCallFingerprint(actual(), 0, true);
  expect(engFirstReviewAUQ({...fp,signature:'foreign'})).toBe(false);
  expect(engFirstReviewAUQ({...fp,nativeQuestionIndex:1})).toBe(false);
  expect(engFirstReviewAUQ({...fp,options:[...fp.options].reverse()})).toBe(false);
});

test('administrative, TODO, uncertain and quoted contexts cannot borrow technical labels', () => {
  const base = actual().questions[0]!.question.split('\n')[0]!;
  const titles = [
    'D4 — Issue 1 (Architecture): Record the completed review in TODOs?',
    'D4 — Issue 1 (Architecture): Confirm that the shared cache review is complete?',
    'D4 — Issue 1 (Architecture): Which review runs next?',
    base.replace('AuthBroker and SessionMint both mutate', 'If AuthBroker and SessionMint both mutate'),
    base.replace('AuthBroker and SessionMint', 'AuthBroker and AuthBroker'),
    base.replace('with no owner and no serialization', 'with an owner and per-key serialization'),
    'Example: ' + base, '> ' + base, '```\n' + base,
    base.replace('How should shared-state access be structured?', 'Should the review report record this finding?'),
  ];
  for (const title of titles) {
    const c = actual(); c.questions[0]!.question = title;
    expect(engFirstReviewAUQ(answer(c))).toBe(false);
  }
  for (const header of ['Issue 2', 'Issue 1.2', 'TODOs', 'Setup', 'Next review']) {
    const c = actual(); c.questions[0]!.header = header;
    expect(engFirstReviewAUQ(answer(c))).toBe(false);
  }
});

test('opposed implementation choices cannot be replaced by report or workflow choices', () => {
  for (const labels of [
    ['Record in report', 'Defer the report', 'Keep the report'],
    ['Run Eng next', 'Run Design next', 'Keep reviewing manually'],
  ]) {
    const c = actual(); c.questions[0]!.options.forEach((o, i) => {o.label = labels[i]!;});
    expect(engFirstReviewAUQ(answer(c))).toBe(false);
  }
  const c = actual(); c.questions[0]!.options[0]!.description = '';
  expect(engFirstReviewAUQ(answer(c))).toBe(false);
});
});

describe('eng-first-review-t', () => {
const calls: NativePlanQuestionCall[] = JSON.parse(readFileSync(join(import.meta.dir, 'fixtures/eng-batching-t-calls.json'), 'utf8'));
const fresh = () => structuredClone(calls[0]!);
const first = (call: NativePlanQuestionCall) => engFirstReviewAUQ(nativePlanCallFingerprint(call, 0, true));
function question(call: NativePlanQuestionCall, text: string) {
  const q = call.questions[0]!; const answer = call.answers![q.question];
  call.answers = { [text]: answer! }; q.question = text;
}

describe('T Eng first architecture choice', () => {
  test('the actual first architecture issue starts review on this call', () => {
    const call = fresh(); const fp = nativePlanCallFingerprint(call, 0, true);
    expect(engSetupAUQ(fp)).toBe(false);
    expect(first(call)).toBe(true);
    expect(planCountQuestionPhase(fp, false, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ))
      .toEqual({ preReview: false, reviewStarted: true });
  });

  test('all five exact native decisions remain separate review calls', () => {
    let started = false;
    const phases = calls.map(call => {
      const fp = nativePlanCallFingerprint(call, 0, !started);
      const phase = planCountQuestionPhase(fp, started, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ);
      started = phase.reviewStarted; return phase.preReview;
    });
    expect(phases).toEqual([false, false, false, false, false]);
  });

  test('offered answer identity survives reorder and either alternative decision', () => {
    const call = fresh(); call.questions[0]!.options.reverse();
    expect(first(call)).toBe(true);
    for (const option of call.questions[0]!.options) {
      call.answers![call.questions[0]!.question] = option.label;
      expect(first(call)).toBe(true);
    }
  });

  test('requires one completed native question and exact offered answer', () => {
    const variants: Array<(c: NativePlanQuestionCall) => void> = [
      c => { c.answered = false; }, c => { c.failed = true; },
      c => { delete c.unansweredQuestionIndices; }, c => { c.unansweredQuestionIndices = [0]; },
      c => { c.answers = {}; }, c => { c.answers![c.questions[0]!.question] = 'Foreign answer'; },
      c => { c.questions.push(structuredClone(c.questions[0]!)); },
      c => { c.questions[0]!.multiSelect = true; },
      c => { c.questions[0]!.options[1]!.label = c.questions[0]!.options[0]!.label; },
    ];
    for (const change of variants) { const call = fresh(); change(call); expect(first(call)).toBe(false); }
    const fp = nativePlanCallFingerprint(fresh(), 0, true); fp.signature = 'foreign:identity';
    expect(engFirstReviewAUQ(fp)).toBe(false);
    delete fp.nativeCall; expect(engFirstReviewAUQ(fp)).toBe(false);
  });

  test('whole-plan approach, setup, missing identity and quoted examples cannot start review', () => {
    for (const header of ['Approach', 'Scope', 'Routing rules', 'Next review']) {
      const call = fresh(); call.questions[0]!.header = header; expect(first(call)).toBe(false);
    }
    const original = fresh().questions[0]!.question;
    for (const text of [
      original.replace('arch-retry-scheduler', 'arch-setup'), original.replace(/ <gstack-qid:[^>]+>/, ''),
      original.replace('Architecture: Custom retry scheduler', 'Approach: Which whole-plan direction'),
      '> ' + original, '```text\n' + original + '\n```', original + ' Should we start another review?',
    ]) { const call = fresh(); question(call, text); expect(first(call)).toBe(false); }
  });

  test('requires an affirmative existing defect, not a neutral or negated comparison', () => {
    for (const description of [
      'Both implementations are equally valid choices.',
      'Each worker gets its own copy. There is no DRY violation.',
      fresh().questions[0]!.options[2]!.description!.replace('acknowledged DRY violation', 'no DRY violation'),
      fresh().questions[0]!.options[2]!.description!.replace('Creates 5 divergence points', 'No longer creates 5 divergence points'),
      '```text\n' + fresh().questions[0]!.options[2]!.description + '\n```',
    ]) { const call = fresh(); call.questions[0]!.options[2]!.description = description; expect(first(call)).toBe(false); }
    const call = fresh(); call.questions[0]!.options[0]!.label = 'Run /office-hours';
    call.answers![call.questions[0]!.question] = 'Run /office-hours'; expect(first(call)).toBe(false);
  });
});
});

describe('eng-injected-export-aq', () => {
const fixture = fixture_eng_injected_export_aq;
const calls=()=>structuredClone(fixture.calls) as NativePlanQuestionCall[];
const first=()=>calls()[1]!;
const fp=(c=first())=>nativePlanCallFingerprint(c,0,true);
const classify=(c=first())=>engFirstReviewAUQ(fp(c));
function mutate(change:(c:NativePlanQuestionCall)=>void){const c=first();change(c);return c;}
function text(change:(s:string)=>string){return mutate(c=>{const q=c.questions[0]!,answer=c.answers![q.question]!;q.question=change(q.question);c.answers={[q.question]:answer};});}
describe('AQ current injected-export architecture decision',()=>{
 test('exact eight owned calls start review only at D2 and preserve scope first',()=>{
  let started=false;const rows=calls().map(c=>{const p=planCountQuestionPhase(fp(c),started,engStep0Boundary,engFirstReviewAUQ,engSetupAUQ);started=p.reviewStarted;return p;});
  expect(rows.map(r=>r.preReview)).toEqual([true,false,false,false,false,false,false,false]);
  expect(classify()).toBe(true);expect(engSetupAUQ(fp())).toBe(false);
  expect(calls().map(c=>classify(c))).toEqual([false,true,false,false,false,false,false,false]);
 });
 test('consistent named actors, cache, issue and decision numbers may vary',()=>{
  const c=first(),q=c.questions[0]!;
  const rename=(s:string)=>s.replaceAll('AuthCache','TokenStore').replaceAll('AuthBroker','LoginReader').replaceAll('SessionMint','SessionWriter').replace('D2 — Issue 1:','D8 — Issue 3:');
  q.question=rename(q.question);q.header='Architecture 3';for(const o of q.options){o.label=rename(o.label).replace(/^1/,'3');o.description=rename(o.description??'');}
  q.options.reverse();for(const o of q.options){c.answers={[q.question]:o.label};expect(classify(c)).toBe(true);}
 });
 test('the current global premise admits Today and both constructor actor orders',()=>{
  expect(classify(text(s=>s.replace('Right now the cache','Today the cache')))).toBe(true);
  expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description=c.questions[0]!.options[0]!.description!.replace('AuthBroker and SessionMint constructors','SessionMint and AuthBroker constructors');}))).toBe(true);
 });
 test('requires complete single-question native ownership and an offered answer',()=>{
  for(const change of [(c:NativePlanQuestionCall)=>{c.answered=false;},(c:NativePlanQuestionCall)=>{c.failed=true;},(c:NativePlanQuestionCall)=>{delete c.failed;},(c:NativePlanQuestionCall)=>{delete c.answeredAt;},(c:NativePlanQuestionCall)=>{c.answeredAt='not a date';},(c:NativePlanQuestionCall)=>{c.sessionId='';},(c:NativePlanQuestionCall)=>{c.toolUseId='';},(c:NativePlanQuestionCall)=>{c.answers={};},(c:NativePlanQuestionCall)=>{c.answers={[c.questions[0]!.question]:'unoffered'};},(c:NativePlanQuestionCall)=>{c.answers!['other']='other';},(c:NativePlanQuestionCall)=>{c.unansweredQuestionIndices=[0];},(c:NativePlanQuestionCall)=>{delete c.unansweredQuestionIndices;},(c:NativePlanQuestionCall)=>{c.questions.push(structuredClone(c.questions[0]!));},(c:NativePlanQuestionCall)=>{c.questions[0]!.multiSelect=true;}])expect(classify(mutate(change))).toBe(false);
  for(const f of [{...fp(),signature:'foreign:call'},{...fp(),nativeQuestionIndex:1},{...fp(),nativeCall:undefined},{...fp(),options:[...fp().options].reverse()}])expect(engFirstReviewAUQ(f)).toBe(false);
 });
 test('issue, header and option identities must match without malformed explicit numbers',()=>{
  for(const c of [text(s=>s.replace('Issue 1:','Issue 01:')),text(s=>s.replace('Issue 1:','Issue 0:')),text(s=>s.replace('Issue 1:','Issue 1.2:')),text(s=>s.replace('D2 —','D02 —')),mutate(c=>{c.questions[0]!.header='Arch 2';}),mutate(c=>{c.questions[0]!.header='Scope';}),mutate(c=>{c.questions[0]!.options[0]!.label='2A: Inject AuthCache (recommended)';c.answers={[c.questions[0]!.question]:c.questions[0]!.options[0]!.label};}),mutate(c=>{c.questions[0]!.options[2]!.label=c.questions[0]!.options[1]!.label;})])expect(classify(c)).toBe(false);
 });
 test('only one current metadata and assessment owner can supply the premise',()=>{
  for(const prefix of ['Source excerpt: ','Earlier review assessment: ','If approved, ','Provided this is approved, ','Historical example: '])expect(classify(text(s=>s.replace('ELI10: ','ELI10: '+prefix)))).toBe(false);
  for(const prefix of ['Source: ','Earlier review assessment: ','If approved, ','Provided this is approved, '])expect(classify(text(s=>s.replace('Project/branch/task: ','Project/branch/task: '+prefix)))).toBe(false);
  for(const line of ['Source excerpt:','Earlier review assessment:','Project/branch/task: a different current project','ELI10: Right now the cache is a global variable that two different services reach into and change.'])expect(classify(text(s=>s.replace('ELI10:',line+'\nELI10:')))).toBe(false);
  expect(classify(text(s=>s.replace(/^Project\/branch\/task:.*\n/m,'')))).toBe(false);
  expect(classify(text(s=>s.replace('a global variable that two different services reach into and change','no longer a global variable that two different services reach into and change')))).toBe(false);
 });
 test('same-owner withdrawn, superseded and quoted-status claims close the question',()=>{
  for(const status of ['withdrawn','superseded','resolved','rejected','cancelled','not current','"closed"','“superseded”'])for(const subject of ['This finding','This amendment','This assessment'])expect(classify(text(s=>s+`\n${subject} is ${status}.`))).toBe(false);
  expect(classify(text(s=>s+'\nThis remedy is a historical example, not the current option.'))).toBe(false);
  expect(classify(text(s=>s+'\n"Earlier review assessment: This finding is withdrawn."'))).toBe(true);
 });
 test('requires the named composition-root injection, removal and isolation test',()=>{
  for(const [from,to] of [['Construct one AuthCache','Construct one ForeignCache'],['AuthBroker and SessionMint constructors','AuthBroker and ForeignWriter constructors'],['AuthBroker and SessionMint constructors','AuthBroker and AuthBroker constructors'],['delete the module-level export','keep the module-level export'],['add a test that two service instances with separate caches never observe each other','tests can be added later']])expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description=c.questions[0]!.options[0]!.description!.replace(from,to);}))).toBe(false);
  for(const prefix of ['Source excerpt: ','If approved, ','Earlier review assessment: '])expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description=prefix+c.questions[0]!.options[0]!.description;}))).toBe(false);
  for(const suffix of [' This amendment is withdrawn.',' This remedy is "superseded".',' This option is not current.',' This remedy is a historical example, not the current option.'])expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description+=suffix;}))).toBe(false);
 });
 test('an actual opposed unchanged global and persistent risk are required',()=>{
  for(const [from,to] of [['Accept the shared global as-is.','Remove the shared global.'],['tenant leakage risk stays','tenant leakage risk is resolved']])expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description=c.questions[0]!.options[2]!.description!.replace(from,to);}))).toBe(false);
  for(const prefix of ['Source excerpt: ','If approved, ','Historical example: '])expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description=prefix+c.questions[0]!.options[2]!.description;}))).toBe(false);
  for(const suffix of [' This option is withdrawn.',' This deferral is "superseded".',' This unchanged risk is resolved.'])expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description+=suffix;}))).toBe(false);
  expect(classify(mutate(c=>{c.questions[0]!.options[2]!.label='1C: Start reviewing';}))).toBe(false);
 });
});


test('AQ direct premise and action withdrawals supersede the earlier positive clauses',()=>{
 expect(classify(text(s=>s.replace('Project/branch/task: main','Project/branch/task: Assuming approval, main')))).toBe(false);
 expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description+=' Correction: do not delete the module-level export.';}))).toBe(false);
 expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description+=' Correction: do not accept the shared global as-is.';}))).toBe(false);
 expect(classify(text(s=>s+' Correction: this cache no longer has a module-level mutable export.'))).toBe(false);
});
});

describe('eng-library-hooks-aq', () => {
const fixture = fixture_eng_library_hooks_aq;
const calls=()=>structuredClone(fixture.calls) as NativePlanQuestionCall[];
const first=()=>calls()[2]!;
const fp=(c=first())=>nativePlanCallFingerprint(c,0,true);
const classify=(c=first())=>engFirstReviewAUQ(fp(c));
function mutate(change:(c:NativePlanQuestionCall)=>void){const c=first();change(c);return c;}
function text(change:(s:string)=>string){return mutate(c=>{const q=c.questions[0]!,answer=c.answers![q.question]!;q.question=change(q.question);c.answers={[q.question]:answer};});}
describe('AQ library-hooks choice opens batching review on its current remedy',()=>{
 test('exact twelve owned calls preserve two setup calls and ten distinct later decisions',()=>{
  let started=false;const rows=calls().map(c=>{const p=planCountQuestionPhase(fp(c),started,engStep0Boundary,engFirstReviewAUQ,engSetupAUQ);started=p.reviewStarted;return p;});
  expect(rows.map(r=>r.preReview)).toEqual([true,true,...Array(10).fill(false)]);
  expect(calls().map(c=>classify(c))).toEqual([false,false,true,...Array(9).fill(false)]);
  expect(classify()).toBe(true);expect(engSetupAUQ(fp())).toBe(false);
 });
 test('issue numbers, option order, worker count and selected opposed choice can vary consistently',()=>{
  const c=first(),q=c.questions[0]!;q.question=q.question.replace('D3 — Architecture issue 1:','D9 — Architecture issue 4:').replaceAll('5 workers','7 workers').replace('Recommendation: 1A','Recommendation: 4A');q.header='Architecture 4';
  for(const o of q.options){o.label=o.label.replace(/^1/,'4');o.description=o.description?.replaceAll('5 copies','7 copies').replace('Five copies','Seven copies').replace('five times','seven times');}q.options.reverse();
  for(const o of q.options){c.answers={[q.question]:o.label};expect(classify(c)).toBe(true);}
 });
 test('a wholly quoted archive cannot displace the current owned assessment',()=>{
  expect(classify(text(s=>s+'\n"Earlier review assessment: This finding is withdrawn."'))).toBe(true);
  expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description+=' "Earlier review assessment: This remedy is withdrawn."';}))).toBe(true);
 });
 test('native answered-call and original menu ownership remain mandatory',()=>{
  for(const change of [(c:NativePlanQuestionCall)=>{c.answered=false;},(c:NativePlanQuestionCall)=>{c.failed=true;},(c:NativePlanQuestionCall)=>{delete c.failed;},(c:NativePlanQuestionCall)=>{delete c.answeredAt;},(c:NativePlanQuestionCall)=>{c.answeredAt='invalid';},(c:NativePlanQuestionCall)=>{c.sessionId='';},(c:NativePlanQuestionCall)=>{c.toolUseId='';},(c:NativePlanQuestionCall)=>{c.answers={};},(c:NativePlanQuestionCall)=>{c.answers={[c.questions[0]!.question]:'unoffered'};},(c:NativePlanQuestionCall)=>{c.answers!['other']='other';},(c:NativePlanQuestionCall)=>{c.unansweredQuestionIndices=[0];},(c:NativePlanQuestionCall)=>{delete c.unansweredQuestionIndices;},(c:NativePlanQuestionCall)=>{c.questions.push(structuredClone(c.questions[0]!));},(c:NativePlanQuestionCall)=>{c.questions[0]!.multiSelect=true;}])expect(classify(mutate(change))).toBe(false);
  for(const f of [{...fp(),signature:'foreign:call'},{...fp(),nativeQuestionIndex:1},{...fp(),nativeCall:undefined},{...fp(),options:[...fp().options].reverse()}])expect(engFirstReviewAUQ(f)).toBe(false);
 });
 test('explicit issue numbers, headers and action identities must agree',()=>{
  for(const c of [text(s=>s.replace('issue 1:','issue 01:')),text(s=>s.replace('issue 1:','issue 0:')),text(s=>s.replace('issue 1:','issue 1.2:')),text(s=>s.replace('D3 —','D03 —')),mutate(c=>{c.questions[0]!.header='Arch 2';}),mutate(c=>{c.questions[0]!.header='Scope';}),mutate(c=>{c.questions[0]!.options[0]!.label='2A: Library hooks + custom backoff fn (recommended)';c.answers={[c.questions[0]!.question]:c.questions[0]!.options[0]!.label};})])expect(classify(c)).toBe(false);
 });
 test('requires unique current context and a current custom-scheduling premise',()=>{
  for(const prefix of ['Source excerpt: ','Earlier review assessment: ','If approved, ','Provided approval, ','Assuming approval, ']){
   expect(classify(text(s=>s.replace('ELI10: ','ELI10: '+prefix)))).toBe(false);
   expect(classify(text(s=>s.replace('Project/branch/task: ','Project/branch/task: '+prefix)))).toBe(false);
  }
  for(const line of ['Source:','Earlier review assessment:','Project/branch/task: other current context','ELI10: The plan rebuilds retry scheduling by hand inside each of 5 workers.'])expect(classify(text(s=>s.replace('ELI10:',line+'\nELI10:')))).toBe(false);
  expect(classify(text(s=>s.replace(/^Project\/branch\/task:.*\n/m,'')))).toBe(false);
  expect(classify(text(s=>s.replace('The plan rebuilds retry scheduling','The plan no longer rebuilds retry scheduling')))).toBe(false);
 });
 test('direct or quoted current withdrawal closes each owning statement',()=>{
  for(const status of ['withdrawn','superseded','resolved','"closed"','“superseded”']){
   expect(classify(text(s=>s+` This finding is ${status}.`))).toBe(false);
   expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description+=` This remedy is ${status}.`;}))).toBe(false);
   expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description+=` This option is ${status}.`;}))).toBe(false);
  }
 });
 test('requires a concrete library-owned retry mechanism and an isolated backoff policy',()=>{
  for(const [from,to] of [['Attempt counting, crash safety, and dashboard visibility come from the library for free.','The library could be evaluated later.'],['The backoff curve lives in one exported function','The backoff curve stays duplicated per worker']])expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description=c.questions[0]!.options[0]!.description!.replace(from,to);}))).toBe(false);
  for(const prefix of ['Source excerpt: ','If approved, ','Earlier review assessment: '])expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description=prefix+c.questions[0]!.options[0]!.description;}))).toBe(false);
  expect(classify(mutate(c=>{c.questions[0]!.options[0]!.label='1A: Start reviewing';c.answers={[c.questions[0]!.question]:c.questions[0]!.options[0]!.label};}))).toBe(false);
 });
 test('unchanged scheduling must retain its current per-worker crash-safety risk',()=>{
  for(const [from,to] of [['Five copies of crash-unsafe scheduling logic','Two copies of crash-unsafe scheduling logic'],['crash-unsafe scheduling logic','crash-safe scheduling logic'],['each drifting independently','all maintained in one shared policy']])expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description=c.questions[0]!.options[2]!.description!.replace(from,to);}))).toBe(false);
  for(const prefix of ['Source excerpt: ','If approved, ','Historical example: '])expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description=prefix+c.questions[0]!.options[2]!.description;}))).toBe(false);
  expect(classify(mutate(c=>{c.questions[0]!.options[2]!.label='1C: Proceed to the next review';}))).toBe(false);
 });
 test('same-owner mechanism, backoff, crash risk and premise cannot contradict their earlier claim',()=>{
  expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description+='\nCorrection: the library will not own attempt counting or crash safety.';}))).toBe(false);
  expect(classify(mutate(c=>{c.questions[0]!.options[0]!.description+='\nCorrection: do not preserve the exported backoff function.';}))).toBe(false);
  expect(classify(mutate(c=>{c.questions[0]!.options[2]!.description+='\nCorrection: the unchanged per-worker scheduler is now crash-safe.';}))).toBe(false);
  expect(classify(text(s=>s+'\nCorrection: retry scheduling no longer runs inside each worker.'))).toBe(false);
 });
});

describe('AY scheduler choices retain the current gap and opposed native remedies', () => {
 const publicCall=(retry=false):NativePlanQuestionCall=>{
  // Minimal excerpts from the two public calls; no transcript/report corpus.
  const question=retry
   ? "D1 — Custom inline backoff scheduler vs the job library's built-in retry hooks\nProject/branch/task: main — background job retry framework (PLAN.md:6-8).\nELI10: The plan says (PLAN.md:6-8) to ignore it and hand-roll a scheduler inside each of the 5 workers, same shape as the library version."
   : "D3 — Issue 1: custom inline scheduler per worker, or the job library's retry hook with a custom curve?\nProject/branch/task: main, PLAN.md §Architecture — background job retry framework.\nELI10: The plan writes its own \"wait, then try again\" loop inside each of the 5 workers. If that process dies mid-wait, the retry is gone and nobody knows.";
  const options=retry?[
   {label:'1A) Library hooks + shared backoff fn (recommended)',description:"Register the library's retry hook in each worker, pass one shared pure backoffDelay(attempt) for the curve. Completeness 9/10."},
   {label:'1B) Custom scheduler as one shared module',description:'Roll your own, but once, with persisted retry state. You own a second job system.'},
   {label:'1C) Proceed as planned (inline in 5 workers)',description:'Keep the plan as written. Completeness 4/10. Retries die with the process; five copies drift.'},
  ]:[
   {label:'1A: Library hook + custom curve (recommended)',description:'✅ Retry state persisted by the library: survives worker crash, deploy, and restart (human: ~1 day / CC: ~20 min).'},
   {label:'1B: Custom inline scheduler as planned',description:'✅ No dependency on library hook semantics. ❌ Retry state lives in process memory: any crash mid-backoff silently drops the job; you rebuild max-attempts, dead-letter, and metrics by hand.'},
   {label:'1C: Hybrid: library hook, but custom scheduler for one worker',description:'Library persistence for 4 workers today; two retry systems remain.'},
  ];
  return {sessionId:'ay-public',toolUseId:retry?'retry':'first',questions:[{header:retry?'Architecture':'Arch 1',question,multiSelect:false,options}],
   answered:true,failed:false,unansweredQuestionIndices:[],answeredAt:'2026-09-11T03:22:05.503Z',answers:{[question]:options[0]!.label}};
 };
 const edit=(retry:boolean,change:(c:NativePlanQuestionCall)=>void)=>{
  const c=publicCall(retry);change(c);
  if(c.answers&&Object.keys(c.answers).length)c.answers={[c.questions[0]!.question]:c.questions[0]!.options[0]!.label};
  return c;
 };
 test('both public forms start review on the same answered native choice',()=>{
  for(const retry of [false,true]){
   const c=publicCall(retry),q=c.questions[0]!;
   for(const o of q.options){c.answers={[q.question]:o.label};expect(classify(c)).toBe(true);}
   expect(engSetupAUQ(fp(c))).toBe(false);
  }
 });
 test('worker counts and native option order may vary consistently',()=>{
  for(const retry of [false,true])expect(classify(edit(retry,c=>{
   const q=c.questions[0]!;q.question=q.question.replace('5 workers','7 workers');
   q.options=q.options.map(o=>({...o,label:o.label.replace('5 workers','7 workers'),description:o.description?.replace('five copies','seven copies')}));
   q.options.reverse();
  }))).toBe(true);
 });
 test('same-owner native completion, metadata and menu are mandatory',()=>{
  const bad:Array<(c:NativePlanQuestionCall)=>void>=[
   c=>{c.answered=false;},c=>{c.failed=true;},c=>{delete c.answeredAt;},c=>{c.answers={};},c=>{c.unansweredQuestionIndices=[0];},
   c=>{c.questions[0]!.header='Routing';},c=>{c.questions[0]!.options[0]!.label='2A: Library hook + custom curve';},
   c=>{c.questions[0]!.question='Source excerpt:\n'+c.questions[0]!.question;},
   c=>{c.questions[0]!.question=c.questions[0]!.question.replace('ELI10: ','ELI10: If approved, ');},
   c=>{c.questions[0]!.question=c.questions[0]!.question.replace('Project/branch/task: ','Project/branch/task: If approved, ');},
   c=>{c.questions[0]!.question=c.questions[0]!.question.replace('ELI10:','> ELI10:');},
   c=>{c.questions[0]!.question+='\nELI10: The plan writes its own loop inside each of the 5 workers.';},
   c=>{c.questions[0]!.question+='\nCorrection: retry scheduling no longer runs inside each worker.';},
  ];
  for(const retry of [false,true])for(const change of bad)expect(classify(edit(retry,change))).toBe(false);
  for(const retry of [false,true])expect(engFirstReviewAUQ({...fp(publicCall(retry)),signature:'foreign:call'})).toBe(false);
 });
 test('the proposed library mechanism cannot borrow from another native option',()=>{
  for(const retry of [false,true]){
   const keep=retry?2:1;
   for(const change of [
    (c:NativePlanQuestionCall)=>{[c.questions[0]!.options[0]!.description,c.questions[0]!.options[keep]!.description]=[c.questions[0]!.options[keep]!.description,c.questions[0]!.options[0]!.description];},
    (c:NativePlanQuestionCall)=>{c.questions[0]!.options[0]!.description='The library could be evaluated later.';},
    (c:NativePlanQuestionCall)=>{c.questions[0]!.options[0]!.description='Source excerpt: '+c.questions[0]!.options[0]!.description;},
    (c:NativePlanQuestionCall)=>{c.questions[0]!.options[0]!.description='"'+c.questions[0]!.options[0]!.description+'"';},
    (c:NativePlanQuestionCall)=>{c.questions[0]!.options[0]!.description+='\nThe library will not own persistence.';},
    (c:NativePlanQuestionCall)=>{c.questions[0]!.options[keep]!.description='Keep the current design; no retries are lost.';},
    (c:NativePlanQuestionCall)=>{c.questions[0]!.options[keep]!.description='If approved, '+c.questions[0]!.options[keep]!.description;},
    (c:NativePlanQuestionCall)=>{c.questions[0]!.options[keep]!.description+='\nThe scheduler is now crash-safe.';},
   ])expect(classify(edit(retry,change))).toBe(false);
  }
  expect(classify(edit(false,c=>{c.questions[0]!.options[0]!.description=c.questions[0]!.options[0]!.description!.replace('survives worker','never survives worker');}))).toBe(false);
  expect(classify(edit(false,c=>{c.questions[0]!.options[1]!.description=c.questions[0]!.options[1]!.description!.replace('silently drops','never drops');}))).toBe(false);
  expect(classify(edit(true,c=>{c.questions[0]!.options[2]!.description=c.questions[0]!.options[2]!.description!.replace('five copies','two copies');}))).toBe(false);
 });
 test('quoted owner status and approval conditions remain current after matched outcomes',()=>{
  for(const retry of [false,true])for(const target of ['question','remedy','unchanged']){
   for(const status of ["This finding is 'withdrawn'.",'This option is conditional on approval.','If approved, proceed with this option.']){
    expect(classify(edit(retry,c=>{
     const q=c.questions[0]!;
     if(target==='question')q.question+='\n'+status;
     else q.options[target==='remedy'?0:retry?2:1]!.description+='\n'+status;
    }))).toBe(false);
   }
  }
 });
 test('approval clauses remain binding after option tradeoffs',()=>{
  for(const retry of [false,true])for(const option of [0,retry?2:1]){
   for(const clause of ['Assuming approval, proceed with this option.','Provided approval, keep this option.']){
    const add=(c:NativePlanQuestionCall,quoted=false)=>{c.questions[0]!.options[option]!.description+=' ❌ Additional integration effort.\n'+(quoted?'"Earlier assessment: '+clause+'"':clause);};
    expect(classify(edit(retry,c=>add(c)))).toBe(false);
    expect(classify(edit(retry,c=>add(c,true)))).toBe(true);
   }
  }
 });
 test('a crash premise cannot erase an owned approval condition',()=>{
  for(const retry of [false,true]){
   expect(classify(edit(retry,c=>{c.questions[0]!.question+='\nIf that process dies, this finding applies only if approved.';}))).toBe(false);
   expect(classify(edit(retry,c=>{c.questions[0]!.question+='\n"Earlier assessment: If that process dies, this finding applies only if approved."';}))).toBe(true);
   expect(classify(edit(retry,c=>{
    const q=c.questions[0]!,consequence='If the worker process crashes, the retry is gone and nobody knows.';
    q.question=retry?q.question+'\n'+consequence:q.question.replace('If that process dies mid-wait, the retry is gone and nobody knows.',consequence);
   }))).toBe(true);
  }
 });
});
});

describe('eng-scope-y', () => {
const captured = captured_eng_scope_y;
const fresh = () => structuredClone(captured[1]!) as NativePlanQuestionCall;
const fp = (c: NativePlanQuestionCall) => nativePlanCallFingerprint(c, 0, false);
const setup = (c: NativePlanQuestionCall) => engSetupAUQ(fp(c));
function question(c: NativePlanQuestionCall, transform: (s: string) => string) {
  const q = c.questions[0]!; const answer = c.answers![q.question]!;
  q.question = transform(q.question); c.answers = {[q.question]: answer}; return c;
}

describe('Y whole-plan complexity setup decision', () => {
  test('the actual accepted-complexity decision remains setup after the review boundary', () => {
    expect(setup(fresh())).toBe(true);
    expect(planCountQuestionPhase(fp(fresh()), true, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ))
      .toEqual({preReview: true, reviewStarted: true});
  });

  test('all seven substantive approvals and TODO obligations stay counted', () => {
    let started = false;
    const phases = captured.map(c => {
      const call = structuredClone(c) as NativePlanQuestionCall;
      const phase = planCountQuestionPhase(fp(call), started, engStep0Boundary, engFirstReviewAUQ, engSetupAUQ);
      started = phase.reviewStarted; return phase.preReview;
    });
    expect(phases).toEqual([true, true, true, false, false, false, false, false, false, false]);
    expect(captured[8]!.questions[0]!.header).toBe('TODO: Diagrams');
    expect(captured[9]!.questions[0]!.header).toBe('TODO: Policy');
  });

  test('either offered scope decision and reordered options remain setup', () => {
    const c = fresh(); c.questions[0]!.options.reverse();
    for (const option of c.questions[0]!.options) {
      c.answers = {[c.questions[0]!.question]: option.label}; expect(setup(c)).toBe(true);
    }
    const varied = question(fresh(), s => s.replace('4 new classes across 12 files', '6 new classes across 20 files'));
    varied.questions[0]!.options[0]!.description = varied.questions[0]!.options[0]!.description.replace('4 classes across 12 files', '6 classes across 20 files');
    expect(setup(varied)).toBe(true);
  });

  test('component remedies, unfinished or conditional scope and additional work do not enter the new arm', () => {
    for (const transform of [
      (s: string) => s.replace('This plan introduces', 'If this plan introduces'),
      (s: string) => s.replace('This plan introduces', 'This component introduces'),
      (s: string) => s.replace('This plan introduces', 'This plan does not introduce'),
      (s: string) => s.replace('Recommend scope reduction before reviewing, or accept the complexity and review as-is?', 'Fix the global cache race before reviewing?'),
      (s: string) => s.replace('review as-is?', 'review as-is? Also approve the cache repair.'),
      (s: string) => s.replace('4 new classes', '0 new classes'),
      (s: string) => s.replace('plan-eng-review-scope-challenge', 'plan-eng-review-arch-shared-cache'),
      (s: string) => s.replace('plan-eng-review-scope-challenge', 'foreign-scope-challenge'),
      (s: string) => s + ' <gstack-qid:plan-eng-review-scope-challenge>',
      (s: string) => '> ' + s,
      (s: string) => '```text\n' + s + '\n```',
    ]) expect(setup(question(fresh(), transform))).toBe(false);
    for (const index of [0, 1]) {
      const c = fresh(); c.questions[0]!.options[index]!.description += ' Also implement the missing cache invalidation guard.';
      expect(setup(c)).toBe(false);
    }
    const mismatched = fresh(); mismatched.questions[0]!.options[0]!.description = mismatched.questions[0]!.options[0]!.description.replace('12 files', '99 files');
    expect(setup(mismatched)).toBe(false);
    for (const c of captured.slice(3)) expect(setup(structuredClone(c) as NativePlanQuestionCall)).toBe(false);
  });

  test('only a matched complete native answer to the closed two-option menu qualifies', () => {
    for (const mutate of [
      (c: NativePlanQuestionCall) => {c.answered = false;},
      (c: NativePlanQuestionCall) => {c.failed = true;},
      (c: NativePlanQuestionCall) => {delete c.failed;},
      (c: NativePlanQuestionCall) => {delete c.unansweredQuestionIndices;},
      (c: NativePlanQuestionCall) => {c.unansweredQuestionIndices = [0];},
      (c: NativePlanQuestionCall) => {c.questions[0]!.multiSelect = true;},
      (c: NativePlanQuestionCall) => {c.questions[0]!.header = 'Architecture';},
      (c: NativePlanQuestionCall) => {c.questions.push(structuredClone(c.questions[0]!));},
      (c: NativePlanQuestionCall) => {c.questions[0]!.options.push(structuredClone(c.questions[0]!.options[0]!));},
      (c: NativePlanQuestionCall) => {c.answers = {[c.questions[0]!.question]: 'unoffered scope decision'};},
    ]) {const c = fresh(); mutate(c); expect(setup(c)).toBe(false);}
    expect(engSetupAUQ({...fp(fresh()), signature: 'foreign:call'})).toBe(false);
    expect(engSetupAUQ({...fp(fresh()), nativeCall: undefined})).toBe(false);
    expect(engSetupAUQ({...fp(fresh()), options: []})).toBe(false);
  });
});
});
