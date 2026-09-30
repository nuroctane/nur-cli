import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { findNativeAutoDecision } from './helpers/native-auto-decide';
import { classifyVisible } from './helpers/claude-pty-runner';
import { readPlanCountTranscript, type NativePublicToolEvent } from './helpers/plan-count-transcript';
import capture_auto_decide_current_declaration from './fixtures/auto-decide-current-declaration-6aef.json';
import capture_auto_decide_explanatory_mode from './fixtures/auto-decide-explanatory-mode-043a.json';
import captured749_auto_decide_explanatory_mode from './fixtures/auto-decide-explanatory-mode-749df.json';
import annotations_auto_decide_explanatory_mode from './fixtures/native-auto-decide-ag.json';
import capture_auto_decide_recommendation_scope from './fixtures/auto-decide-recommendation-361c.json';
import captured_auto_decide_saved_ai from './fixtures/auto-decide-saved-ai.json';
import retry_auto_decide_saved_ai from './fixtures/auto-decide-retry-ai.json';
import capture_auto_decide_structured from './fixtures/auto-decide-structured-77.json';
import completedModeCapture_auto_decide_structured from './fixtures/auto-decide-completed-mode-f359.json';
import capture_auto_decide_target_identity from './fixtures/auto-decide-target-361c.json';
import { bindAutoDecisionState } from './helpers/auto-decision-state';
import capture_auto_decision_state from './fixtures/auto-decide-state-cab3.json';
import { describe } from 'bun:test';
const captured = JSON.parse(fs.readFileSync(path.join(import.meta.dir,'fixtures/native-auto-decide-ag.json'),'utf8'));
const clone = (i=0) => structuredClone(captured.attempts[i]);
const verdict = (f=clone()) => findNativeAutoDecision(f.transcript,f.tools,f.options);
const ownedMessage = (f:any) => f.transcript.assistantMessages.find((m:any)=>m.sessionId===f.options.sessionId&&m.text.includes('Auto-decided'));
const literal = 'Auto-decided review mode → **HOLD SCOPE** (your preference). Change with /plan-tune.';

test('both exact native attempts preserve the asserted preference annotation despite terminal damage',()=>{
  for(const [i,attempt] of captured.attempts.entries()){
    const actual=verdict(attempt);expect(actual).not.toBeNull();expect(actual!.sessionId).toBe(attempt.options.sessionId);
    expect(actual!.option).toBe('HOLD SCOPE');expect(actual!.annotation).toContain('(your preference). Change with /plan-tune.');
    expect(ownedMessage(attempt).text).toContain(actual!.annotation);
    if(i===1)expect(attempt.transcript.assistantMessages.some((m:any)=>m.sessionId!==attempt.options.sessionId&&m.text.includes('Auto-decided'))).toBe(true);
  }
  expect(classifyVisible('Auto-dcided review mode: HOLD SCOPE. DONE.')).toBeNull();
});

test('native annotation needs one successful post-command load in the owned session',()=>{
  const mutations:Array<(f:any)=>void>=[
    f=>{f.transcript.status='missing';},f=>{f.transcript.status='error';},f=>{f.options.sessionId='foreign';},
    f=>{f.options.commandStartedAt=f.options.now;},f=>{f.options.commandStartedAt=NaN;},f=>{f.options.now=Infinity;},
    f=>{f.options.now=f.options.commandStartedAt-1;},f=>{f.options.skillName='other-skill';},
    f=>{const use=f.tools.find((e:any)=>e.kind==='use'&&e.name==='Skill');use.name='Read';},
    f=>{const use=f.tools.find((e:any)=>e.kind==='use'&&e.name==='Skill');f.tools.push({...use});},
    f=>{const use=f.tools.find((e:any)=>e.kind==='use'&&e.name==='Skill');f.tools=f.tools.filter((e:any)=>e.kind!=='result'||e.toolUseId!==use.toolUseId);},
    f=>{const use=f.tools.find((e:any)=>e.kind==='use'&&e.name==='Skill');f.tools.find((e:any)=>e.kind==='result'&&e.toolUseId===use.toolUseId).isError=true;},
    f=>{const use=f.tools.find((e:any)=>e.kind==='use'&&e.name==='Skill');const result=f.tools.find((e:any)=>e.kind==='result'&&e.toolUseId===use.toolUseId);f.tools.push({...result});},
    f=>{const use=f.tools.find((e:any)=>e.kind==='use'&&e.name==='Skill');f.tools.find((e:any)=>e.kind==='result'&&e.toolUseId===use.toolUseId).timestamp=new Date(f.options.commandStartedAt-1).toISOString();},
    f=>{ownedMessage(f).timestamp='invalid';},f=>{ownedMessage(f).timestamp=new Date(f.options.now+1).toISOString();},
    f=>{ownedMessage(f).timestamp=new Date(f.options.commandStartedAt-1).toISOString();},
    f=>{f.transcript.calls.push({sessionId:f.options.sessionId,toolUseId:'asked',questions:[{header:'Mode',question:'Choose mode?',options:[{label:'A'},{label:'B'}]}],answered:false});},
  ];
  for(const mutate of mutations){const f=clone();mutate(f);expect(verdict(f)).toBeNull();}
  const f=clone();f.tools.find((e:any)=>e.kind==='use'&&e.name==='Skill').input.skill='gstack:plan-ceo-review';expect(verdict(f)).not.toBeNull();
});

test('quotes, source introductions, conditions and incomplete template prose are not annotations',()=>{
  for(const text of [
    `> ${literal}`,`    ${literal}`,`\t${literal}`,`"${literal}"`,`\`\`\`text\n${literal}\n\`\`\``,
    `Example:\n\n${literal}`,`An unproven hypothesis.\n\n${literal}`,`Previous transcript:\n\n${literal}`,
    `If approved, ${literal}`,literal.replace('(your preference)','(if you approve)'),literal.replace('Auto-decided','Auto-dcided'),
    literal.replace('Change with /plan-tune.',''),`**Review mode: EXPANSION.**\n\n${literal}`,
    `Example:\n\n**Review mode: HOLD SCOPE.**\n\n${literal}`,
    'Auto-decided <summary> → <option> (your preference). Change with /plan-tune.',
    'Auto-decided review mode → <selected option> (your preference). Change with /plan-tune.',
    `**Review mode: HOLD SCOPE.**\n\n    ${literal}`,`**Review mode: HOLD SCOPE.**\n\n\t${literal}`,
  ]){const f=clone();ownedMessage(f).text=text;expect(verdict(f),text).toBeNull();}
  const f=clone(1);ownedMessage(f).text=ownedMessage(f).text.replace('Heads-up from gstack: this branch has unshipped work.', 'Heads-up from gstack: the following is a hypothetical example.');expect(verdict(f)).toBeNull();
});

test('same-session current retractions override an earlier annotation without borrowing quoted examples',()=>{
  for(const text of [
    'Correction: I withdraw this auto-decision.',
    'The annotation is only an example.',
    'This decision is withdrawn.',
    'Correction: "I retract this decision."',
    'I did not auto-decide the review mode.',
    'Correction: that statement was only an example.',
    'I did not make this choice.',
    '**Review mode: SCOPE EXPANSION.**',
  ]){
    const f=clone();ownedMessage(f).text+='\n\n'+text;expect(verdict(f),text).toBeNull();
    const g=clone();g.options.now++;g.transcript.assistantMessages.push({sessionId:g.options.sessionId,timestamp:new Date(g.options.now).toISOString(),text});expect(verdict(g),text).toBeNull();
  }
  for(const text of ['> I withdraw this decision.','Quoted example: "I withdraw this decision."','```text\nI withdraw this decision.\n```','Example: "I did not make this choice."','Example: "that statement was only an example."']){
    const f=clone();ownedMessage(f).text+='\n\n'+text;expect(verdict(f),text).not.toBeNull();
  }
  const f=clone();f.transcript.assistantMessages.push({sessionId:'foreign',timestamp:new Date(f.options.now).toISOString(),text:'I withdraw this decision.'});expect(verdict(f)).not.toBeNull();
});

test('the native reader cannot promote foreign cwd, child, user or tool-result text to an annotation',()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'auto-native-'));
  try {
    const config=path.join(dir,'config'),journals=path.join(config,'projects','fixture');fs.mkdirSync(journals,{recursive:true});
    const start=Date.now()-100,session='owned';
    const make=()=>[
      {type:'assistant',isSidechain:false,cwd:dir,sessionId:session,timestamp:new Date(start+1).toISOString(),message:{role:'assistant',content:[{type:'tool_use',id:'load',name:'Skill',input:{skill:'plan-ceo-review'}}]}},
      {type:'user',isSidechain:false,cwd:dir,sessionId:session,timestamp:new Date(start+2).toISOString(),message:{role:'user',content:[{type:'tool_result',tool_use_id:'load',content:'loaded',is_error:false}]}},
      {type:'assistant',isSidechain:false,cwd:dir,sessionId:session,timestamp:new Date(start+3).toISOString(),message:{role:'assistant',content:[{type:'text',text:literal}]}},
    ];
    const cases:Array<[(rows:any[])=>void,boolean]>=[
      [()=>{},true],[rows=>{for(const r of rows)r.cwd=dir+'-foreign';},false],
      [rows=>{rows[2].isSidechain=true;},false],[rows=>{rows[2].message.role='user';},false],
      [rows=>{rows[2].message.content=[{type:'tool_result',tool_use_id:'other',content:literal}];},false],
    ];
    for(const [mutate,expected]of cases){const rows=make();mutate(rows);fs.writeFileSync(path.join(journals,session+'.jsonl'),rows.map(r=>JSON.stringify(r)).join('\n')+'\n');
      const tools:NativePublicToolEvent[]=[];const transcript=readPlanCountTranscript(config,dir,e=>tools.push(e));
      expect(Boolean(findNativeAutoDecision(transcript,tools,{sessionId:session,skillName:'plan-ceo-review',commandStartedAt:start,now:Date.now()}))).toBe(expected);
    }
  }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
describe('auto-decide-current-declaration', () => {
const capture = capture_auto_decide_current_declaration;
const clone = () => structuredClone(capture.retry) as any;
const decide = (f: any) => findNativeAutoDecision(f.transcript, f.tools, f.options);
const message = (f: any) => f.transcript.assistantMessages.find((m: any) =>
  m.timestamp === '2026-09-16T23:23:28.931Z');
const use = (f: any) => f.tools.find((e: any) => e.kind === 'use' &&
  e.input?.command?.includes('gstack-skill-start'));
const ack = (f: any) => f.tools.find((e: any) => e.kind === 'result' && e.toolUseId === use(f).toolUseId);

test('actual current Decision declaration completes the retained owned retry', () => {
  const f = clone();
  const result = decide(f);
  expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.annotation).toBe(message(f).text);
  expect(result?.stateRecord).toEqual(f.options.stateEvidence.records[0]);
  expect(result?.preambleToolUseId).toBe(use(f).toolUseId);
  expect(result?.skillToolUseId).toBeUndefined();
  expect(result?.questionLogToolUseId).toBeUndefined();
});

test('literal first declaration form is supported by the authenticated retry context', () => {
  const f = clone();
  // This checks representation only. The first attempt's state was deleted;
  // transplanting its text grants no first-attempt ownership or verdict credit.
  message(f).text = capture.firstDeclaration.text;
  expect(decide(f)?.option).toBe('HOLD SCOPE');
  delete f.options.stateEvidence;
  f.tools = [];
  expect(decide(f)).toBeNull();
});

const modes = ['HOLD SCOPE', 'SCOPE EXPANSION', 'SELECTIVE EXPANSION', 'SCOPE REDUCTION'];
for (const mode of modes) {
  for (const label of ['Decision', 'Decision: review mode is', 'Mode']) {
    for (const target of ['for this draft.', 'for the current review (saved preference).']) {
      const text = `${label}${label.includes(':') ? '' : ':'} ${mode} ${target}`;
      test(`one current full mode owns its target clause: ${text}`, () => {
        const f = clone();
        Object.assign(f.options.stateEvidence.records[0], { user_choice: mode, recommended: mode });
        message(f).text = text;
        expect(decide(f)?.option).toBe(mode);
      });
    }
  }
}

const invalidDeclarations = [
  'Decision: HOLD SCOPELESS for this draft.',
  'Decision: HOLD for this draft.',
  'Decision: SCOPE for this draft.',
  'Decision: SELECTIVE for this draft.',
  'Decision: review mode is CUSTOM MODE for this draft.',
  'Decision: HOLD SCOPE?',
  'Decision: HOLD SCOPE for',
  'Decision: HOLD SCOPE for (',
  'Decision: HOLD SCOPE for this draft (unfinished.',
  'Decision: HOLD SCOPE for this draft (unbalanced)).',
  'Decision: HOLD SCOPE for this draft or SCOPE EXPANSION.',
  'Decision: HOLD SCOPE for this draft. Instead choose SCOPE REDUCTION.',
  'Decision: HOLD SCOPE for this draft; SELECTIVE_EXPANSION.',
  'Decision: HOLD SCOPE for this draft, if approved.',
  'Decision: HOLD SCOPE for this draft, pending approval.',
  'Decision: HOLD SCOPE for this draft, not yet selected.',
  'Decision: HOLD SCOPE for this draft, withdrawn.',
  'Decision: HOLD SCOPE for this draft; the selected mode is not HOLD SCOPE.',
  'Decision: HOLD SCOPE for plan-eng-review.',
  'Decision: HOLD SCOPE for another draft.',
  'Decision: HOLD SCOPE for a future review.',
  'Decision: HOLD SCOPE for this future review.',
  'Decision pending: HOLD SCOPE for this draft.',
  'Decision: review mode is not selected.',
  'Decision: not HOLD SCOPE for this draft.',
];
for (const text of invalidDeclarations) {
  test(`unsupported current declaration cannot complete a decision: ${text}`, () => {
    const f = clone(); message(f).text = text;
    expect(decide(f)).toBeNull();
  });
  test(`unsupported current declaration retracts the earlier decision: ${text}`, () => {
    const f = clone(); message(f).text += `\n\nCorrection: ${text}`;
    expect(decide(f)).toBeNull();
  });
}

for (const prefix of ['> ', '    ', '"', '`']) {
  test(`quoted current-mode syntax does not declare or retract: ${JSON.stringify(prefix)}`, () => {
    const f = clone();
    const quote = (value: string) => prefix + value + (['"', '`'].includes(prefix) ? prefix : '');
    message(f).text = quote('Decision: HOLD SCOPE for this draft.');
    expect(decide(f)).toBeNull();
    message(f).text = clone().transcript.assistantMessages.at(-1).text + '\n\n' +
      quote('Decision: SCOPE EXPANSION for this draft.');
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
}
for (const text of [
  'Example:\nDecision: HOLD SCOPE for this draft.',
  'Historical transcript:\nDecision: HOLD SCOPE for this draft.',
  'Previous decision:\nDecision: HOLD SCOPE for this draft.',
  '```text\nDecision: HOLD SCOPE for this draft.\n```',
  'If approved, Decision: HOLD SCOPE for this draft.',
  'Not a decision: HOLD SCOPE for this draft.',
]) test(`unasserted declaration provides no mode: ${JSON.stringify(text)}`, () => {
  const f = clone(); message(f).text = text;
  expect(decide(f)).toBeNull();
});

for (const label of ['Decision', 'Decision: review mode is', 'Mode']) {
  test(`later matching current declaration retains the owned mode: ${label}`, () => {
    const f = clone(); message(f).text += `\n\nUpdate: ${label}${label.includes(':') ? '' : ':'} HOLD SCOPE for this draft.`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
  test(`later conflicting current declaration retracts the owned mode: ${label}`, () => {
    const f = clone(); message(f).text += `\n\nUpdate: ${label}${label.includes(':') ? '' : ':'} SCOPE EXPANSION for this draft.`;
    expect(decide(f)).toBeNull();
  });
}

test('a separately scoped non-mode decision does not retract the review mode', () => {
  const f = clone(); message(f).text += '\n\nDecision: publish the audit log.';
  expect(decide(f)?.option).toBe('HOLD SCOPE');
});

test('the completed native preference and log ACK retain their independent authority', () => {
  const f = clone(); delete f.options.stateEvidence;
  const result = decide(f);
  expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.stateRecord).toBeUndefined();
  expect(result?.preferenceToolUseId).toBeDefined();
  expect(result?.questionLogToolUseId).toBeDefined();
});

test('target-clause capitalization and a negative non-mode explanation remain valid', () => {
  const f = clone(); message(f).text = 'Decision: HOLD SCOPE For this draft, not for implementation.';
  expect(decide(f)?.option).toBe('HOLD SCOPE');
});

test('a named target must match the complete audit target, including dotted identifiers', () => {
  const f = clone();
  f.options.stateEvidence.records[0].question_summary = 'Select review mode for PLAN.md';
  message(f).text = 'Decision: HOLD SCOPE for PLAN.md.';
  expect(decide(f)?.option).toBe('HOLD SCOPE');
  message(f).text = 'Decision: HOLD SCOPE for PLAN.other.';
  expect(decide(f)).toBeNull();
});

for (const target of ['a future review', 'the previous review', 'another draft', 'the next invocation', 'an example plan']) {
  test(`even a matching audit cannot make an explicitly noncurrent target current: ${target}`, () => {
    const f = clone();
    f.options.stateEvidence.records[0].question_summary = `Select review mode for ${target}`;
    message(f).text = `Decision: HOLD SCOPE for ${target}.`;
    expect(decide(f)).toBeNull();
  });
}
for (const target of ['future.md', 'previous-review.md', 'another.plan.md']) {
  test(`owned literal filename remains a current target: ${target}`, () => {
    const f = clone();
    f.options.stateEvidence.records[0].question_summary = `Select review mode for ${target}`;
    message(f).text = `Decision: HOLD SCOPE for ${target}.`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
}

for (const [name, mutate] of Object.entries({
  'missing state and native log ACK': (f: any) => {
    delete f.options.stateEvidence;
    const log = f.tools.find((e: any) => e.kind === 'use' && e.input?.command?.includes('gstack-question-log'));
    f.tools = f.tools.filter((e: any) => e.kind !== 'result' || e.toolUseId !== log.toolUseId);
  },
  'empty owned log': (f: any) => { f.options.stateEvidence.records = []; },
  'duplicate owned log': (f: any) => { f.options.stateEvidence.records.push({ ...f.options.stateEvidence.records[0] }); },
  'wrong preference and native check': (f: any) => {
    f.options.stateEvidence.preference = 'always-ask';
    const check = f.tools.find((e: any) => e.kind === 'use' && e.input?.command?.includes('gstack-question-preference'));
    f.tools.find((e: any) => e.kind === 'result' && e.toolUseId === check.toolUseId).content = 'ASK\nEXIT: 0';
  },
  'foreign log session': (f: any) => { f.options.stateEvidence.records[0].session_id = 'foreign'; },
  'foreign log skill': (f: any) => { f.options.stateEvidence.records[0].skill = 'plan-eng-review'; },
  'different logged choice': (f: any) => { f.options.stateEvidence.records[0].user_choice = 'SCOPE EXPANSION'; },
  'different recommendation': (f: any) => { f.options.stateEvidence.records[0].recommended = 'SCOPE EXPANSION'; },
  'nonautomatic log': (f: any) => { f.options.stateEvidence.records[0].auto_decided = false; },
  'foreign native session': (f: any) => { f.options.sessionId = 'foreign'; },
  'failed preamble': (f: any) => { ack(f).isError = true; },
  'missing preamble ACK': (f: any) => { f.tools = f.tools.filter((e: any) => e !== ack(f)); },
  'duplicate preamble ACK': (f: any) => { f.tools.push({ ...ack(f) }); },
  'disabled tuning': (f: any) => { ack(f).content = ack(f).content.replace('QUESTION_TUNING: true', 'QUESTION_TUNING: false'); },
  'old log': (f: any) => { f.options.stateEvidence.records[0].ts = new Date(f.options.commandStartedAt - 1).toISOString(); },
  'future log': (f: any) => { f.options.stateEvidence.records[0].ts = new Date(f.options.now + 1).toISOString(); },
  'public decision before log': (f: any) => { message(f).timestamp = new Date(Date.parse(f.options.stateEvidence.records[0].ts) - 1).toISOString(); },
  'native question': (f: any) => { f.transcript.calls.push({ sessionId: f.options.sessionId }); },
  'prose question': (f: any) => { f.options.proseQuestionObserved = true; },
  'current withdrawal': (f: any) => { message(f).text += '\n\nI withdraw this decision.'; },
})) test(`current Decision syntax retains ${name} rejection`, () => {
  const f = clone(); mutate(f); expect(decide(f)).toBeNull();
});
});

describe('auto-decide-explanatory-mode', () => {
const capture = capture_auto_decide_explanatory_mode;
const captured749 = captured749_auto_decide_explanatory_mode;
const annotations = annotations_auto_decide_explanatory_mode;
const clone = () => structuredClone(capture) as any;
const declaration = (f: any) => f.transcript.assistantMessages.find((m: any) =>
  m.text.startsWith('**Review mode: HOLD SCOPE** —'));
const decide = (f: any) => findNativeAutoDecision(f.transcript, f.tools, f.options);

function witnessed() {
  const f = clone();
  const use = f.tools.find((e: any) => e.input?.command?.includes('gstack-question-log'));
  // Synthetic owned-file witness from the exact literal request. The original
  // file was not retained; its failed paid attempt remains failed.
  const record = JSON.parse(/gstack-question-log '(\{[^\n]*\})'/.exec(use.input.command)![1]!);
  record.source = 'agent';
  record.ts = f.tools.find((e: any) => e.kind === 'result' && e.toolUseId === use.toolUseId).timestamp;
  f.options.stateEvidence = { questionId: 'plan-ceo-review-mode', preference: 'never-ask', records: [record] };
  return f;
}

test('original public events alone cannot authenticate the unretained owned append', () => {
  expect(decide(clone())).toBeNull();
});

test('exact completed announcement agrees with an authenticated owned append', () => {
  const f = witnessed();
  const result = decide(f);
  expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.annotation).toBe(declaration(f).text);
  expect(result?.stateRecord).toEqual(f.options.stateEvidence.records[0]);
  expect(result?.questionLogToolUseId).toBeUndefined();
});

const separators = ['. ', ', ', '; ', ': ', ' — ', ' – ', ' - '];
for (const separator of separators) {
  test(`complete mode with separated explanation ${JSON.stringify(separator)}`, () => {
    const f = witnessed();
    declaration(f).text = `Review mode: HOLD SCOPE${separator}selected from the saved preference.`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
  test(`same later completed mode retains its explanation ${JSON.stringify(separator)}`, () => {
    const f = witnessed();
    declaration(f).text += `\n\nMode decision completed: HOLD SCOPE${separator}selected from the saved preference.`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
  test(`different later completed mode withdraws the choice ${JSON.stringify(separator)}`, () => {
    const f = witnessed();
    declaration(f).text += `\n\nCorrection: Mode: SCOPE EXPANSION${separator}selected from the saved preference.`;
    expect(decide(f)).toBeNull();
  });
  test(`conditional explanation never completes the mode ${JSON.stringify(separator)}`, () => {
    const f = witnessed();
    declaration(f).text = `Review mode: HOLD SCOPE${separator}if approved.`;
    expect(decide(f)).toBeNull();
  });
  test(`later conditional explanation withdraws the choice ${JSON.stringify(separator)}`, () => {
    const f = witnessed();
    declaration(f).text += `\n\nMode: HOLD SCOPE${separator}pending approval.`;
    expect(decide(f)).toBeNull();
  });
}

for (const value of ['HOLD SCOPELESS', 'HOLD SCOPE SCOPE EXPANSION', 'HOLD SCOPE / SCOPE EXPANSION',
  'HOLD SCOPE?', 'HOLD SCOPE selected from my preference', 'HOLD SCOPE—if approved', 'HOLD SCOPE - ']) {
  test(`incomplete or ambiguous mode is not a declaration: ${value}`, () => {
    const f = witnessed(); declaration(f).text = `Mode: ${value}`;
    expect(decide(f)).toBeNull();
  });
  test(`incomplete current field retracts a previous mode: ${value}`, () => {
    const f = witnessed(); declaration(f).text += `\n\nMode: ${value}`;
    expect(decide(f)).toBeNull();
  });
}

for (const [name, mutate] of Object.entries({
  'missing append': (f: any) => { f.options.stateEvidence.records = []; },
  'foreign session': (f: any) => { f.options.stateEvidence.records[0].session_id = 'foreign'; },
  'different logged mode': (f: any) => { f.options.stateEvidence.records[0].user_choice = 'SCOPE EXPANSION'; },
  'native question': (f: any) => { f.transcript.calls.push({ sessionId: f.options.sessionId }); },
  'unfinished declaration': (f: any) => { declaration(f).text = 'Mode decision pending: HOLD SCOPE — saved preference.'; },
  'withdrawn decision': (f: any) => { declaration(f).text += '\n\nI withdraw this decision.'; },
  'quoted declaration': (f: any) => { declaration(f).text = '> Mode: HOLD SCOPE — saved preference.'; },
  'example declaration': (f: any) => { declaration(f).text = 'Example:\nMode: HOLD SCOPE — saved preference.'; },
})) test(`explanatory announcement still rejects ${name}`, () => {
  const f = witnessed(); mutate(f); expect(decide(f)).toBeNull();
});

test('quoted historical correction does not withdraw the current completed mode', () => {
  const f = witnessed();
  declaration(f).text += '\n\n> Mode: SCOPE EXPANSION — a historical example.';
  expect(decide(f)?.option).toBe('HOLD SCOPE');
});

test('generic Skill annotations retain their existing non-CEO mode vocabulary', () => {
  const f = structuredClone(annotations.attempts[0]) as any;
  f.options.skillName = 'office-hours';
  f.tools.find((e: any) => e.kind === 'use' && e.name === 'Skill').input.skill = 'office-hours';
  const message = f.transcript.assistantMessages.find((m: any) => m.text.startsWith('Auto-decided'));
  message.text = 'Auto-decided workflow → **Builder** (your preference). Change with /plan-tune.\n\nMode: Builder (saved preference).';
  expect(decide(f)?.option).toBe('Builder');
  message.text += '\n\nMode: Startup (saved preference).';
  expect(decide(f)).toBeNull();
});

test('retained retry messages alone cannot authenticate missing tool and file evidence', () => {
  const retry = capture.retryObservation;
  expect(findNativeAutoDecision(retry.transcript, [], retry.options)).toBeNull();
});

test('exact retry prose accepts the optional decision label in an owned context', () => {
  const f = witnessed();
  // Only the text is replayed. Session/time and owned witness belong to the
  // first fixture; this is not a reconstruction or promotion of the retry.
  declaration(f).text = capture.retryObservation.transcript.assistantMessages.find(m =>
    m.text.startsWith('**Mode decision:'))!.text;
  expect(decide(f)?.option).toBe('HOLD SCOPE');
});

for (const field of ['Mode', 'Mode decision', 'Review mode', 'Review mode decision']) {
  test(`a completed field does not require a separate status word: ${field}`, () => {
    const f = witnessed(); declaration(f).text = `${field}: HOLD SCOPE (saved preference).`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
  test(`a later matching field does not withdraw its choice: ${field}`, () => {
    const f = witnessed(); declaration(f).text += `\n\n${field}: HOLD SCOPE (saved preference).`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
  test(`a conflicting later field still withdraws its choice: ${field}`, () => {
    const f = witnessed(); declaration(f).text += `\n\n${field}: SCOPE EXPANSION (saved preference).`;
    expect(decide(f)).toBeNull();
  });
}

const clone749 = () => structuredClone(captured749) as any;
const declaration749 = (f: any) => f.transcript.assistantMessages.find((m: any) =>
  m.timestamp === '2026-09-16T12:13:02.513Z');

test('actual 749 public declaration agrees with its retained owned append', () => {
  // Exact public tools, final declaration and owned log were captured while the
  // paid observer was still waiting. This free replay does not promote that run.
  const f = clone749();
  const result = decide(f);
  expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.annotation).toBe(declaration749(f).text);
  expect(result?.stateRecord).toEqual(f.options.stateEvidence.records[0]);
});

const explanatoryTails = [
  ' (saved preference; no prompt required). The review remains paused.',
  ' (saved preference (confirmed for this project); no prompt required). The review remains paused.',
  ' (saved preference), recorded for this invocation.',
  ' (saved preference): recorded for this invocation.',
  ' (saved preference) — recorded for this invocation.',
  ' (saved preference)\nThe review remains paused.',
  '. Selected from the saved preference (recorded).',
  '; selected from the saved preference (recorded).',
];
for (const tail of explanatoryTails) {
  test(`balanced explanation with following prose is a complete declaration: ${JSON.stringify(tail)}`, () => {
    const f = clone749(); declaration749(f).text = `Mode decision: HOLD SCOPE${tail}`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
  test(`matching later explanation preserves the current decision: ${JSON.stringify(tail)}`, () => {
    const f = clone749(); declaration749(f).text += `\n\nMode: HOLD SCOPE${tail}`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
  test(`conflicting later explanation withdraws the current decision: ${JSON.stringify(tail)}`, () => {
    const f = clone749(); declaration749(f).text += `\n\nMode: SCOPE EXPANSION${tail}`;
    expect(decide(f)).toBeNull();
  });
}

const incompleteFields = [
  'Mode: HOLD SCOPE (saved preference; recorded.',
  'Mode: HOLD SCOPE (saved preference (recorded).',
  'Mode: HOLD SCOPE (saved preference)). Recorded.',
  'Mode: HOLD SCOPE (saved preference)SCOPE EXPANSION',
  'Mode: HOLD SCOPE. A following explanation (unfinished.',
  'Mode: HOLD SCOPE (saved preference). If approved.',
  'Mode: HOLD SCOPE (saved preference (if approved)). Recorded.',
  'Mode: HOLD SCOPE (saved preference). Not yet selected.',
  'Mode: HOLD SCOPE (saved preference). I did not auto-decide the review mode.',
  'Mode: HOLD SCOPE (saved preference). This decision is withdrawn.',
  'Mode decision pending: HOLD SCOPE (saved preference). Recorded.',
  'Mode decision tentative: HOLD SCOPE (saved preference). Recorded.',
  'Mode: CUSTOM MODE (saved preference). Recorded.',
  'Mode: HOLD SCOPE / SCOPE EXPANSION (saved preference). Recorded.',
  'Mode: HOLD SCOPE (saved preference).\nMode decision pending: HOLD SCOPE',
];
for (const field of incompleteFields) {
  test(`explanatory prose cannot complete an unsupported field: ${JSON.stringify(field)}`, () => {
    const f = clone749(); declaration749(f).text = field;
    expect(decide(f)).toBeNull();
  });
  test(`later unsupported field retracts the earlier decision: ${JSON.stringify(field)}`, () => {
    const f = clone749(); declaration749(f).text += `\n\n${field}`;
    expect(decide(f)).toBeNull();
  });
}

for (const [name, mutate] of Object.entries({
  'missing owned append': (f: any) => { f.options.stateEvidence.records = []; },
  'foreign owned session': (f: any) => { f.options.stateEvidence.records[0].session_id = 'foreign'; },
  'duplicate owned append': (f: any) => { f.options.stateEvidence.records.push({ ...f.options.stateEvidence.records[0] }); },
  'conflicting logged choice': (f: any) => { f.options.stateEvidence.records[0].user_choice = 'SCOPE EXPANSION'; },
  'failed preamble': (f: any) => {
    const preamble = f.tools.find((e: any) => e.kind === 'use' && e.input?.command?.includes('gstack-skill-start'));
    f.tools.find((e: any) => e.kind === 'result' && e.toolUseId === preamble.toolUseId).isError = true;
  },
  'native question': (f: any) => { f.transcript.calls.push({ sessionId: f.options.sessionId }); },
  'declaration before owned append': (f: any) => { declaration749(f).timestamp = '2026-09-16T12:12:00.000Z'; },
  'quoted declaration': (f: any) => { declaration749(f).text = '> Mode: HOLD SCOPE (saved preference). Recorded.'; },
  'example declaration': (f: any) => { declaration749(f).text = 'Example:\nMode: HOLD SCOPE (saved preference). Recorded.'; },
})) test(`captured explanatory mode still requires ${name}`, () => {
  const f = clone749(); mutate(f); expect(decide(f)).toBeNull();
});

const reviewModes = ['HOLD SCOPE', 'SCOPE EXPANSION', 'SELECTIVE EXPANSION', 'SCOPE REDUCTION'];
for (const mode of reviewModes) {
  for (const tail of [' (saved preference). Recorded for this invocation.',
    ' (saved preference (confirmed); recorded). No further mode decision.',
    ` (saved preference). ${mode} is recorded for this invocation.`]) {
    test(`each complete review mode supports an unambiguous explanatory suffix: ${mode}${tail}`, () => {
      const f = clone749();
      Object.assign(f.options.stateEvidence.records[0], { user_choice: mode, recommended: mode });
      declaration749(f).text = `Mode: ${mode}${tail}`;
      expect(decide(f)?.option).toBe(mode);
    });
  }
  for (const other of reviewModes.filter(value => value !== mode)) {
    for (const connector of [' or ', ' versus ', ' vs. ', ' / ', ' | ', '; or ', ', choose ',
      '. Alternatively, select ', ' — instead choose ', ' (otherwise choose ', ' rather than ']) {
      test(`a second distinct mode in the suffix stays ambiguous: ${mode}${connector}${other}`, () => {
        const f = clone749();
        Object.assign(f.options.stateEvidence.records[0], { user_choice: mode, recommended: mode });
        const tail = connector.startsWith(' (') ? ')' : '';
        declaration749(f).text = `Mode: ${mode} (saved preference)${connector}${other}${tail}`;
        expect(decide(f)).toBeNull();
      });
    }
  }
}

test('alternate current mode spellings remain ambiguous after an explanatory parenthetical', () => {
  for (const alternative of ['scope expansion', 'SCOPE_EXPANSION', 'SCOPE   EXPANSION']) {
    const f = clone749(); declaration749(f).text = `Mode: HOLD SCOPE (saved preference); ${alternative}`;
    expect(decide(f)).toBeNull();
  }
});

test('generic annotation vocabulary retains its original parenthetical boundaries', () => {
  for (const suffix of ['', ' or Startup', '; or Startup', ' versus Startup', '. Recorded for this invocation.']) {
    const f = structuredClone(annotations.attempts[0]) as any;
    f.options.skillName = 'office-hours';
    f.tools.find((e: any) => e.kind === 'use' && e.name === 'Skill').input.skill = 'office-hours';
    const message = f.transcript.assistantMessages.find((m: any) => m.text.startsWith('Auto-decided'));
    message.text = `Auto-decided workflow → **Builder** (your preference). Change with /plan-tune.\n\nMode: Builder (saved preference)${suffix}`;
    expect(decide(f)?.option ?? null).toBe(suffix ? null : 'Builder');
  }
});
});

describe('auto-decide-recommendation-scope', () => {
const capture = capture_auto_decide_recommendation_scope;
const clone = () => structuredClone(capture) as any;
const decide = (f: any) => findNativeAutoDecision(f.transcript, f.tools, f.options);
const message = (f: any) => f.transcript.assistantMessages.find((m: any) =>
  m.timestamp === '2026-09-17T02:23:11.495Z');

test('actual completed mode and recommendation commentary match the retained owned audit', () => {
  const f = clone(), result = decide(f);
  expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.annotation).toBe(message(f).text);
  expect(result?.stateRecord).toEqual(f.options.stateEvidence.records[0]);
  expect(result?.preambleToolUseId).toBe('toolu_01Ni4b9NZeiexmcAz1jRTUa4');
});

const modes = ['HOLD SCOPE', 'SCOPE EXPANSION', 'SELECTIVE EXPANSION', 'SCOPE REDUCTION'];
for (const mode of modes) for (const commentary of [
  'recommendation would have been the same',
  'my recommendation might differ without the saved preference',
  `the recommendation would still be ${mode}`,
  'our recommendation will remain unchanged',
  'recommendation stays the same unless the product context changes',
]) test(`completed ${mode} is separate from ${commentary}`, () => {
  const f = clone();
  Object.assign(f.options.stateEvidence.records[0], { user_choice: mode, recommended: mode });
  message(f).text = `Decision: review mode is ${mode} (${commentary}).`;
  expect(decide(f)?.option).toBe(mode);
});

for (const separator of ['; ', ', ', '. ', ' — ', ' – ', ' - ', ' (']) {
  test(`recommendation assertion has an explicit boundary: ${JSON.stringify(separator)}`, () => {
    const f = clone();
    message(f).text = `Mode: HOLD SCOPE${separator}recommendation would have been unchanged${separator === ' (' ? ')' : ''}.`;
    expect(decide(f)?.option).toBe('HOLD SCOPE');
  });
}

const uncertain = [
  'Mode: would choose HOLD SCOPE.',
  'Mode: HOLD SCOPE if approved.',
  'Mode: HOLD SCOPE unless you object.',
  'Mode: HOLD SCOPE (I will make this selection).',
  'Mode: HOLD SCOPE (this choice might change).',
  'Mode: HOLD SCOPE (recommendation would be the same; if approved).',
  'Mode: HOLD SCOPE (recommendation would be the same, unless you object).',
  'Mode: HOLD SCOPE (recommendation would be the same and I will select it later).',
  'Mode: HOLD SCOPE (recommendation would be the same but the choice might change).',
  'Mode: HOLD SCOPE (recommendation would be the same while we would still need approval).',
  'Mode: HOLD SCOPE (recommendation would be the same; selection is pending).',
  'Mode: HOLD SCOPE (recommendation says the decision would be conditional).',
  'Mode: HOLD SCOPE (recommendation would still be SCOPE EXPANSION).',
  'Mode: HOLD SCOPE (recommendation would be unchanged). Not yet selected.',
  'Mode: HOLD SCOPE (recommendation would be unchanged). This decision is withdrawn.',
  'Mode pending: HOLD SCOPE (recommendation would be unchanged).',
  'Mode: not HOLD SCOPE (recommendation would be unchanged).',
  'Mode: HOLD SCOPE for a future review (recommendation would be unchanged).',
  'Mode: HOLD SCOPE for another draft (recommendation would be unchanged).',
  'Mode: HOLD SCOPELESS (recommendation would be unchanged).',
  'Mode: HOLD SCOPE (recommendation would be unchanged.',
];
for (const text of uncertain) {
  test(`commentary does not authenticate an uncertain choice: ${text}`, () => {
    const f = clone(); message(f).text = text;
    expect(decide(f)).toBeNull();
  });
  test(`later uncertain choice retracts the original completed decision: ${text}`, () => {
    const f = clone(); message(f).text += `\n\nCorrection: ${text}`;
    expect(decide(f)).toBeNull();
  });
}

for (const [name, wrap] of [
  ['quoted', (s: string) => `> ${s}`],
  ['indented', (s: string) => `    ${s}`],
  ['fenced', (s: string) => `\`\`\`text\n${s}\n\`\`\``],
  ['historical', (s: string) => `Previous decision:\n${s}`],
  ['example', (s: string) => `Example:\n${s}`],
] as const) test(`recommendation commentary cannot authenticate ${name} declarations`, () => {
  const f = clone(); message(f).text = wrap('Mode: HOLD SCOPE (recommendation would be unchanged).');
  expect(decide(f)).toBeNull();
});

for (const [name, mutate] of Object.entries({
  'missing owned log': (f: any) => { f.options.stateEvidence.records = []; },
  'duplicate owned log': (f: any) => { f.options.stateEvidence.records.push({ ...f.options.stateEvidence.records[0] }); },
  'foreign audit session': (f: any) => { f.options.stateEvidence.records[0].session_id = 'foreign'; },
  'different selected choice': (f: any) => { f.options.stateEvidence.records[0].user_choice = 'SCOPE EXPANSION'; },
  'different recommendation': (f: any) => { f.options.stateEvidence.records[0].recommended = 'SCOPE EXPANSION'; },
  'nonautomatic record': (f: any) => { f.options.stateEvidence.records[0].auto_decided = false; },
  'foreign native session': (f: any) => { f.options.sessionId = 'foreign'; },
  'missing successful preamble': (f: any) => { f.tools = f.tools.filter((e: any) => e.toolUseId !== 'toolu_01Ni4b9NZeiexmcAz1jRTUa4'); },
  'future audit': (f: any) => { f.options.stateEvidence.records[0].ts = new Date(f.options.now + 1).toISOString(); },
  'decision before completed log': (f: any) => { message(f).timestamp = new Date(Date.parse(f.options.stateEvidence.records[0].ts) - 1).toISOString(); },
  'surfaced native question': (f: any) => { f.transcript.calls.push({ sessionId: f.options.sessionId }); },
  'surfaced prose question': (f: any) => { f.options.proseQuestionObserved = true; },
})) test(`actual recommendation commentary retains ${name} rejection`, () => {
  const f = clone(); mutate(f); expect(decide(f)).toBeNull();
});
});

describe('auto-decide-saved-ai', () => {
const captured = captured_auto_decide_saved_ai;
const retry = retry_auto_decide_saved_ai;
const clone=()=>structuredClone(captured) as any;
const decision=(f=clone())=>findNativeAutoDecision(f.transcript,f.tools,f.options);
const message=(f:any)=>f.transcript.assistantMessages.find((m:any)=>m.text.includes('Auto-decided'));

test('actual saved mode preference annotation is a completed native auto-decision',()=>{
  const f=clone(), result=decision(f);
  expect(result).not.toBeNull();
  expect(result!.option).toBe('HOLD SCOPE');
  expect(message(f).text).toContain(result!.annotation);
  expect(f.transcript.calls).toEqual([]);
});

test('saved preference is bound to this invoked skill and an agreeing current mode',()=>{
  for(const change of [
    (s:string)=>s.replace('`plan-ceo-review-mode`','`plan-design-review-mode`'),
    (s:string)=>s.replace('`plan-ceo-review-mode`','`plan-ceo-review-routing`'),
    (s:string)=>s.replace('**Review mode: HOLD SCOPE.**','**Review mode: SCOPE EXPANSION.**'),
    (s:string)=>s.replace('**Review mode: HOLD SCOPE.**\n\n',''),
    (s:string)=>s.replace('"Select review mode"','"Select report folder"'),
    (s:string)=>s.replace('(your saved preference on','(a proposed preference on'),
    (s:string)=>s.replace('Change with /plan-tune.',''),
    (s:string)=>s.replace('Auto-decided','I will auto-decide'),
  ]) {const f=clone();message(f).text=change(message(f).text);expect(decision(f)).toBeNull();}
});

test('prefixed examples, quotations and hypothetical notices do not assert a current choice',()=>{
  for(const change of [
    (s:string)=>'Example:\n\n'+s,
    (s:string)=>'```text\n'+s+'\n```',
    (s:string)=>s.split('\n').map(l=>'> '+l).join('\n'),
    (s:string)=>s.replace('Heads-up from the preamble: unshipped work on this branch','Heads-up from the preamble: a hypothetical example'),
    (s:string)=>s.replace('Auto-decided "Select','    Auto-decided "Select'),
    (s:string)=>s.replace('Auto-decided "Select','If approved, Auto-decided "Select'),
  ]) {const f=clone();message(f).text=change(message(f).text);expect(decision(f)).toBeNull();}
});

test('failed loads, foreign sessions, actual questions and later withdrawals retain precedence',()=>{
  for(const mutate of [
    (f:any)=>{f.options.sessionId='foreign';},
    (f:any)=>{const use=f.tools.find((t:any)=>t.kind==='use'&&t.name==='Skill');f.tools.find((t:any)=>t.kind==='result'&&t.toolUseId===use.toolUseId).isError=true;},
    (f:any)=>{f.transcript.calls.push({sessionId:f.options.sessionId,toolUseId:'actual-question'});},
    (f:any)=>{f.options.now=Date.parse(message(f).timestamp)-1;},
    (f:any)=>{message(f).text+='\n\nCorrection: I withdraw this decision.';},
    (f:any)=>{message(f).text+='\n\n**Review mode: SCOPE EXPANSION.**';},
  ]) {const f=clone();mutate(f);expect(decision(f)).toBeNull();}
});
test('actual retry mode-decision heading retains its own annotation, excluding prior foreign text',()=>{
  const f:any=structuredClone(retry), actual=decision(f);
  expect(actual).not.toBeNull();expect(actual!.sessionId).toBe(f.options.sessionId);
  expect(actual!.option).toBe('HOLD SCOPE');
  expect(actual!.annotation).toContain('(your preference)');
  const own=f.transcript.assistantMessages.filter((m:any)=>m.sessionId===f.options.sessionId);
  f.transcript.assistantMessages=f.transcript.assistantMessages.filter((m:any)=>m.sessionId!==f.options.sessionId);
  expect(decision(f)).toBeNull();expect(own.length).toBeGreaterThan(0);
});

test('retry heading cannot supply a hypothetical, different decision, or withdrawn selection',()=>{
  for(const change of [
    (s:string)=>'Example:\n\n'+s,
    (s:string)=>s.replace('Review mode for the deterministic','Review mode for the hypothetical'),
    (s:string)=>s.replace('D1 — Review mode','D1 — Report destination'),
    (s:string)=>s.replace('Auto-decided "Review mode:', 'Auto-decided "Report destination:'),
    (s:string)=>s.replace('→ **HOLD SCOPE**','→ **Save a file**'),
    (s:string)=>s+'\n\nCorrection: I withdraw this selection.',
    (s:string)=>s+'\n\n**Review mode: SCOPE EXPANSION.**',
    (s:string)=>s.replace('Heads-up from gstack: there is unshipped work on this branch','Heads-up from gstack: here is an example'),
  ]) {const f:any=structuredClone(retry),m=f.transcript.assistantMessages.find((m:any)=>m.sessionId===f.options.sessionId&&m.text.includes('Auto-decided'));m.text=change(m.text);expect(decision(f)).toBeNull();}
});
});

describe('auto-decide-structured', () => {
const capture = capture_auto_decide_structured;
const priorAnnotation = captured_auto_decide_saved_ai;
const completedModeCapture = completedModeCapture_auto_decide_structured;
const statusFixture = completedModeCapture_auto_decide_structured;
const clone = () => structuredClone(capture) as any;
const decision = (f = clone()) => findNativeAutoDecision(f.transcript, f.tools, f.options);
test('actual slash expansion with completed preference log and current mode is an auto-decision', () => {
  const f = clone();
  expect(f.tools.some((e: any) => e.name === 'Skill')).toBe(false);
  expect(f.transcript.calls).toEqual([]);
  const result = decision(f);
  expect(result).not.toBeNull();
  expect(result!.option).toBe('HOLD SCOPE');
});

const use = (f: any, name: string) => f.tools.find((e: any) => e.kind === 'use' && e.input?.command?.includes(name));
const ack = (f: any, request: any) => f.tools.find((e: any) => e.kind === 'result' && e.toolUseId === request.toolUseId);
const modeMessage = (f: any) => f.transcript.assistantMessages.find((m: any) => m.text.includes('**Mode:'));
const changeLog = (f: any, modify: (log: any) => void) => {
  const request = use(f, 'gstack-question-log'), match = /'(\{.*\})'/.exec(request.input.command)!;
  const value = JSON.parse(match[1]!); modify(value);
  request.input.command = request.input.command.replace(match[1], JSON.stringify(value));
};

for (const [label, mutate] of Object.entries({
  'missing transcript': (f: any) => { f.transcript.status = 'missing'; },
  'foreign owned session': (f: any) => { f.options.sessionId = 'foreign'; },
  'wrong invoked skill': (f: any) => { f.options.skillName = 'plan-eng-review'; },
  'pre-command evidence': (f: any) => { f.options.commandStartedAt = Date.parse(modeMessage(f).timestamp); },
  'future final statement': (f: any) => { f.options.now = Date.parse(modeMessage(f).timestamp) - 1; },
  'invalid final timestamp': (f: any) => { modeMessage(f).timestamp = 'invalid'; },
  'native question': (f: any) => { f.transcript.calls.push({ sessionId: f.options.sessionId, toolUseId: 'asked' }); },
  'malformed native question tool': (f: any) => { f.tools.push({ ...use(f, 'gstack-question-log'), toolUseId: 'asked', name: 'mcp__ask__AskUserQuestion', input: {} }); },
  'earlier visible prose question': (f: any) => { f.options.proseQuestionObserved = true; },
  'public reply request': (f: any) => { modeMessage(f).text += '\nReply with A or B.'; },
  'public option list': (f: any) => { modeMessage(f).text += '\nA) Hold scope\nB) Expand scope'; },
  'no preamble': (f: any) => { const request = use(f, 'gstack-skill-start'); f.tools = f.tools.filter((e: any) => e.toolUseId !== request.toolUseId); },
  'preamble failed': (f: any) => { ack(f, use(f, 'gstack-skill-start')).isError = true; },
  'preamble missing ACK': (f: any) => { const request = use(f, 'gstack-skill-start'); f.tools = f.tools.filter((e: any) => e !== ack(f, request)); },
  'preamble duplicate': (f: any) => { f.tools.push({ ...use(f, 'gstack-skill-start') }); },
  'wrong preamble skill': (f: any) => { use(f, 'gstack-skill-start').input.command = use(f, 'gstack-skill-start').input.command.replace('--skill "plan-ceo-review"', '--skill "plan-eng-review"'); },
  'question tuning disabled': (f: any) => { const result = ack(f, use(f, 'gstack-skill-start')); result.content = result.content.replace('QUESTION_TUNING: true', 'QUESTION_TUNING: false'); },
  'ambiguous preamble session': (f: any) => { ack(f, use(f, 'gstack-skill-start')).content = 'SKILL_START_PROTO: 1\nQUESTION_TUNING: true\nSESSION_ID: duplicate\n' + ack(f, use(f, 'gstack-skill-start')).content; },
  'nonzero preference': (f: any) => { ack(f, use(f, 'gstack-question-preference')).content = 'AUTO_DECIDE\nEXIT: 1'; },
  'ASK preference': (f: any) => { ack(f, use(f, 'gstack-question-preference')).content = 'ASK\nEXIT: 0'; },
  'preference error': (f: any) => { ack(f, use(f, 'gstack-question-preference')).isError = true; },
  'wrong preference id': (f: any) => { use(f, 'gstack-question-preference').input.command = use(f, 'gstack-question-preference').input.command.replace('--check "plan-ceo-review-mode"', '--check "plan-ceo-review-other"'); },
  'no preference check': (f: any) => { const request = use(f, 'gstack-question-preference'); f.tools = f.tools.filter((e: any) => e.toolUseId !== request.toolUseId); },
  'unacknowledged log': (f: any) => { const request = use(f, 'gstack-question-log'); f.tools = f.tools.filter((e: any) => e !== ack(f, request)); },
  'failed log': (f: any) => { ack(f, use(f, 'gstack-question-log')).isError = true; },
  'fallback log result': (f: any) => { ack(f, use(f, 'gstack-question-log')).content = 'log unavailable (best-effort)'; },
  'wrong log session': (f: any) => changeLog(f, log => { log.session_id = 'foreign'; }),
  'wrong log skill': (f: any) => changeLog(f, log => { log.skill = 'plan-eng-review'; }),
  'wrong log question id': (f: any) => changeLog(f, log => { log.question_id = 'plan-ceo-review-scope'; }),
  'nonautomatic log': (f: any) => changeLog(f, log => { log.auto_decided = false; }),
  'string automatic flag': (f: any) => changeLog(f, log => { log.auto_decided = 'true'; }),
  'unmatched recommendation': (f: any) => changeLog(f, log => { log.recommended = 'SCOPE_EXPANSION'; }),
  'different logged mode': (f: any) => changeLog(f, log => { log.recommended = log.user_choice = 'SCOPE_EXPANSION'; }),
  'arbitrary logged value': (f: any) => changeLog(f, log => { log.recommended = log.user_choice = 'APPROVE_SCOPE'; }),
  'nondecision summary': (f: any) => changeLog(f, log => { log.question_summary = ''; }),
  'later checked preference': (f: any) => { ack(f, use(f, 'gstack-question-preference')).timestamp = modeMessage(f).timestamp; },
  'mode before log ACK': (f: any) => { modeMessage(f).timestamp = use(f, 'gstack-question-log').timestamp; },
  'reversed log ACK': (f: any) => { ack(f, use(f, 'gstack-question-log')).timestamp = use(f, 'gstack-question-preference').timestamp; },
  'duplicate log ACK': (f: any) => { f.tools.push({ ...ack(f, use(f, 'gstack-question-log')) }); },
  'foreign log ACK': (f: any) => { ack(f, use(f, 'gstack-question-log')).sessionId = 'foreign'; },
  'missing current statement': (f: any) => { modeMessage(f).text = 'Done. Waiting for your next instruction.'; },
})) test(`structured current mode rejects ${label}`, () => {
  const f = clone(); mutate(f); expect(decision(f)).toBeNull();
});

for (const name of ['gstack-skill-start', 'gstack-question-preference', 'gstack-question-log']) {
  for (const [label, change] of Object.entries({
    'echoed source': (s: string) => `echo '${s.replaceAll("'", "'\\''")}'`,
    'conditional command': (s: string) => `false && ${s}`,
    'commented source': (s: string) => `# ${s}`,
    'extra prefix command': (s: string) => `true; ${s}`,
    'extra suffix command': (s: string) => `${s}; true`,
    'command substitution': (s: string) => `echo "$(${s})"`,
  })) test(`${name} cannot authenticate ${label}`, () => {
    const f = clone(); use(f, name).input.command = change(use(f, name).input.command); expect(decision(f)).toBeNull();
  });
}

for (const [label, text] of Object.entries({
  'plain current field': 'Mode: HOLD SCOPE.',
  'parenthetical explanation with punctuation': 'Mode: HOLD SCOPE (saved preference, confirmed).',
  'parenthetical review explanation': '**Review mode: HOLD SCOPE (saved preference; confirmed).**',
  'current review field': '**Review mode: HOLD SCOPE.**',
  'compact completion': '**STATUS: DONE**\n\nMode: HOLD SCOPE',
  'bullet conclusion': 'The requested routing decision is complete.\n\n- **Mode: HOLD SCOPE**, using the saved preference.\n\nThe substantive review is deferred.',
  'quoted historical contradiction': 'Mode: HOLD SCOPE.\n\nEarlier example: "Review mode: SCOPE EXPANSION."',
})) test(`completed structured log supports ${label} without exact annotation prose`, () => {
  const f = clone(); modeMessage(f).text = text;
  const result = decision(f); expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.skillToolUseId).toBeUndefined();
  expect(result?.preambleToolUseId).toBe(use(f, 'gstack-skill-start').toolUseId);
  expect(result?.annotation).toBe(text);
});

for (const text of [
  '> Mode: HOLD SCOPE.', '    Mode: HOLD SCOPE.', '`Mode: HOLD SCOPE.`',
  '```text\nMode: HOLD SCOPE.\n```', 'Example:\n\nMode: HOLD SCOPE.',
  'Previous transcript:\n\nMode: HOLD SCOPE.', 'If approved, Mode: HOLD SCOPE.',
  'Mode: HOLD SCOPE, if you approve.', 'Mode: HOLD SCOPE, pending approval.',
  'Mode: HOLD SCOPE?', 'Mode: HOLD SCOPELESS.',
  'Mode: HOLD SCOPE (withdrawn).', 'Mode: HOLD SCOPE (retracted).',
  'Mode: HOLD SCOPE.\n\nMode: HOLD SCOPE (pending approval).',
  'Mode: HOLD SCOPE.\n\nCorrection: I withdraw this decision.',
  'Mode: HOLD SCOPE.\n\nI did not auto-decide the review mode.',
  'Mode: HOLD SCOPE.\n\nCorrection: Mode: SCOPE EXPANSION.',
  'Mode: HOLD SCOPE.\n\nMode: SCOPE EXPANSION.',
]) test(`quoted, conditional or withdrawn mode has no completed choice: ${JSON.stringify(text)}`, () => {
  const f = clone(); modeMessage(f).text = text; expect(decision(f)).toBeNull();
});

test('the same command contracts also support direct literal invocations and quiet ACKs', () => {
  const f = clone();
  use(f, 'gstack-skill-start').input.command = '"$HOME/.claude/skills/gstack/bin/gstack-skill-start" --model claude --skill plan-ceo-review --parent-pid "$PPID"';
  use(f, 'gstack-question-preference').input.command = '~/.claude/skills/gstack/bin/gstack-question-preference --check plan-ceo-review-mode';
  ack(f, use(f, 'gstack-question-preference')).content = 'AUTO_DECIDE\n';
  use(f, 'gstack-question-log').input.command = use(f, 'gstack-question-log').input.command.split(' 2>/dev/null')[0];
  ack(f, use(f, 'gstack-question-log')).content = '';
  expect(decision(f)?.option).toBe('HOLD SCOPE');
});
for (const status of ['undecided', 'not selected', 'pending approval', 'none']) {
  test(`later Review mode: ${status} withdraws both existing annotation and structured decision`, () => {
    const previous: any = structuredClone(priorAnnotation);
    previous.transcript.assistantMessages.find((m: any) => m.text.includes('Auto-decided')).text += `\n\nReview mode: ${status}.`;
    expect(findNativeAutoDecision(previous.transcript, previous.tools, previous.options)).toBeNull();
    const f = clone(); modeMessage(f).text += `\n\nReview mode: ${status}.`;
    expect(decision(f)).toBeNull();
  });
  test(`later Mode: ${status} withdraws a structured decision`, () => {
    const f = clone(); modeMessage(f).text += `\n\n- **Mode: ${status}.**`;
    expect(decision(f)).toBeNull();
  });
}

for (const name of ['gstack-question-preference', 'gstack-question-log']) test(`${name} cannot borrow an earlier success after a contradictory current call`, () => {
  const f = clone(), request = structuredClone(use(f, name)), result = structuredClone(ack(f, request));
  request.toolUseId += '-later'; result.toolUseId = request.toolUseId;
  request.timestamp = result.timestamp = new Date(Date.parse(modeMessage(f).timestamp) - 1).toISOString();
  if (name === 'gstack-question-preference') result.content = 'ASK\nEXIT: 0';
  else request.input.command = request.input.command.replace('"auto_decided":true', '"auto_decided":false');
  f.tools.push(request, result); expect(decision(f)).toBeNull();
});

test('a literal command cannot treat a physical newline as argument whitespace', () => {
  const f = clone();
  use(f, 'gstack-question-log').input.command = use(f, 'gstack-question-log').input.command.replace("gstack-question-log '", "gstack-question-log\n'");
  expect(decision(f)).toBeNull();
});

for (const fallback of ['"LOGGED"', '" LOGGED "', '"\\x4cOGGED"', '-e "\\x4cOGGED"'])
  test(`a failure branch cannot impersonate the question-log success marker: ${fallback}`, () => {
    const f = clone(), request = use(f, 'gstack-question-log');
    request.input.command = request.input.command.replace('"log unavailable (best-effort)"', fallback);
    expect(decision(f)).toBeNull();
  });
{
const copy=()=>structuredClone(completedModeCapture);
const check=(f:any)=>findNativeAutoDecision(f.transcript,f.tools,f.options);
const message=(f:any)=>f.transcript.assistantMessages.find((m:any)=>m.text.includes('Mode decision done:'));
const logUse=(f:any)=>f.tools.find((t:any)=>t.kind==='use'&&t.input?.command?.includes('gstack-question-log'));
test('actual owned public attempt fails original and completes mode-only with full acknowledged authority',()=>{
 const f=copy();const v=check(f);expect(v?.option).toBe('HOLD SCOPE');expect(v?.questionLogToolUseId).toBe(logUse(f).toolUseId);
});
const mutations:Record<string,(f:any)=>void>={
 'unlogged':f=>{const id=logUse(f).toolUseId;f.tools=f.tools.filter((t:any)=>t.toolUseId!==id)},
 'failed log':f=>{f.tools.find((t:any)=>t.kind==='result'&&t.toolUseId===logUse(f).toolUseId).isError=true},
 'masked log failure':f=>{logUse(f).input.command=logUse(f).input.command.replace('&& echo','; echo')},
 'wrong returned marker':f=>{f.tools.find((t:any)=>t.kind==='result'&&t.toolUseId===logUse(f).toolUseId).content='LOG_FAILED (best-effort)'},
 'unmatched quote':f=>{logUse(f).input.command=logUse(f).input.command.replace('"LOGGED"','"LOGGED')},
 'foreign session':f=>{f.options.sessionId='foreign'},
 'wrong mode':f=>{message(f).text=message(f).text.replace('done: HOLD SCOPE','done: SCOPE EXPANSION')},
 'unfinished':f=>{message(f).text=message(f).text.replace('Mode decision done:','Mode decision pending:')},
 'late declaration':f=>{message(f).timestamp=new Date(f.options.now+1000).toISOString()},
 'prior declaration':f=>{message(f).timestamp=new Date(f.options.commandStartedAt-1000).toISOString()},
 'cancelled':f=>{message(f).text+='\n\nI cancel this decision.'},
 'wrong later completed mode':f=>{message(f).text+='\n\nMode decision done: SCOPE EXPANSION'},
 'quoted declaration':f=>{message(f).text='> '+message(f).text},
 'hypothetical':f=>{message(f).text='Example:\n'+message(f).text},
 'conditional':f=>{message(f).text=message(f).text.replace('done: HOLD SCOPE','done: HOLD SCOPE (if approved)')},
 'native question surfaced':f=>{f.transcript.calls.push({sessionId:f.options.sessionId})},
 'wrong logged mode':f=>{logUse(f).input.command=logUse(f).input.command.replace('"user_choice":"HOLD SCOPE"','"user_choice":"SCOPE EXPANSION"')},
};
for(const [name,mutate] of Object.entries(mutations))test(name,()=>{const f=copy();mutate(f);expect(check(f)).toBeNull()});

for(const completion of ['done','complete','completed']) {
 test(`completed mode class ${completion}`,()=>{const f=copy();message(f).text=message(f).text.replace('decision done:','decision '+completion+':');expect(check(f)?.option).toBe('HOLD SCOPE')});
 test(`conflicting later completed mode ${completion}`,()=>{const f=copy();message(f).text+='\n\nMode decision '+completion+': SCOPE EXPANSION';expect(check(f)).toBeNull()});
 test(`unfinished completed mode ${completion}`,()=>{const f=copy();message(f).text=message(f).text.replace('done: HOLD SCOPE',completion+': HOLD SCOPE (pending approval)');expect(check(f)).toBeNull()});
}
test('paired single-quoted success token retains exact shell ACK',()=>{const f=copy();logUse(f).input.command=logUse(f).input.command.replace('"LOGGED"',"'LOGGED'");expect(check(f)?.option).toBe('HOLD SCOPE')});
test('unpaired single-quoted success token cannot authenticate log',()=>{const f=copy();logUse(f).input.command=logUse(f).input.command.replace('"LOGGED"',"'LOGGED");expect(check(f)).toBeNull()});

}
{
const fixture=statusFixture;
const fixed=findNativeAutoDecision;
const copy=()=>structuredClone(fixture) as any;
const message=(f:any)=>f.transcript.assistantMessages.find((m:any)=>m.text.includes('Mode decision done:'));
const check=(f:any)=>fixed(f.transcript,f.tools,f.options);
test('current pending status retracts the completed owned mode',()=>{const f=copy();message(f).text+='\n\nMode decision pending: HOLD SCOPE';expect(check(f)).toBeNull()});
for(const status of ['pending','pending approval','unfinished','incomplete','cancelled','canceled','withdrawn','retracted','revoked','undecided','proposed','not selected','not decided','not yet complete','in progress','on hold','unknown']){
 test(`unfinished declaration ${status}`,()=>{const f=copy();message(f).text=message(f).text.replace('decision done:','decision '+status+':');expect(check(f)).toBeNull()});
 test(`later unfinished status ${status}`,()=>{const f=copy();message(f).text+='\n\nMode decision '+status+': HOLD SCOPE';expect(check(f)).toBeNull()});
 test(`quoted historical status ${status}`,()=>{const f=copy();message(f).text+='\n\n> Historical example:\n> Mode decision '+status+': HOLD SCOPE';expect(check(f)?.option).toBe('HOLD SCOPE')});
}
for(const status of ['done','complete','completed']){
 test(`same current completed field ${status}`,()=>{const f=copy();message(f).text+='\n\nMode decision '+status+': HOLD SCOPE';expect(check(f)?.option).toBe('HOLD SCOPE')});
 test(`completed conflicting field ${status}`,()=>{const f=copy();message(f).text+='\n\nMode decision '+status+': SCOPE EXPANSION';expect(check(f)).toBeNull()});
}
for(const status of ['unfinished','incomplete','pending approval','cancelled','not completed'])test(`unfinished value suffix ${status}`,()=>{const f=copy();message(f).text+='\n\nMode decision done: HOLD SCOPE ('+status+')';expect(check(f)).toBeNull()});
for(const text of ['Historical example: Mode decision pending: HOLD SCOPE','```\nMode decision pending: HOLD SCOPE\n```','"Mode decision cancelled: HOLD SCOPE"'])test(`unasserted historical field ${text}`,()=>{const f=copy();message(f).text+='\n\n'+text;expect(check(f)?.option).toBe('HOLD SCOPE')});
}
});

describe('auto-decide-target-identity', () => {
const capture = capture_auto_decide_target_identity;
const clone = () => structuredClone(capture) as any;
const message = (f: any) => f.transcript.assistantMessages.at(-1);
const decide = (f: any) => findNativeAutoDecision(f.transcript, f.tools, f.options);

test('actual quoted current title and completed owned audit produce the original mode decision', () => {
  const f = clone(), result = decide(f);
  expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.annotation).toBe(message(f).text);
  expect(result?.stateRecord).toEqual(f.options.stateEvidence.records[0]);
  expect(result?.preambleToolUseId).toBe('toolu_01KbsH6ybJxbNozwbSXywVbb');
});

const title = 'deterministic skill-list ordering';
const modes = ['HOLD SCOPE', 'SCOPE EXPANSION', 'SELECTIVE EXPANSION', 'SCOPE REDUCTION'];
for (const mode of modes) for (const quote of [(s: string) => `"${s}"`, (s: string) => `“${s}”`, (s: string) => `\`${s}\``]) {
  for (const wrapper of ['', ' draft', ' plan']) test(`${mode} quoted title agrees with one audit wrapper: ${quote(title)}${wrapper}`, () => {
    const f = clone();
    Object.assign(f.options.stateEvidence.records[0], { user_choice: mode, recommended: mode, question_summary: `Select review mode for ${title}${wrapper}` });
    message(f).text = `Decision: ${mode} for ${quote(title)}.\n\nMode: ${mode}, auto-selected using the saved preference.`;
    expect(decide(f)?.option).toBe(mode);
    expect(decide(f)?.annotation).toBe(message(f).text);
  });
}
for (const [declared, recorded] of [
  [`"${title}" draft`, `"${title}"`],
  [`"${title}" plan`, `${title} draft`],
  [title, `${title} draft`],
  [`${title} draft`, title],
  ['"release plan"', 'release plan draft'],
  ['"what if ordering"', 'what if ordering draft'],
  ['"ordering v2. current"', '"ordering v2. current" draft'],
]) test(`exact title identity with syntactic wrapper: ${declared} / ${recorded}`, () => {
  const f = clone(); f.options.stateEvidence.records[0].question_summary = `Select mode for ${recorded}`;
  message(f).text = `Decision: HOLD SCOPE for ${declared}.`;
  expect(decide(f)?.option).toBe('HOLD SCOPE');
});

test('quoted target and mode labels remain case insensitive', () => {
  const f = clone(); message(f).text = `decision: hold scope FOR "${title}".`;
  expect(decide(f)?.option).toBe('HOLD SCOPE');
});

const negatives: Array<[string, string]> = [
  ['"deterministic skill-list sorting"', `${title} draft`],
  ['"skill-list ordering"', `${title} draft`],
  [`"${title}-v2"`, `${title} draft`],
  [`"${title} extra"`, `${title} draft`],
  ['"release"', '"release draft"'],
  ['"release draft"', '"release"'],
  ['"release plan"', 'release draft'],
  ['release plan', 'release draft'],
  ['"release draft plan"', 'release plan'],
  ['"release plan draft"', '"release plan"'],
  ['"draft release"', 'release'],
  ['""', 'draft'],
  ['"   "', 'plan'],
  [`"${title}" or "foreign"`, `${title} draft`],
  [`"${title}" and another plan`, `${title} draft`],
  [`"${title}`, `${title} draft`],
  [`${title}"`, `${title} draft`],
  ['"future plan"', 'future plan'],
  ['future', 'future plan'],
  ['previous', 'previous draft'],
  ['"previous draft"', 'previous draft'],
  ['"another draft"', 'another draft'],
  ['"next plan"', 'next plan'],
];
for (const [declared, recorded] of negatives) {
  test(`target cannot borrow a named or historical match: ${declared} / ${recorded}`, () => {
    const f = clone(); f.options.stateEvidence.records[0].question_summary = `Select mode for ${recorded}`;
    message(f).text = `Decision: HOLD SCOPE for ${declared}.`;
    expect(decide(f)).toBeNull();
  });
  test(`later agreeing Mode does not erase invalid target: ${declared} / ${recorded}`, () => {
    const f = clone(); f.options.stateEvidence.records[0].question_summary = `Select mode for ${recorded}`;
    message(f).text = `Decision: HOLD SCOPE for ${declared}.\n\nMode: HOLD SCOPE, auto-selected.`;
    expect(decide(f)).toBeNull();
  });
}

for (const wrap of [
  (s: string) => `"${s}"`, (s: string) => `“${s}”`, (s: string) => `\`${s}\``,
  (s: string) => `> ${s}`, (s: string) => `    ${s}`, (s: string) => `\`\`\`text\n${s}\n\`\`\``,
  (s: string) => `Example:\n${s}`, (s: string) => `Previous review:\n${s}`,
]) test(`only an asserted field can own a quoted target: ${wrap('Decision')}`, () => {
  const f = clone(); message(f).text = wrap(`Decision: HOLD SCOPE for "${title}".`);
  expect(decide(f)).toBeNull();
});

for (const value of [
  `HOLD SCOPE for "${title}" if approved`, `HOLD SCOPE for "${title}", pending approval`,
  `not HOLD SCOPE for "${title}"`, `HOLD SCOPE for "${title}"; SCOPE EXPANSION`,
  `HOLD SCOPE for "${title}" (withdrawn)`, `HOLD SCOPE for "${title}" (I will select it)`,
]) test(`quoted name cannot hide a lifecycle veto: ${value}`, () => {
  const f = clone(); message(f).text = `Decision: ${value}.\n\nMode: HOLD SCOPE.`;
  expect(decide(f)).toBeNull();
});
for (const suffix of [
  '\n\nCorrection: Mode: SCOPE EXPANSION.',
  '\n\nCorrection: I withdraw this decision.',
  `\n\nDecision: HOLD SCOPE for "foreign target".`,
  '\n\nMode pending: HOLD SCOPE.',
]) test(`a later contradiction remains effective: ${suffix}`, () => {
  const f = clone(); message(f).text += suffix; expect(decide(f)).toBeNull();
});
for (const [name, mutate] of Object.entries({
  'missing owned log': (f: any) => { f.options.stateEvidence.records = []; },
  'duplicate owned log': (f: any) => { f.options.stateEvidence.records.push({ ...f.options.stateEvidence.records[0] }); },
  'foreign audit session': (f: any) => { f.options.stateEvidence.records[0].session_id = 'foreign'; },
  'different audit choice': (f: any) => { f.options.stateEvidence.records[0].user_choice = 'SCOPE EXPANSION'; },
  'wrong preference': (f: any) => { f.options.stateEvidence.preference = 'ask'; },
  'missing preamble ACK': (f: any) => { f.tools = f.tools.filter((e: any) => !(e.kind === 'result' && e.toolUseId === 'toolu_01KbsH6ybJxbNozwbSXywVbb')); },
  'native question': (f: any) => { f.transcript.calls.push({sessionId:f.options.sessionId}); },
  'prose question': (f: any) => { f.options.proseQuestionObserved = true; },
  'decision before log': (f: any) => { message(f).timestamp = new Date(Date.parse(f.options.stateEvidence.records[0].ts) - 1).toISOString(); },
  'wrong native session': (f: any) => { f.options.sessionId = 'foreign'; },
  'future log': (f: any) => { f.options.stateEvidence.records[0].ts = new Date(f.options.now + 1).toISOString(); },
})) test(`actual quoted target retains ${name} boundary`, () => {
  const f = clone(); mutate(f); expect(decide(f)).toBeNull();
});
for (const preposition of ['for', 'FOR']) test(`a quoted lifecycle word belongs to its title with ${preposition}`, () => {
  const f = clone(); f.options.stateEvidence.records[0].question_summary = 'Select mode for Pending notifications draft';
  message(f).text = `Decision: HOLD SCOPE ${preposition} "Pending notifications".`;
  expect(decide(f)?.option).toBe('HOLD SCOPE');
});
});

describe('auto-decision-state', () => {
const capture = capture_auto_decision_state;
const clone = () => structuredClone(capture) as any;
const qid = 'plan-ceo-review-mode';
function state(f: any) {
  const use = f.tools.find((e: any) => e.input?.command?.includes('gstack-question-log'));
  // Synthetic file witness, built from the actual literal request. The original
  // run did not retain this file, and is still a failed paid attempt.
  const record = JSON.parse(/gstack-question-log '(\{[^\n]*\})'/.exec(use.input.command)![1]!);
  record.source = 'agent';
  record.ts = f.tools.find((e: any) => e.kind === 'result' && e.toolUseId === use.toolUseId).timestamp;
  return { questionId: qid, preference: 'never-ask' as const, records: [record] };
}
const decide = (f: any) => findNativeAutoDecision(f.transcript, f.tools, f.options);
const mode = (f: any) => f.transcript.assistantMessages.find((m: any) => m.text.startsWith('**Mode:'));

test('original captured retry cannot prove a masked log succeeded', () => {
  expect(decide(clone())).toBeNull();
});
test('actual retry declaration plus a completed owned append proves the chosen mode', () => {
  const f = clone(); f.options.stateEvidence = state(f);
  const result = decide(f);
  expect(result?.option).toBe('HOLD SCOPE');
  expect(result?.stateRecord).toEqual(f.options.stateEvidence.records[0]);
  expect(result?.questionLogToolUseId).toBeUndefined();
});

for (const [name, mutate] of Object.entries({
  'foreign record session': (f: any) => { f.options.stateEvidence.records[0].session_id = 'foreign'; },
  'wrong question': (f: any) => { f.options.stateEvidence.questionId = 'wrong'; },
  'wrong skill': (f: any) => { f.options.stateEvidence.records[0].skill = 'plan-eng-review'; },
  'nonautomatic record': (f: any) => { f.options.stateEvidence.records[0].auto_decided = false; },
  'string flag': (f: any) => { f.options.stateEvidence.records[0].auto_decided = 'true'; },
  'wrong source': (f: any) => { f.options.stateEvidence.records[0].source = 'hook'; },
  'different preference': (f: any) => { f.options.stateEvidence.preference = 'always-ask'; },
  'missing append': (f: any) => { f.options.stateEvidence.records = []; },
  'duplicate append': (f: any) => { f.options.stateEvidence.records.push({ ...f.options.stateEvidence.records[0] }); },
  'contradictory recommendation': (f: any) => { f.options.stateEvidence.records[0].recommended = 'SCOPE EXPANSION'; },
  'empty summary': (f: any) => { f.options.stateEvidence.records[0].question_summary = ''; },
  'old record': (f: any) => { f.options.stateEvidence.records[0].ts = new Date(f.options.commandStartedAt - 1).toISOString(); },
  'future record': (f: any) => { f.options.stateEvidence.records[0].ts = new Date(f.options.now + 1).toISOString(); },
  'record after declaration': (f: any) => { f.options.stateEvidence.records[0].ts = new Date(Date.parse(mode(f).timestamp) + 1).toISOString(); },
  'invalid timestamp': (f: any) => { f.options.stateEvidence.records[0].ts = 'invalid'; },
  'actual native question': (f: any) => { f.transcript.calls.push({ sessionId: f.options.sessionId }); },
  'actual prose question': (f: any) => { f.options.proseQuestionObserved = true; },
  'failed preamble': (f: any) => { f.tools.find((e: any) => e.kind === 'result' && e.content?.includes('SKILL_START_PROTO')).isError = true; },
  'quoted declaration': (f: any) => { mode(f).text = '> Mode: HOLD SCOPE (saved preference).'; },
  'conditional declaration': (f: any) => { mode(f).text = 'Mode: HOLD SCOPE (if approved).'; },
  'later withdrawal': (f: any) => { mode(f).text += '\n\nCorrection: I withdraw this decision.'; },
  'later different mode': (f: any) => { mode(f).text += '\n\nMode: SCOPE EXPANSION (saved preference).'; },
})) test(`owned log witness rejects ${name}`, () => {
  const f = clone(); f.options.stateEvidence = state(f); mutate(f); expect(decide(f)).toBeNull();
});

function withState(check: (x: { root: string; project: string; pref: string; log: string; bind: () => ReturnType<typeof bindAutoDecisionState> }) => void) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'auto-state-')));
  const project = path.join(root, 'projects', 'fixture'); fs.mkdirSync(project, { recursive: true });
  const pref = path.join(project, 'question-preferences.json'), log = path.join(project, 'question-log.jsonl');
  fs.writeFileSync(pref, JSON.stringify({ [qid]: 'never-ask' }));
  const bind = () => bindAutoDecisionState({ stateRoot: root, projectSlug: 'fixture' }, { GSTACK_STATE_ROOT: root }, 'plan-ceo-review');
  try { check({ root, project, pref, log, bind }); } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
test('state witness binds before launch and observes only completed owned file contents', () => withState(({ log, bind }) => {
  const read = bind(); expect(read()).toBeUndefined();
  const record = state(clone()).records[0]; fs.writeFileSync(log, JSON.stringify(record) + '\n');
  expect(read()?.records).toEqual([record]);
}));
for (const scenario of ['existing-log', 'preference-change', 'malformed-log', 'log-symlink', 'preference-symlink', 'wrong-root', 'path-escape'])
  test(`state binding rejects ${scenario}`, () => withState(({ root, pref, log, bind }) => {
    if (scenario === 'wrong-root' || scenario === 'path-escape') {
      expect(() => bindAutoDecisionState({ stateRoot: root, projectSlug: scenario === 'path-escape' ? '../fixture' : 'fixture' },
        { GSTACK_STATE_ROOT: scenario === 'wrong-root' ? root + '-other' : root }, 'plan-ceo-review')).toThrow(); return;
    }
    if (scenario === 'existing-log') { fs.writeFileSync(log, '{}\n'); expect(bind).toThrow('fresh attempt'); return; }
    const read = bind();
    if (scenario === 'preference-change') fs.writeFileSync(pref, JSON.stringify({ [qid]: 'always-ask' }));
    if (scenario === 'malformed-log') fs.writeFileSync(log, '{');
    if (scenario === 'log-symlink') fs.symlinkSync(pref, log);
    if (scenario === 'preference-symlink') { fs.renameSync(pref, pref + '.real'); fs.symlinkSync(pref + '.real', pref); }
    expect(read()).toBeUndefined();
  }));
});
