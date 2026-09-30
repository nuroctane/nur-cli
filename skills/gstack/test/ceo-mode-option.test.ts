import { describe, expect, test } from 'bun:test';
import { findCeoModeOption, hasPostAnswerCeoPosture, hasNativePostAnswerCeoPosture, nativeCeoModeAnswer, nextCeoModeNavigation, nextCeoPostureContinuation } from './helpers/ceo-mode-option';
import { parseNumberedOptions, stripAnsi, planCountQuestionInput, nativePlanCallFingerprint } from './helpers/claude-pty-runner';
import type { PlanCountTranscript } from './helpers/plan-count-transcript';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { pathToFileURL } from 'node:url';
import captured_ceo_hold_commitment_ar from './fixtures/ceo-hold-commitment-ar.json';
import captured_ceo_hold_posture_ag from './fixtures/ceo-hold-posture-ag.json';
import retainedPreservationCaptures_ceo_hold_posture_ag from './fixtures/ceo-hold-preservation-f359.json';
import captured_ceo_mode_colon_at from './fixtures/ceo-mode-colon-at.json';
import fs_ceo_mode_full_ad from 'node:fs';
import os_ceo_mode_full_ad from 'node:os';
import path_ceo_mode_full_ad from 'node:path';
import { ceoExpansionPacingChoice } from './helpers/ceo-mode-option';
import { ceoExpansionPacingReady } from './helpers/ceo-mode-option';
import { ceoModeSubmissionInput } from './helpers/ceo-mode-option';
import { capturePlanCountQuestion } from './helpers/claude-pty-runner';
import { planCountPrerequisitePick } from './helpers/claude-pty-runner';
import { isNumberedOptionListVisible } from './helpers/claude-pty-runner';
import { isPlanReadyVisible } from './helpers/claude-pty-runner';
import { readPlanCountTranscript } from './helpers/plan-count-transcript';
import type { NativePublicToolEvent } from './helpers/plan-count-transcript';
import type { NativePlanQuestionCall } from './helpers/plan-count-transcript';
import captured_ceo_mode_full_ad from './fixtures/ceo-mode-full-ad.json';
import kindCapture_ceo_mode_full_ad from './fixtures/ceo-expansion-posture-kind-dacc.json';
import pauseCapture_ceo_mode_full_ad from './fixtures/ceo-expansion-pause-6714.json';
import completeInventory_ceo_mode_full_ad from './fixtures/ceo-expansion-complete-inventory-6f.json';
import nativePacing77_ceo_mode_full_ad from './fixtures/ceo-expansion-pacing-77.json';
import captured_ceo_mode_posture_ad from './fixtures/ceo-mode-posture-ad.json';
import captured_ceo_prerequisite_ad_v2 from './fixtures/ceo-prerequisite-ad-v2.json';

describe('CEO mode option matching', () => {
  test('selects option 4 from the failed Claude Code 2.1.257 menu capture', () => {
    // Labels and side-pane residue from the 2026-09-08 paid failure. The
    // option existed; literal includes("SCOPE EXPANSION") could not see it.
    // The failure log preserves parsed labels, not the original raw frame.
    const options = [
      { index: 1, label: 'SELECTIVEEXPANSION┌────────────────────────────────────────────────────────────────────────────────────┐\r    (ecommnded)                │SELECTIVEEXPANSION│' },
      { index: 2, label: 'HOLD SCOPE                  │  Hld scope: eview rigorusly fr failure modes, edg ass, observability.│' },
      { index: 3, label: 'SCOPE REDUCTION              │   Then surface: cherry-pikableadditions you ca Accept/Defer/Skip.│' },
      { index: 4, label: 'SCOPEEXPANSION│Neutralposture:presentopportunities,stateeffort,youdecide.│\r                           │  Good for: substantialfeaturewithsolidfoundation,shippedbeforescopelock.│\r└────────────────────────────────────────────────────────────────────────────────────┘' },
    ];
    expect(findCeoModeOption(options, 'SCOPE EXPANSION')).toBe(4);
    expect(findCeoModeOption(options, 'HOLD SCOPE')).toBe(2);
    expect(findCeoModeOption(options, 'SELECTIVE EXPANSION')).toBe(1);
  });

  test('recognizes the mode question when every label loses its inter-word spaces', () => {
    const frame = stripAnsi([
      '❯ 1. HOLD\x1b[1CSCOPE (recommended)',
      '  2. SELECTIVE\x1b[1CEXPANSION',
      '  3. SCOPE\x1b[1CEXPANSION',
      '  4. SCOPE\x1b[1CREDUCTION',
    ].join('\n'));
    const options = parseNumberedOptions(frame);
    expect(findCeoModeOption(options, 'SCOPE EXPANSION')).toBe(3);
    expect(findCeoModeOption(options, 'SCOPE REDUCTION')).toBe(4);
  });

  test('retains spaced, mixed-case labels and recommendation suffixes', () => {
    expect(findCeoModeOption([
      { index: 1, label: 'Scope Expansion (recommended)' },
      { index: 2, label: 'HOLD SCOPE' },
    ], 'SCOPE EXPANSION')).toBe(1);
  });

  test('still fails on the earlier three-option capture with no expansion target', () => {
    const options = [
      { index: 1, label: 'HOLD SCOPE (recommended)    ┌─────────────────────────────────────────────────────────────┐' },
      { index: 2, label: 'SELECTIVE EXPANSION        │HOLD SCOPE                                             │' },
      { index: 3, label: 'SCOPE REDUCTION             │   Codeiswritten.Makeitbulletproof.│' },
    ];
    expect(() => findCeoModeOption(options, 'SCOPE EXPANSION'))
      .toThrow('target "SCOPE EXPANSION" not in option labels');
  });

  test('does not select another mode because the side pane mentions the target', () => {
    expect(() => findCeoModeOption([
      { index: 1, label: 'HOLD SCOPE │ SCOPE EXPANSION is another option' },
      { index: 2, label: 'SELECTIVEEXPANSION┌ SCOPEEXPANSION' },
    ], 'SCOPE EXPANSION')).toThrow('target "SCOPE EXPANSION" not in option labels');
  });

  test('leaves unrelated navigation questions to the existing driver', () => {
    expect(findCeoModeOption([
      { index: 1, label: 'Review HOLD SCOPE examples' },
      { index: 2, label: 'Choose a plan │ SCOPE EXPANSION' },
    ], 'HOLD SCOPE')).toBeNull();
  });
});

describe('CEO mode navigation replay', () => {
  test('advances different setup questions with the same choices and ignores redraws', () => {
    const seen = new Set<string>();
    const first = '☐Routing\rEnable skill routing?\r❯1.Enable\r2.Skip';
    const next = '☐Learnings\rEnable cross-project learnings?\r❯1.Enable\r2.Skip';
    expect(nextCeoModeNavigation(first, 'HOLD SCOPE', seen).kind).toBe('question');
    expect(nextCeoModeNavigation(first, 'HOLD SCOPE', seen).kind).toBe('wait');
    expect(nextCeoModeNavigation(next, 'HOLD SCOPE', seen).kind).toBe('question');
    expect(seen.size).toBe(2);
  });

  test('navigates the captured unanswered setup tab before submitting, without counting Submit', () => {
    const seen = new Set<string>();
    const partial = [
      '← ☒ Skill routing ☐ Learnings scope ✔ Submit →',
      'Review your answers',
      '⚠You have not answered all questions',
      '❯1.Submit aswers',
      '2Cancel',
    ].join('\r\r');
    expect(nextCeoModeNavigation(partial, 'HOLD SCOPE', seen)).toEqual({ kind: 'submission', input: '\x1b[Z' });
    expect(seen.size).toBe(0);
    const question = '← ☒ Skill routing ☐ Learnings scope ✔ Submit →\rEnable cross-project learnings?\r❯1.Enable\r2.Skip';
    expect(nextCeoModeNavigation(`${partial}\r${question}`, 'HOLD SCOPE', seen).kind).toBe('question');
    const answered = partial.replace('☐ Learnings scope', '☒ Learnings scope').replace('⚠You have not answered all questions', '');
    expect(nextCeoModeNavigation(answered, 'HOLD SCOPE', seen)).toEqual({ kind: 'submission', input: '\r' });
    expect(seen.size).toBe(1);
  });

  test('handles native permission controls before question parsing and dedup', () => {
    const seen = new Set<string>();
    const permission = 'DoyouwanttooverwriteCLAUDE.md?\r❯1.Yes\r2.No\rEsctocancel·Tabtoamend';
    expect(nextCeoModeNavigation(permission, 'HOLD SCOPE', seen)).toEqual({ kind: 'permission', input: '1\r' });
    expect(seen.size).toBe(0);
    const actual = '☐ Approaches\rWhich storage strategy?\r❯1.Server\r2.Local';
    expect(nextCeoModeNavigation(`${permission}\r${actual}`, 'HOLD SCOPE', seen).kind).toBe('question');
  });

  test('file permission lifecycle is shared by navigation and posture without becoming an AUQ', () => {
    const seen = new Set<string>();
    const permission = 'Do you want to overwrite CLAUDE.md?\n❯1.Yes\n2.No\nEsc to cancel · Tab to amend';
    const transcript: PlanCountTranscript = { status: 'missing', calls: [], assistantMessages: [] };
    expect(nextCeoModeNavigation(permission, 'HOLD SCOPE', seen).kind).toBe('permission');
    expect(nextCeoModeNavigation(permission, 'HOLD SCOPE', seen).kind).toBe('wait');
    expect(nextCeoPostureContinuation(permission, transcript, 'HOLD SCOPE', 0, seen, true)).toBeNull();
    const completed = permission + '\n⎿ Added1line\n';
    expect(nextCeoPostureContinuation(completed, transcript, 'HOLD SCOPE', 0, seen, true)).toBeNull();
    expect(nextCeoModeNavigation(completed, 'HOLD SCOPE', seen).kind).toBe('wait');
    const again = completed + permission;
    expect(nextCeoPostureContinuation(again, transcript, 'HOLD SCOPE', 0, seen, true)).toBe('permission');
    expect(nextCeoModeNavigation(again, 'HOLD SCOPE', seen).kind).toBe('wait');
    expect(seen.size).toBe(0);
    const question = '☐ Approaches\nWhich storage strategy?\n❯1.Server\n2.Local';
    expect(nextCeoModeNavigation(again + '\n' + question, 'HOLD SCOPE', seen).kind).toBe('question');
    expect(seen.size).toBe(1);
    // Different sessions retain independent permission state.
    expect(nextCeoModeNavigation(permission, 'HOLD SCOPE', new Set()).kind).toBe('permission');
  });

  test('selects the intended mode from the observed menu, preserving its index', () => {
    const frame = '☐ReviewMode\rWhat review posture should I use?\r❯1.SELECTIVEEXPANSION(recommended)\r2.HOLDSCOPE\r3.SCOPEEXPANSION\r4.SCOPEREDUCTION';
    for (const [mode, index] of [['HOLD SCOPE', 2], ['SCOPE EXPANSION', 3]] as const) {
      const action = nextCeoModeNavigation(frame, mode, new Set());
      expect(action.kind).toBe('mode');
      if (action.kind === 'mode') expect(action.index).toBe(index);
    }
  });
});

describe('CEO posture evidence after mode selection', () => {
  const posture = /\b(rigor|bulletproof|hold\s*scope|maximum\s+rigor)\b/i;
  const menu = [
    '☐ Review mode',
    '❯1.SELECTIVEEXPANSION(recommended)',
    '2.HOLD SCOPE │ Code is written. Make it bulletproof.',
    '3.SCOPE EXPANSION',
    '4.SCOPE REDUCTION',
    'Enter to select · ↑/↓ to navigate',
  ].join('\r');

  test('menu redraw and native selected-option echo cannot satisfy the posture gate', () => {
    expect(posture.test(menu)).toBe(true); // The old unscoped check passed here.
    expect(hasPostAnswerCeoPosture(menu, posture)).toBe(false);
    const answer = "⏺ User answered Claude's questions:\r⎿ · Review mode? → HOLD SCOPE";
    expect(hasPostAnswerCeoPosture(`${menu}\r${answer}`, posture)).toBe(false);
    expect(hasPostAnswerCeoPosture('❯ HOLD SCOPE\r● HOLD SCOPE', posture)).toBe(false);
    expect(hasPostAnswerCeoPosture('● Selected option: HOLD SCOPE', posture)).toBe(false);
    expect(hasPostAnswerCeoPosture('● HOLD SCOPE\r✶ Honking… (5s · ↓ 300 tokens)', posture)).toBe(false);
  });

  test('assistant output must itself contain the existing posture evidence', () => {
    expect(hasPostAnswerCeoPosture(`${menu}\r● I will inspect the plan now.`, posture)).toBe(false);
    expect(hasPostAnswerCeoPosture('⏺ Read(plan-ceo-review/SKILL.md)\r  Review with maximum rigor.', posture)).toBe(false);
    expect(hasPostAnswerCeoPosture('● high · /effort\rHOLD SCOPE', posture)).toBe(false);
    expect(hasPostAnswerCeoPosture(`● I will inspect the plan now.\r${menu}`, posture)).toBe(false);
  });

  test('accepts new assistant posture after the answered-question echo, including wrapped prose', () => {
    const answer = "⏺UseransweredClaude'squestions:\r⎿Reviewmode?→HOLDSCOPE";
    expect(hasPostAnswerCeoPosture(`${answer}\r● HOLD SCOPE. I will review the existing scope for failure modes.`, posture)).toBe(true);
    expect(hasPostAnswerCeoPosture(`${answer}\r⏺\rI will apply maximum rigor\rto the agreed scope.`, posture)).toBe(true);
    const expansion = /\b(expansion|10x|delight|dream|cathedral|opt[\s-]?in)\b/i;
    expect(hasPostAnswerCeoPosture(`${answer}\r● I will explore expansion opportunities that improve the saved-view workflow.`, expansion)).toBe(true);
  });
});

describe('native CEO mode posture evidence', () => {
  const selectedAt = Date.parse('2026-09-08T15:43:28.000Z');
  const posture = /\b(rigor|bulletproof|hold\s*scope|maximum\s+rigor)\b/i;
  function transcript(text: string, answer = 'HOLD SCOPE'): PlanCountTranscript {
    // Actual question/options/answer shape from targeted-a's false-negative
    // HOLD SCOPE run. The native answer selected option3 correctly.
    const question = 'Which review mode should I use for this plan? <gstack-qid:plan-ceo-review-mode-selection>';
    return { status: 'ready', calls: [{
      sessionId: 'mode-session', toolUseId: 'mode-call', answered: true,
      answeredAt: '2026-09-08T15:43:30.405Z', answers: { [question]: answer },
      questions: [{ header: 'Review mode', question, options: [
        { label: 'SELECTIVE EXPANSION (Recommended)' }, { label: 'SCOPE EXPANSION' },
        { label: 'HOLD SCOPE' }, { label: 'SCOPE REDUCTION' },
      ] }],
    }], assistantMessages: [{ sessionId: 'mode-session', timestamp: '2026-09-08T15:44:01.150Z', text }] };
  }

  test('recognizes the retained native answer followed by actual HOLD SCOPE analysis', () => {
    const captured = 'HOLD SCOPE mode confirmed. Running Step 0D analysis, then reading the review sections file.\n\n**0D — HOLD SCOPE Analysis**\n\n**Complexity check:**\nThe plan introduces: 1 DB migration, 1 SavedView model, 1 CRUD API module (~4 endpoints), 1 view picker UI component, and integration into the existing filter UI.';
    expect(hasNativePostAnswerCeoPosture(transcript(captured), 'HOLD SCOPE', posture, selectedAt)).toBe(true);
  });

  test('wrong, missing, failed, or earlier mode answers cannot establish target routing', () => {
    const text = 'I will apply maximum rigor to the existing plan.';
    expect(hasNativePostAnswerCeoPosture(transcript(text, 'SCOPE EXPANSION'), 'HOLD SCOPE', posture, selectedAt)).toBe(false);
    for (const change of [{ answered: false }, { failed: true }, { answers: {} }, { answeredAt: undefined }]) {
      const t = transcript(text); Object.assign(t.calls[0]!, change);
      expect(hasNativePostAnswerCeoPosture(t, 'HOLD SCOPE', posture, selectedAt)).toBe(false);
    }
    expect(hasNativePostAnswerCeoPosture(transcript(text), 'HOLD SCOPE', posture, selectedAt + 10000)).toBe(false);
  });

  test('prior or foreign assistant prose, a menu, source quotation, and bare confirmation remain insufficient', () => {
    for (const text of [
      '', 'HOLD SCOPE', '**HOLD SCOPE mode confirmed.**',
      'Which mode?\n1. SELECTIVE EXPANSION\n2. HOLD SCOPE\n3. SCOPE EXPANSION',
      '```markdown\nReview with maximum rigor.\n```',
      '> Review with maximum rigor.',
      'Read(SKILL.md)\nReview with maximum rigor.',
    ]) expect(hasNativePostAnswerCeoPosture(transcript(text), 'HOLD SCOPE', posture, selectedAt)).toBe(false);
    for (const change of [{ timestamp: '2026-09-08T15:43:29.000Z' }, { sessionId: 'other-session' }]) {
      const t = transcript('I will apply maximum rigor.'); Object.assign(t.assistantMessages[0]!, change);
      expect(hasNativePostAnswerCeoPosture(t, 'HOLD SCOPE', posture, selectedAt)).toBe(false);
    }
    expect(hasNativePostAnswerCeoPosture({ status: 'missing', calls: [], assistantMessages: [] }, 'HOLD SCOPE', posture, selectedAt)).toBe(false);
  });

  test('continuation requires the confirmed target and permits at most one fresh downstream question', () => {
    const t = transcript('');
    const fresh = '☐ Architecture\nD4 — Guard the member-scoped lookup?\n❯1.Add the guard\n2.Defer';
    const mode = '☐ Review mode\nWhich review mode?\n❯1.HOLD SCOPE\n2.SCOPE EXPANSION';
    const seen = new Set<string>();
    expect(nextCeoPostureContinuation(mode, t, 'HOLD SCOPE', selectedAt, seen, false)).toBeNull();
    expect(nextCeoPostureContinuation(fresh, transcript('', 'SCOPE EXPANSION'), 'HOLD SCOPE', selectedAt, seen, false)).toBeNull();
    expect(nextCeoPostureContinuation(fresh, t, 'HOLD SCOPE', selectedAt, seen, false)).toBe('question');
    expect(nextCeoPostureContinuation(fresh, t, 'HOLD SCOPE', selectedAt, seen, false)).toBeNull();
    expect(nextCeoPostureContinuation(fresh.replace('member-scoped', 'project-scoped'), t, 'HOLD SCOPE', selectedAt, seen, true)).toBeNull();
    expect(nativeCeoModeAnswer(t, 'HOLD SCOPE', selectedAt)?.toolUseId).toBe('mode-call');
    const permission = 'DoyouwanttooverwriteCLAUDE.md?\n❯1.Yes\n2.No\nEsctocancel·Tabtoamend';
    expect(nextCeoPostureContinuation(permission, t, 'HOLD SCOPE', selectedAt, seen, false)).toBe('permission');
    expect(nextCeoPostureContinuation(permission, t, 'HOLD SCOPE', selectedAt, seen, false)).toBeNull();
  });

  test.skipIf(process.platform === 'win32')('one downstream answer releases delayed native prose without passing on the streamed menu', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ceo-posture-flush-'));
    const fake = path.join(dir, 'fake-claude');
    const worker = path.join(dir, 'worker.ts');
    const recordFile = path.join(dir, 'events.jsonl');
    const resultFile = path.join(dir, 'result.json');
    fs.writeFileSync(fake, `#!${process.execPath}\n` + String.raw`
import * as fs from 'node:fs';
import * as path from 'node:path';
const record = event => fs.appendFileSync(process.env.POSTURE_RECORD, JSON.stringify(event) + '\n');
record({type:'startup', pid:process.pid});
const sessionId = 'fake-mode-session';
const dir = path.join(process.env.CLAUDE_CONFIG_DIR, 'projects', 'fixture');
fs.mkdirSync(dir, {recursive:true});
const write = (role, content, extra = {}) => fs.appendFileSync(path.join(dir, sessionId + '.jsonl'), JSON.stringify({
  sessionId, cwd:process.cwd(), isSidechain:false, timestamp:new Date().toISOString(), message:{role,content}, ...extra,
}) + '\n');
const question = 'Which mode?';
write('assistant', [{type:'tool_use', id:'mode', name:'AskUserQuestion', input:{questions:[{header:'Mode', question,
  options:[{label:'HOLD SCOPE'},{label:'SCOPE EXPANSION'}]}]}}]);
write('user', [{type:'tool_result', tool_use_id:'mode', content:'Answered.'}], {toolUseResult:{answers:{[question]:'SCOPE EXPANSION'}}});
process.stdin.setRawMode?.(true);
let answered = false;
process.stdin.on('data', data => {
  record({type:'input', data:data.toString()});
  if (data.toString().includes('\r') && !answered) {
    answered = true;
    write('assistant', [{type:'text', text:'I will explore expansion opportunities that improve saved project views.'}]);
    process.stdout.write('\nPOSTURE_FLUSHED\n');
  }
});
process.stdout.write('POSTURE_READY\n● I will explore expansion opportunities.\n☐ Expansion 1\nD4 — Add shared project views?\n❯1.Add to scope\n2.Defer\n');
process.on('SIGINT', () => process.exit(0));
process.stdin.resume();
`);
    fs.chmodSync(fake, 0o755);
    const moduleUrl = (name: string) => pathToFileURL(path.resolve(import.meta.dir, 'helpers', name)).href;
    fs.writeFileSync(worker, `
import { launchClaudePty, selectPtyNumberedOption } from ${JSON.stringify(moduleUrl('claude-pty-runner.ts'))};
import { readPlanCountTranscript } from ${JSON.stringify(moduleUrl('plan-count-transcript.ts'))};
import { hasNativePostAnswerCeoPosture, nextCeoPostureContinuation } from ${JSON.stringify(moduleUrl('ceo-mode-option.ts'))};
const started = Date.now();
const session = await launchClaudePty({cwd:${JSON.stringify(dir)}, timeoutMs:5000, env:{POSTURE_RECORD:${JSON.stringify(recordFile)}}});
try {
  await session.waitFor('POSTURE_READY', {timeoutMs:2000, pollMs:20});
  const read = () => readPlanCountTranscript(session.hermeticConfigDir, ${JSON.stringify(dir)});
  const posture = /\\b(expansion|10x|delight|dream|cathedral|opt[\\s-]?in)\\b/i;
  const before = hasNativePostAnswerCeoPosture(read(), 'SCOPE EXPANSION', posture, started);
  const action = nextCeoPostureContinuation(session.visibleText(), read(), 'SCOPE EXPANSION', started, new Set(), false);
  if (action === 'question') await selectPtyNumberedOption(session, 1);
  await session.waitFor('POSTURE_FLUSHED', {timeoutMs:2000, pollMs:20});
  const after = hasNativePostAnswerCeoPosture(read(), 'SCOPE EXPANSION', posture, started);
  await Bun.write(${JSON.stringify(resultFile)}, JSON.stringify({before, action, after}));
} finally { await session.close(); }
`);
    const child = Bun.spawn([process.execPath, worker], {
      env: { ...process.env, BROWSE_TERMINAL_BINARY: fake, EVALS_HERMETIC: '1' }, stdout: 'pipe', stderr: 'pipe',
    });
    const timer = setTimeout(() => child.kill('SIGKILL'), 8000);
    try {
      const [code, stdout, stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
      expect(code, stdout + stderr).toBe(0);
      expect(JSON.parse(fs.readFileSync(resultFile, 'utf8'))).toEqual({before:false, action:'question', after:true});
      const events = fs.readFileSync(recordFile, 'utf8').trim().split('\n').map(line => JSON.parse(line));
      expect(events.filter(e => e.type === 'input').map(e => e.data).join('')).toBe('1\r');
      expect(() => process.kill(events[0].pid, 0)).toThrow();
    } finally {
      clearTimeout(timer); child.kill('SIGKILL');
      if (fs.existsSync(recordFile)) {
        const first = JSON.parse(fs.readFileSync(recordFile, 'utf8').split('\n')[0]!);
        try { process.kill(first.pid, 'SIGKILL'); } catch { /* already reaped */ }
      }
      fs.rmSync(dir, {recursive:true, force:true});
    }
  }, 10_000);
});


const sidebarModeScreen = fs.readFileSync(path.join(import.meta.dir, 'fixtures/ceo-mode-preview-aa-screen.txt'), 'utf8');
// The AA first SCOPE pane was retained in the terminal failure, but its native
// mode call never flushed. This pending call is synthetic identity coverage.
function sidebarPendingMode() {
  return {sessionId:'sidebar-fixture', toolUseId:'sidebar-mode', answered:false, failed:false,
    questions:[{header:'Review mode', question:'Which review mode should I use for this plan?', multiSelect:false,
      options:[
        {label:'SELECTIVE EXPANSION — baseline + cherry-pick (Recommended)'},
        {label:'HOLD SCOPE — maximum rigor, no expansions'},
        {label:'SCOPE EXPANSION — dream big'},
        {label:'SCOPE REDUCTION — strip to essentials'},
      ]}]};
}

describe('AA mode sidebar preview protocol', () => {
  test('submits the independently retained CEO count pane when Notes sits on a wrapped option line', () => {
    const screen=fs.readFileSync(path.join(import.meta.dir,'fixtures/ceo-count-mode-preview-aa-screen.txt'),'utf8');
    const action=nextCeoModeNavigation(screen,'HOLD SCOPE',new Set());
    expect(action.kind).toBe('mode');
    if(action.kind!=='mode')throw new Error('Expected mode');
    expect(action.index).toBe(1);
    expect(planCountQuestionInput(screen,action.question,1)).toBe('1\r');
    const native=nativePlanCallFingerprint(sidebarPendingMode(),0,true);
    for(const altered of [screen.replace('    bigger','  bigger'),screen.replace('  4. SCOPE EXPANSION — dream','  Unrelated unnumbered message'),screen.replace('    bigger','    bigger│')]) {
      expect(planCountQuestionInput(altered,native,1)).toBe('1');
    }
  });


  test('submits all four offered modes from the exact pane, with or without pending metadata', () => {
    for (const [mode,index] of [['SELECTIVE EXPANSION',1],['HOLD SCOPE',2],['SCOPE EXPANSION',3],['SCOPE REDUCTION',4]] as const) {
      for (const pending of [undefined,sidebarPendingMode()]) {
        const action=nextCeoModeNavigation(sidebarModeScreen,mode,new Set(),pending);
        expect(action.kind).toBe('mode');
        if(action.kind!=='mode')throw new Error('Expected mode');
        expect(action.index).toBe(index);
        expect(action.question.nativeCall).toBe(pending);
        expect(planCountQuestionInput(sidebarModeScreen,action.question,index)).toBe(`${index}\r`);
      }
    }
  });

  test('requires a complete aligned preview and exact notes protocol', () => {
    const fp=nativePlanCallFingerprint(sidebarPendingMode(),0,true);
    for(const frame of [
      sidebarModeScreen.replace('Notes: press n to add notes','Notes: press n to run a command'),
      sidebarModeScreen.replace('      Notes:','     Notes:'),
      sidebarModeScreen.replace(/┌─+┐/,'no preview box'),
      sidebarModeScreen.replace(/└─+┘/,'no preview bottom'),
      sidebarModeScreen.replace('└──','└─'),
      sidebarModeScreen+'\n☐ Next question\nWhat now?\n❯ 1. Continue\n  2. Stop\nEnter to select · ↑/↓ to navigate · n to add notes · Esc to cancel',
      sidebarModeScreen.replace(' · n to add notes',''),
      sidebarModeScreen.replace(' · Esc to cancel',''),
      sidebarModeScreen.replace('☐ Review mode','quoted Review mode'),
      sidebarModeScreen.replace('n to add notes · ','n to add notes · n to add notes · '),
      sidebarModeScreen+'\nUnrelated active prompt',
      sidebarModeScreen.split('\n').map(line=>'> '+line).join('\n'),
    ])expect(planCountQuestionInput(frame,fp,3)).toBe('3');
    const checkbox=sidebarPendingMode();checkbox.questions[0]!.multiSelect=true;
    expect(planCountQuestionInput(sidebarModeScreen,nativePlanCallFingerprint(checkbox,0,true),3)).toBe('3');
  });

  test('keeps the actual retry missing-target failure and independent answer/posture gates', () => {
    const omitted=[
      {index:1,label:'HOLD SCOPE — make the client-side plan bulletproof (Recommended)'},
      {index:2,label:'SELECTIVE EXPANSION — hold core scope but surface cherry-pick options'},
      {index:3,label:'SCOPE REDUCTION — cut to absolute minimum'},
      {index:4,label:'Type something.'},{index:5,label:'Chat about this'},
    ];
    expect(()=>findCeoModeOption(omitted,'SCOPE EXPANSION')).toThrow('target "SCOPE EXPANSION" not in option labels');
    const t:PlanCountTranscript={status:'ready',calls:[sidebarPendingMode()],assistantMessages:[
      {sessionId:'sidebar-fixture',timestamp:new Date().toISOString(),text:'I will explore expansion opportunities.'},
    ]};
    expect(hasNativePostAnswerCeoPosture(t,'SCOPE EXPANSION',/expansion/i,0)).toBe(false);
    const foreign=sidebarPendingMode();foreign.questions[0]!.header='Other';
    const action=nextCeoModeNavigation(sidebarModeScreen,'SCOPE EXPANSION',new Set(),foreign);
    expect(action.kind).toBe('mode');
    if(action.kind==='mode')expect(action.question.nativeCall).toBeUndefined();
  });
});

test.skipIf(process.platform==='win32')('AA sidebar fake CLI requires submission before native mode posture',async()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'mode-sidebar-'));
  const fake=path.join(dir,'fake-claude');const worker=path.join(dir,'worker.ts');const output=path.join(dir,'result.json');
  const cases=[
    {mode:'SELECTIVE EXPANSION',index:1},{mode:'HOLD SCOPE',index:2},
    {mode:'SCOPE EXPANSION',index:3},{mode:'SCOPE REDUCTION',index:4},
    {mode:'SCOPE EXPANSION',index:3,digitOnly:true},
  ].map((item,i)=>({...item,cwd:path.join(dir,String(i)),record:path.join(dir,`${i}.jsonl`)}));
  for(const item of cases)fs.mkdirSync(item.cwd);
  fs.writeFileSync(fake,`#!${process.execPath}\n`+String.raw`
import fs from 'node:fs';import path from 'node:path';
const item=JSON.parse(process.env.SIDEBAR_REPLAY);const record=row=>fs.appendFileSync(item.record,JSON.stringify(row)+'\n');
const sid='sidebar-'+process.pid;const file=path.join(process.env.CLAUDE_CONFIG_DIR,'projects',sid,sid+'.jsonl');fs.mkdirSync(path.dirname(file),{recursive:true});
const native=(role,content,extra={})=>fs.appendFileSync(file,JSON.stringify({cwd:process.cwd(),sessionId:sid,isSidechain:false,timestamp:new Date().toISOString(),message:{role,content},...extra})+'\n');
record({type:'start',pid:process.pid});native('assistant',[{type:'text',text:'Review preview: expansion, rigor, and reduction.'}]);
let focused=1;let done=false;process.stdin.setRawMode?.(true);
process.stdin.on('data',data=>{
 const input=data.toString();record({type:'input',input});
 if(done){record({type:'unexpected',input});return;}
 const digit=/[1-4]/.exec(input)?.[0];
 if(digit)setTimeout(()=>{focused=Number(digit);record({type:'focus',focused});},25);
 if(!input.includes('\r'))return;done=true;
 const answer=item.question.options[focused-1].label;
 native('assistant',[{type:'tool_use',name:'AskUserQuestion',id:'mode',input:{questions:[item.question]}}]);
 native('user',[{type:'tool_result',tool_use_id:'mode',content:'Answered'}],{toolUseResult:{answers:{[item.question.question]:answer}}});
 record({type:'answer',answer});
 setTimeout(()=>{native('assistant',[{type:'text',text:'I will apply '+answer+' to assess this plan thoroughly.'}]);process.stdout.write('\r\nANSWER_READY\r\n');},25);
});
process.stdout.write('\x1b[2J\x1b[H'+item.screen.replaceAll('\n','\r\n'));process.on('SIGINT',()=>process.exit(0));process.stdin.resume();
`);fs.chmodSync(fake,0o755);
  const moduleUrl=(name:string)=>pathToFileURL(path.join(import.meta.dir,'helpers',name)).href;
  fs.writeFileSync(worker,`
import {launchClaudePty,selectPtyNumberedOption,planCountQuestionInput} from ${JSON.stringify(moduleUrl('claude-pty-runner.ts'))};
import {nextCeoModeNavigation,hasNativePostAnswerCeoPosture} from ${JSON.stringify(moduleUrl('ceo-mode-option.ts'))};
import {readPlanCountTranscript} from ${JSON.stringify(moduleUrl('plan-count-transcript.ts'))};
const cases=${JSON.stringify(cases)};const screen=${JSON.stringify(sidebarModeScreen)};const question=${JSON.stringify(sidebarPendingMode().questions[0])};
const results=await Promise.all(cases.map(async item=>{
 const session=await launchClaudePty({cwd:item.cwd,observeScreen:true,timeoutMs:5000,env:{SIDEBAR_REPLAY:JSON.stringify({...item,screen,question})}});
 try{
  await session.waitFor('Which review mode',{timeoutMs:2000,pollMs:20});
  const visible=await session.currentScreen();const action=nextCeoModeNavigation(visible,item.mode,new Set());
  if(action.kind!=='mode')throw new Error('Mode not captured');
  const started=Date.now();const before=readPlanCountTranscript(session.hermeticConfigDir,item.cwd);
  const beforeMatched=hasNativePostAnswerCeoPosture(before,item.mode,new RegExp(item.mode,'i'),started);
  const input=item.digitOnly?String(action.index):planCountQuestionInput(visible,action.question,action.index);
  if(input.includes('\\r'))await selectPtyNumberedOption(session,action.index);else session.send(input);
  if(!item.digitOnly)await session.waitFor('ANSWER_READY',{timeoutMs:1500,pollMs:20});else await Bun.sleep(150);
  const transcript=readPlanCountTranscript(session.hermeticConfigDir,item.cwd);
  return {mode:item.mode,digitOnly:!!item.digitOnly,index:action.index,input,beforeMatched,matched:hasNativePostAnswerCeoPosture(transcript,item.mode,new RegExp(item.mode,'i'),started)};
 }finally{await session.close();}
}));await Bun.write(${JSON.stringify(output)},JSON.stringify(results));
`);
  const child=Bun.spawn([process.execPath,worker],{env:{...process.env,BROWSE_TERMINAL_BINARY:fake,EVALS_HERMETIC:'1'},stdout:'pipe',stderr:'pipe'});
  const timer=setTimeout(()=>child.kill('SIGKILL'),10000);
  try{
    const [code,out,err]=await Promise.all([child.exited,new Response(child.stdout).text(),new Response(child.stderr).text()]);
    expect(code,out+err).toBe(0);
    const results=JSON.parse(fs.readFileSync(output,'utf8'));
    for(const [i,item] of cases.entries()){
      expect(results[i]).toEqual({mode:item.mode,digitOnly:!!item.digitOnly,index:item.index,input:String(item.index)+(item.digitOnly?'':'\r'),beforeMatched:false,matched:!item.digitOnly});
      const rows=fs.readFileSync(item.record,'utf8').trim().split('\n').map(line=>JSON.parse(line));
      expect(rows.filter(row=>row.type==='input').map(row=>row.input)).toEqual(item.digitOnly?[String(item.index)]:[String(item.index),'\r']);
      expect(rows.filter(row=>row.type==='answer').length).toBe(item.digitOnly?0:1);
      expect(rows.some(row=>row.type==='unexpected')).toBe(false);
      expect(()=>process.kill(rows[0].pid,0)).toThrow();
    }
  }finally{
    clearTimeout(timer);child.kill('SIGKILL');await child.exited;
    for(const item of cases){
      if(!fs.existsSync(item.record))continue;const first=JSON.parse(fs.readFileSync(item.record,'utf8').split('\n')[0]!);
      try{
        const argv=process.platform==='linux'?fs.readFileSync('/proc/'+first.pid+'/cmdline','utf8').split('\0'):Bun.spawnSync(['ps','-p',String(first.pid),'-o','command='],{timeout:1000}).stdout.toString().trim().split(/\s+/);
        if(argv.includes(fake))process.kill(first.pid,'SIGKILL');
      }catch{/* owned child already closed */}
    }
    fs.rmSync(dir,{recursive:true,force:true});
  }
},12000);

describe('ceo-hold-commitment-ar', () => {
const captured = captured_ceo_hold_commitment_ar;
const posture = /\b(rigor|bulletproof|hold\s*scope|maximum\s+rigor)\b/i;
const original = captured.transcript.assistantMessages[0]!.text;
const replay = () => structuredClone(captured.transcript) as PlanCountTranscript;
const matches = (transcript = replay()) => hasNativePostAnswerCeoPosture(
  transcript, 'HOLD SCOPE', posture, captured.selectionStartedAt,
);
const withText = (text: string) => { const t = replay(); t.assistantMessages[0]!.text = text; return matches(t); };

test('the actual failed attempt adopted HOLD through scope, hardening and exclusion', () => {
  expect(captured.provenance.actualState).toBe('failed');
  expect(nativeCeoModeAnswer(replay(), 'HOLD SCOPE', captured.selectionStartedAt)?.toolUseId)
    .toBe('toolu_01E1HnYjRCz79826bo7nNnoK');
  expect(posture.test(original)).toBe(false);
  expect(matches()).toBe(true);
  // This is prospective posture recognition, not evidence of completed work.
  for (const prefix of ["I'm keeping", 'I am keeping', 'I will keep', "We'll keep", 'We will keep', 'We are keeping']) {
    expect(withText(original.replace("I'll keep", prefix)), prefix).toBe(true);
  }
  expect(withText(original.replace("I'll", 'I’ll').replace("PLAN.md's", 'PLAN.md’s'))).toBe(true);
});

test('explicitly future, conditional and quoted statements are not adopted current posture', () => {
  for (const text of [
    original.replace("I'll keep", 'I will later keep'),
    original.replace("I'll keep", 'I will eventually keep'),
    original.replace("I'll keep", 'I would keep'),
    original.replace("I'll keep", 'I may keep'),
    original.replace("I'll keep", "I'll not keep"),
    original.replace('scope fixed', 'scope tomorrow fixed'),
    original.replace('production visibility', 'production visibility next week'),
    original.replace('production visibility', 'production visibility tomorrow'),
    ...['after approval', 'once approved', 'when approved', 'after launch', 'pending approval', 'subject to approval'].map(when =>
      original.replace('production visibility', 'production visibility ' + when)),
    'Later, ' + original, 'If you approve, ' + original,
    'Hypothetical scenario. ' + original, 'Example only: ' + original,
    '"' + original + '"', '> ' + original,
    '```text\n' + original + '\n```', '~~~text\n' + original + '\n~~~',
    'Read(file)\n' + original, 'The user said: ' + original,
  ]) expect(withText(text), text).toBe(false);
});

test('all three obligations remain concrete and bound to the selected plan', () => {
  for (const [from, to] of [
    ['PLAN.md', 'OTHER.md'], ['PLAN.md', 'archive/PLAN.md'],
    ["PLAN.md's four bullets plus the approved schema", 'the future expanded plan'],
    ['plus the approved schema', 'plus a new unapproved schema'],
    [', pressure-testing every stated behavior for failure modes, errors, tests, and production visibility', ''],
    ['errors, tests, and production visibility', 'word choice and formatting'],
    ['while deferring anything extra rather than adding it silently', 'while adding anything extra'],
    ['while deferring', 'while not deferring'], ['pressure-testing', 'not pressure-testing'],
  ]) expect(withText(original.replace(from!, to!)), from).toBe(false);
  for (const contextChange of [
    (text: string) => text.replace('PLAN.md', 'PLAN.md and OTHER.md'),
    (text: string) => text.replace('schema) approved', 'schema) not approved'),
    (text: string) => text.replace('schema) approved', 'schema) discussed'),
    ...['approved if the user agrees', 'approved once migration finishes', 'approved pending migration', 'approved subject to migration'].map(status =>
      (text: string) => text.replace('schema) approved', 'schema) ' + status)),
  ]) {
    const t = replay(); const q = t.calls[0]!.questions[0]!; const prior = q.question;
    q.question = contextChange(q.question); t.calls[0]!.answers = { [q.question]: t.calls[0]!.answers![prior]! };
    expect(matches(t)).toBe(false);
  }
});

test('current corrections withdraw a commitment; quoted corrections do not', () => {
  for (const correction of [
    'Correction: I will expand scope to include defaults.',
    'Correction: I will not keep scope fixed to these requirements.',
    'Correction: I am no longer keeping scope to those requirements.',
    'The formerly excluded additions are in scope.',
  ]) {
    expect(withText(original + '\n\n' + correction), correction).toBe(false);
    for (const quote of ['> ' + correction, '```text\n' + correction + '\n```', '~~~text\n' + correction + '\n~~~', 'A quotation: "' + correction + '"']) {
      expect(withText(original + '\n\n' + quote), quote).toBe(true);
    }
  }
});

test('native selection, session and timestamp evidence remain required', () => {
  for (const change of [
    (t: PlanCountTranscript) => { t.status = 'missing'; },
    (t: PlanCountTranscript) => { t.calls[0]!.answered = false; },
    (t: PlanCountTranscript) => { t.calls[0]!.failed = true; },
    (t: PlanCountTranscript) => { t.calls[0]!.answeredAt = new Date(captured.selectionStartedAt - 1).toISOString(); },
    (t: PlanCountTranscript) => { t.calls[0]!.answers![t.calls[0]!.questions[0]!.question] = 'Scope expansion'; },
    (t: PlanCountTranscript) => { t.calls[0]!.answers![t.calls[0]!.questions[0]!.question] = 'Unknown'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.sessionId = 'foreign'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = t.calls[0]!.answeredAt!; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = 'invalid'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = new Date(Date.now() + 60_000).toISOString(); },
    (t: PlanCountTranscript) => { t.assistantMessages = []; },
  ]) { const t = replay(); change(t); expect(matches(t)).toBe(false); }
});
});

describe('ceo-hold-posture-ag', () => {
const captured = captured_ceo_hold_posture_ag;
const retainedPreservationCaptures = retainedPreservationCaptures_ceo_hold_posture_ag;
const posture = /\b(rigor|bulletproof|hold\s*scope|maximum\s+rigor)\b/i;
const original = captured.transcript.assistantMessages[0]!.text;
const replay = () => structuredClone(captured.transcript) as PlanCountTranscript;
const matches = (transcript = replay()) => hasNativePostAnswerCeoPosture(
  transcript, 'HOLD SCOPE', posture, captured.selectionStartedAt,
);

test('the captured selected HOLD scope lock and hardening establish posture without a keyword', () => {
  const transcript = replay();
  expect(captured.provenance.actualState).toBe('failed');
  expect(nativeCeoModeAnswer(transcript, 'HOLD SCOPE', captured.selectionStartedAt)?.toolUseId)
    .toBe('toolu_011bt3yabPDSEsPNm97EhqV4');
  expect(posture.test(original)).toBe(false);
  expect(matches(transcript)).toBe(true);
});

test('ordinary current scope declarations preserve the same three obligations', () => {
  for (const text of [
    original.replace("I'm locking", 'I will lock'),
    original.replace("I'm locking", "I'll lock"),
    original.replace("I'm locking", 'We are keeping').replace('the four PLAN.md bullets from approach B', 'the agreed plan')
      .replace('flagging anything beyond', 'treating everything outside').replace('hunting', 'checking'),
    original.replace("I'm locking", 'I am holding').replace('four PLAN.md bullets from approach B', 'PLAN.md requirements')
      .replace('flagging', 'marking').replace('hunting', 'looking'),
    original.replace("I'm", 'I’m').replace('PLAN.md', '**PLAN.md**'),
  ]) {
    const transcript = replay(); transcript.assistantMessages[0]!.text = text;
    expect(matches(transcript)).toBe(true);
  }
});

test('deferred commitments, conditions and quotation cannot establish the current posture', () => {
  for (const text of [
    original.replace("I'm locking", 'I would lock'),
    original.replace("I'm locking", 'I will later lock'),
    'If you approve, ' + original,
    'Later, ' + original,
    'Example only: ' + original,
    'An unproven hypothesis: ' + original,
    'Example only. ' + original,
    '"' + original + '"',
    '> ' + original,
    '```text\n' + original + '\n```',
    '~~~~\n' + original + '\n~~~~',
    'Read(file)\n' + original,
    'The user said: ' + original,
    original.replace('and hunting', 'and not hunting'),
  ]) {
    const transcript = replay(); transcript.assistantMessages[0]!.text = text;
    expect(matches(transcript), text).toBe(false);
  }
});

test('all three obligations refer to the selected current scope', () => {
  for (const text of [
    original.replace('PLAN.md', 'OTHER.md'),
    original.replace('PLAN.md', 'archive/PLAN.md'),
    original.replace('the four PLAN.md bullets from approach B', 'the future expanded plan'),
    original.replace('the four PLAN.md bullets from approach B', 'the two imagined requirements'),
    original.replace('out of scope', 'in scope'),
    original.replace('as out of scope', 'as not out of scope'),
    original.replace('flagging anything beyond that (defaults, sharing, deep links) as out of scope, and ', ''),
    original.replace(/, and hunting[^.]+\./, '.'),
    original.replace('constraints, error handling, UI edge cases, access-rule leaks', 'word choice and formatting'),
    original + ' I am expanding scope to include a new feature.',
    original + ' I am adding extra features to scope.',
  ]) {
    const transcript = replay(); transcript.assistantMessages[0]!.text = text;
    expect(matches(transcript), text).toBe(false);
  }
  const ambiguous = replay();
  const question = ambiguous.calls[0]!.questions[0]!;
  const oldQuestion = question.question;
  question.question = question.question.replace('reviewing PLAN.md', 'reviewing PLAN.md and OTHER.md');
  ambiguous.calls[0]!.answers = { [question.question]: ambiguous.calls[0]!.answers![oldQuestion]! };
  expect(matches(ambiguous)).toBe(false);
});

test('explicit later corrections withdraw scope locking, while quoted examples do not', () => {
  const corrections = [
    'Correction: the previously excluded defaults, sharing, and deep links are now in scope.',
    'Correction: I am no longer locking scope to those requirements.',
    'I am not keeping scope to those requirements.',
    'The formerly excluded additions are in scope.',
  ];
  for (const correction of corrections) {
    const transcript = replay();
    transcript.assistantMessages[0]!.text = original + '\n\n' + correction;
    expect(matches(transcript), correction).toBe(false);
    for (const quote of ['> ' + correction, '```text\n' + correction + '\n```',
      '~~~text\n' + correction + '\n~~~', 'An example of withdrawn wording is: "' + correction + '"']) {
      transcript.assistantMessages[0]!.text = original + '\n\n' + quote;
      expect(matches(transcript), quote).toBe(true);
    }
  }
});

test('only a real selected HOLD answer followed by its own public statement supplies evidence', () => {
  for (const change of [
    (t: PlanCountTranscript) => { t.status = 'missing'; },
    (t: PlanCountTranscript) => { t.calls[0]!.answered = false; },
    (t: PlanCountTranscript) => { t.calls[0]!.failed = true; },
    (t: PlanCountTranscript) => { t.calls[0]!.answeredAt = new Date(captured.selectionStartedAt - 1).toISOString(); },
    (t: PlanCountTranscript) => { t.calls[0]!.answers![t.calls[0]!.questions[0]!.question] = 'Scope Expansion'; },
    (t: PlanCountTranscript) => { t.calls[0]!.answers![t.calls[0]!.questions[0]!.question] = 'Unknown'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.sessionId = 'foreign'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = t.calls[0]!.answeredAt!; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = 'invalid'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = new Date(Date.now() + 60_000).toISOString(); },
    (t: PlanCountTranscript) => { t.assistantMessages = []; },
  ]) {
    const transcript = replay(); change(transcript); expect(matches(transcript)).toBe(false);
  }
  const expansion = replay();
  expansion.calls[0]!.answers![expansion.calls[0]!.questions[0]!.question] = 'Scope Expansion';
  expect(hasNativePostAnswerCeoPosture(expansion, 'SCOPE EXPANSION', posture, captured.selectionStartedAt)).toBe(false);
});
// Exact public AY parent narration after the answered HOLD SCOPE mode AUQ.
// Its native ownership controls use the existing PLAN.md / approved-approach-B fixture.
const ambiguityNarration = "I'm holding strictly to the plan's approved scope (Approach B, private-only views) and flagging any ambiguities the sketch leaves undecided as targeted questions rather than expanding scope. First up: what happens when a saved view's filters reference something that's been deleted.\n\n";
const ambiguityReplay = () => {
  const transcript = replay();
  transcript.assistantMessages[0]!.text = ambiguityNarration;
  return transcript;
};
const ambiguityMatches = (text = ambiguityNarration) => {
  const transcript = ambiguityReplay(); transcript.assistantMessages[0]!.text = text;
  return matches(transcript);
};

test('approved scope plus targeted ambiguity questions applies HOLD without naming the mode', () => {
  expect(posture.test(ambiguityNarration)).toBe(false);
  expect(ambiguityMatches()).toBe(true);
  for (const text of [
    ambiguityNarration.replace("I'm holding", 'We are keeping'),
    ambiguityNarration.replace("I'm holding", 'I will hold'),
    ambiguityNarration.replace('the sketch leaves undecided', 'in the plan').replace('flagging', 'surfacing'),
    ambiguityNarration.replace("plan's", "PLAN.md's"),
    ambiguityNarration.replace("I'm", 'I’m').replace("plan's", 'plan’s'),
  ]) expect(ambiguityMatches(text), text).toBe(true);
});

test('ambiguity wording must adopt every obligation without quoting, negating or deferring it', () => {
  for (const text of [
    '> ' + ambiguityNarration, '"' + ambiguityNarration.trim() + '"',
    '```text\n' + ambiguityNarration + '```', '~~~text\n' + ambiguityNarration + '~~~',
    'Example only: ' + ambiguityNarration, 'The user said: ' + ambiguityNarration,
    'Read(file)\n' + ambiguityNarration, 'If approved, ' + ambiguityNarration,
    ambiguityNarration.replace("I'm holding", 'I would hold'),
    ambiguityNarration.replace("I'm holding", 'I will later hold'),
    ambiguityNarration.replace("I'm holding", "I'm not holding"),
    ambiguityNarration.replace('and flagging', 'and not flagging'),
    ambiguityNarration.replace('approved scope', 'proposed scope'),
    ambiguityNarration.replace("plan's", "OTHER.md's"),
    ambiguityNarration.replace('private-only views', 'OTHER.md views'),
    ambiguityNarration.replace('Approach B', 'Approach C'),
    ambiguityNarration.replace('as targeted questions rather than expanding scope', 'as optional improvements'),
    ambiguityNarration.replace('rather than expanding scope', 'while expanding scope'),
    ambiguityNarration.replace('ambiguities the sketch leaves undecided', 'word choice and formatting'),
  ]) expect(ambiguityMatches(text), text).toBe(false);
  for (const correction of [
    'I am expanding scope to include sharing.',
    'Correction: I will add defaults to scope.',
    'The previously excluded sharing feature is now in scope.',
    'Correction: I am no longer holding scope to this plan.',
    "Correction: I am not holding strictly to the plan's approved scope.",
    'Correction: I am no longer flagging ambiguities as targeted questions.',
    'Correction: this posture is withdrawn.',
    'This posture is no longer current.',
  ]) {
    expect(ambiguityMatches(ambiguityNarration + correction), correction).toBe(false);
    expect(ambiguityMatches(ambiguityNarration + '> ' + correction), correction).toBe(true);
  }
});

test('ambiguity posture stays bound to the approved plan and its actual native answer', () => {
  for (const change of [
    (t: PlanCountTranscript) => { t.calls[0]!.answered = false; },
    (t: PlanCountTranscript) => { t.calls[0]!.failed = true; },
    (t: PlanCountTranscript) => { t.calls[0]!.answers![t.calls[0]!.questions[0]!.question] = 'Scope Expansion'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.sessionId = 'foreign'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = t.calls[0]!.answeredAt!; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = 'invalid'; },
    (t: PlanCountTranscript) => { t.assistantMessages[0]!.timestamp = new Date(Date.now() + 60_000).toISOString(); },
  ]) { const transcript = ambiguityReplay(); change(transcript); expect(matches(transcript)).toBe(false); }
  for (const [from, to] of [
    ['PLAN.md', 'PLAN.md and OTHER.md'],
    ['approved.', 'not approved.'],
    ['approved.', 'approved if accepted.'],
    ['approved.', 'discussed.'],
  ]) {
    const transcript = ambiguityReplay(); const q = transcript.calls[0]!.questions[0]!;
    const before = q.question; q.question = before.replace(from!, to!);
    transcript.calls[0]!.answers = { [q.question]: transcript.calls[0]!.answers![before]! };
    expect(matches(transcript), to).toBe(false);
  }
});
{
const captures = retainedPreservationCaptures;
const posture=/\b(rigor|bulletproof|hold\s*scope|maximum\s+rigor)\b/i;
const clone=(i=0)=>structuredClone(captures[i]) as any;
const check=(x:any)=>hasNativePostAnswerCeoPosture(x.transcript,'HOLD SCOPE',posture,x.selectionStartedAt,x.tools,x.source);
const decision=(x:any)=>x.transcript.calls.find((c:any)=>c.questions[0]?.question.match(/^D\d+ — Keep/));
function editQuestion(x:any,change:(q:any)=>void){const c=decision(x);const before=c.questions[0].question;change(c.questions[0]);const after=c.questions[0].question;if(before!==after){c.answers[after]=c.answers[before];delete c.answers[before]};x.tools.find((t:any)=>t.kind==='use'&&t.toolUseId===c.toolUseId).input.questions=structuredClone(c.questions)}
for(let i=0;i<2;i++)test(`actual acknowledged preserve decision ${i+1}`,()=>{const x=clone(i);expect(check(x)).toBe(true)});
const mutations:Record<string,(x:any)=>void>={
 'unanswered':x=>{decision(x).answered=false},
 'failed answer':x=>{x.tools.find((t:any)=>t.kind==='result'&&t.toolUseId===decision(x).toolUseId).isError=true},
 'unmatched native request':x=>{x.tools.find((t:any)=>t.kind==='use'&&t.toolUseId===decision(x).toolUseId).input.questions=[]},
 'foreign decision session':x=>{decision(x).sessionId='foreign'},
 'foreign source path':x=>{x.source.path='/foreign/PLAN.md'},
 'altered source bytes':x=>{x.source.content=x.source.content.replace('update,','share,')},
 'different named source':x=>{editQuestion(x,q=>q.question=q.question.replace('PLAN.md','OTHER.md'))},
 'unrelated choice':x=>{editQuestion(x,q=>{q.question=q.question.replaceAll('update','sharing');q.options=q.options.map((o:any)=>({...o,label:o.label.replaceAll('update','sharing')}))});const c=decision(x);c.answers[c.questions[0].question]=c.questions[0].options[0].label},
 'expanding description':x=>{editQuestion(x,q=>q.options[0].description+=' Also add shared team views outside the plan.')},
 'mere mode label':x=>{editQuestion(x,q=>{q.question=q.question.replace(/ELI10:[\s\S]*?Stakes if/,'ELI10: Keep it.\nStakes if').replace(/Stakes if[\s\S]*?Recommendation:/,'Stakes if we pick wrong: None.\nRecommendation:');q.options.forEach((o:any)=>o.description='Fine.')})},
 'historical decision':x=>{editQuestion(x,q=>q.question='Historical example: '+q.question)},
 'quoted decision':x=>{editQuestion(x,q=>q.question=q.question.split('\n').map((l:string)=>'> '+l).join('\n'))},
 'withdrawn decision':x=>{editQuestion(x,q=>q.question=q.question.replace('HOLD SCOPE review','withdrawn HOLD SCOPE review'))},
 'later withdrawal':x=>{x.transcript.assistantMessages.push({sessionId:decision(x).sessionId,timestamp:new Date().toISOString(),text:'I withdraw this decision.'})},
 'later scope expansion':x=>{x.transcript.assistantMessages.push({sessionId:decision(x).sessionId,timestamp:new Date().toISOString(),text:'I expand the scope.'})},
 'missing source ACK':x=>{x.tools=x.tools.filter((t:any)=>!(t.kind==='result'&&x.tools.some((u:any)=>u.kind==='use'&&u.toolUseId===t.toolUseId&&u.name==='Read'&&u.input?.file_path===x.source.path)))},
 'wrong actual choice':x=>{const c=decision(x);c.answers[c.questions[0].question]=c.questions[0].options[1].label},
};
for(const [name,mutate] of Object.entries(mutations))test(name,()=>{const x=clone();mutate(x);expect(check(x)).toBe(false)});
test('later quoted withdrawal is not current withdrawal',()=>{const x=clone();x.transcript.assistantMessages.push({sessionId:decision(x).sessionId,timestamp:new Date().toISOString(),text:'Example: "I withdraw this decision."'});expect(check(x)).toBe(true)});
test('new proof path is unavailable without explicit fixture source binding',()=>{const x=clone();expect(hasNativePostAnswerCeoPosture(x.transcript,'HOLD SCOPE',posture,x.selectionStartedAt,x.tools)).toBe(false)});

test('retry source cat requires the actual owned project',()=>{const x=clone(1);x.tools.find((t:any)=>t.kind==='use'&&t.input?.command?.includes('cat PLAN.md')).input.command=x.tools.find((t:any)=>t.kind==='use'&&t.input?.command?.includes('cat PLAN.md')).input.command.replace(x.source.path.replace('/PLAN.md',''),'/foreign');expect(check(x)).toBe(false)});
test('retry source read ACK cannot be missing',()=>{const x=clone(1);const use=x.tools.find((t:any)=>t.kind==='use'&&t.input?.command?.includes('cat PLAN.md'));x.tools=x.tools.filter((t:any)=>!(t.kind==='result'&&t.toolUseId===use.toolUseId));expect(check(x)).toBe(false)});

}
});

describe('ceo-mode-colon-at', () => {
const captured = captured_ceo_mode_colon_at;
function transcript(): PlanCountTranscript {
  return { status: 'ready', calls: [structuredClone(captured)], assistantMessages: [] };
}

describe('CEO colon-prefixed native mode choices', () => {
  test('the exact public menu resolves each named mode by display position', () => {
    const options = captured.questions[0]!.options.map((option, i) => ({ index: i + 1, label: option.label }));
    expect(findCeoModeOption(options, 'SELECTIVE EXPANSION')).toBe(1);
    expect(findCeoModeOption(options, 'SCOPE EXPANSION')).toBe(2);
    expect(findCeoModeOption(options, 'HOLD SCOPE')).toBe(3);
    expect(findCeoModeOption(options, 'SCOPE REDUCTION')).toBe(4);
  });

  test('navigation selects expansion in either display order without changing native input', () => {
    for (const reverse of [false, true]) {
      const call = transcript().calls[0]!;
      call.answered = false;
      delete call.answers;
      delete call.unansweredQuestionIndices;
      const question = call.questions[0]!;
      if (reverse) question.options.reverse();
      const original = structuredClone(call);
      const visible = `☐ ${question.header}\n${question.question}\n` + question.options.map((option, i) =>
        `${i ? ' ' : '❯'} ${i + 1}. ${option.label}`).join('\n') +
        '\nEnter to select · ↑/↓ to navigate · Esc to cancel';
      const action = nextCeoModeNavigation(visible, 'SCOPE EXPANSION', new Set(), call);
      expect(action.kind).toBe('mode');
      expect(action.kind === 'mode' && action.index).toBe(reverse ? 3 : 2);
      expect(call).toEqual(original);
    }
  });

  test('the recorded wrong selection remains selective expansion, never expansion coverage', () => {
    const actual = transcript();
    expect(nativeCeoModeAnswer(actual, 'SELECTIVE EXPANSION', 0)?.toolUseId)
      .toBe('toolu_01XY3qPeSuJZa3H2uCfatJ8b');
    expect(nativeCeoModeAnswer(actual, 'SCOPE EXPANSION', 0)).toBeNull();
    expect(actual.calls[0]).toEqual(captured);
  });

  test('pending, failed, stale and ambiguous native answers cannot prove selection', () => {
    for (const change of [
      (value: PlanCountTranscript) => { value.calls[0]!.answered = false; },
      (value: PlanCountTranscript) => { value.calls[0]!.failed = true; },
      (value: PlanCountTranscript) => { delete value.calls[0]!.answers; },
      (value: PlanCountTranscript) => { value.calls[0]!.answeredAt = 'invalid'; },
      (value: PlanCountTranscript) => {
        value.calls[0]!.questions[0]!.options.push({ label: 'E: SELECTIVE EXPANSION' });
      },
    ]) {
      const value = transcript();
      change(value);
      expect(nativeCeoModeAnswer(value, 'SELECTIVE EXPANSION', 0)).toBeNull();
    }
    expect(nativeCeoModeAnswer(transcript(), 'SELECTIVE EXPANSION', Date.parse(captured.answeredAt) + 1)).toBeNull();
    const laterAmbiguous = transcript();
    const later = structuredClone(laterAmbiguous.calls[0]!);
    later.toolUseId = 'later-ambiguous-mode';
    later.answeredAt = new Date(Date.parse(captured.answeredAt) + 1000).toISOString();
    later.questions[0]!.options.push({ label: 'E: SELECTIVE EXPANSION' });
    laterAmbiguous.calls.push(later);
    expect(nativeCeoModeAnswer(laterAmbiguous, 'SELECTIVE EXPANSION', 0)).toBeNull();
  });

  test('action titles, lookalikes and preview descriptions do not become modes', () => {
    for (const label of [
      'A: Use HOLD SCOPE for the next review',
      'B: Explain SCOPE EXPANSION',
      'AA: HOLD SCOPE',
      '1: HOLD SCOPE',
      'A:: HOLD SCOPE',
      'A: HOLD SCOPES',
      'A: Fix contrast │ HOLD SCOPE',
      'A: Fix contrast ┌ SCOPE EXPANSION',
      'A: "HOLD SCOPE"',
      'Prior: HOLD SCOPE',
    ]) expect(findCeoModeOption([{ index: 1, label }], 'HOLD SCOPE')).toBeNull();
    expect(() => findCeoModeOption([
      { index: 1, label: 'A: SELECTIVE EXPANSION │ SCOPE EXPANSION' },
      { index: 2, label: 'B: HOLD SCOPE' },
    ], 'SCOPE EXPANSION')).toThrow('not in option labels');
  });

  test('duplicate and missing mode titles fail before selection; legacy prefixes still work', () => {
    expect(() => findCeoModeOption([
      { index: 1, label: 'A: HOLD SCOPE' },
      { index: 2, label: 'HOLD SCOPE (recommended)' },
    ], 'HOLD SCOPE')).toThrow('duplicate');
    expect(() => findCeoModeOption([{ index: 1, label: 'A: SCOPE REDUCTION' }], 'HOLD SCOPE'))
      .toThrow('not in option labels');
    for (const label of ['A) HOLD SCOPE', 'A. HOLD SCOPE', 'a: hold scope', 'A:  HOLD SCOPE']) {
      expect(findCeoModeOption([{ index: 3, label }], 'HOLD SCOPE')).toBe(3);
    }
  });
});
});

describe('ceo-mode-full-ad', () => {
const fs = fs_ceo_mode_full_ad;
const os = os_ceo_mode_full_ad;
const path = path_ceo_mode_full_ad;
const captured = captured_ceo_mode_full_ad;
const kindCapture = kindCapture_ceo_mode_full_ad;
const pauseCapture = pauseCapture_ceo_mode_full_ad;
const completeInventory = completeInventory_ceo_mode_full_ad;
const nativePacing77 = nativePacing77_ceo_mode_full_ad;
const pattern=/\b(expansion|10x|delight|dream|cathedral|opt[\s-]?in)\b/i;
function replay(i:number){
 const item=captured.cases[i]!,root=fs.mkdtempSync(path.join(os.tmpdir(),'ceo-full-ad-'));
 fs.mkdirSync(path.join(root,'projects','owned'),{recursive:true});
 fs.writeFileSync(path.join(root,'projects','owned',item.process.sessionId+'.jsonl'),item.records.map(r=>JSON.stringify(r)).join('\n')+'\n');
 const events:NativePublicToolEvent[]=[];
 try{return {item,transcript:readPlanCountTranscript(root,item.process.cwd,e=>events.push(e)),events};}
 finally{fs.rmSync(root,{recursive:true,force:true});}
}
function pending(){const c=structuredClone(replay(0).transcript.calls[0]!);c.answered=false;delete c.answers;delete c.answeredAt;delete c.unansweredQuestionIndices;return c;}
// Full panes projected from exact native questions, not retained historical viewports.
function pane(call:NativePlanQuestionCall,index:number){const q=call.questions[index]!;return [
 call.questions.length>1?'← '+call.questions.map((v,i)=>`${i<index?'☒':'☐'} ${v.header}`).join(' ')+' ✔ Submit →':'☐ '+q.header,
 q.question,...q.options.map((v,i)=>`${i?' ':'❯'} ${i+1}. ${v.label}`),
 `Enter to select · ${call.questions.length>1?'Tab/Arrow keys':'↑/↓'} to navigate · Esc to cancel`].join('\n');}
function frame(c:NativePlanQuestionCall,index:number){const visible=pane(c,index);return {visible,active:capturePlanCountQuestion(visible,new Set(),0,true,c)!,routing:nativePlanCallFingerprint(c,0,true)};}
function match(e= replay(1)){return hasNativePostAnswerCeoPosture(e.transcript,'SCOPE EXPANSION',pattern,e.item.selectedAt!,e.events);}
function rebind(e:ReturnType<typeof replay>){const d=e.transcript.calls[1]!,q=d.questions[0]!;e.events[2]!.input={questions:d.questions};d.answers={[q.question]:q.options[0]!.label};}
describe('full AD mode failures retain their actual outcomes',()=>{
 test('Proposal 1 is a completed scope decision after the actual selected mode',()=>{
  const e=replay(1);expect(e.item.actualState).toBe('failed');expect(e.transcript.calls).toHaveLength(2);expect(e.events).toHaveLength(4);
  expect(e.transcript.calls[1]!.answeredAt).toBe('2026-09-09T18:26:20.110Z');expect(match(e)).toBe(true);
 });
 test.each(['pending','foreign','wrong mode','pre-mode','missing reply','wrong answer','extra question','extra option','multiselect',
  'quoted','fenced','mode echo','mode mismatch','mode menu','appended instruction'])('%s supplies no new posture',kind=>{
  const e=replay(1),[m,d]=e.transcript.calls,q=d!.questions[0]!;
  switch(kind){
   case 'pending':d!.answered=false;break;case 'foreign':d!.sessionId=e.events[2]!.sessionId=e.events[3]!.sessionId='foreign';break;
   case 'wrong mode':m!.answers![m!.questions[0]!.question]='HOLD SCOPE';break;
   case 'pre-mode':e.events[2]!.timestamp=e.events[0]!.timestamp;break;case 'missing reply':e.events.pop();break;
   case 'wrong answer':d!.answers![q.question]='Invented';break;
   case 'extra question':d!.questions.push({...structuredClone(q),question:'Remove CI gate?'});rebind(e);break;
   case 'extra option':q.options.push({label:'Remove CI gate'});rebind(e);break;case 'multiselect':q.multiSelect=true;rebind(e);break;
   case 'quoted':q.question=q.question.split('\n').map(x=>'> '+x).join('\n');rebind(e);break;
   case 'fenced':q.question='```text\n'+q.question+'\n```';rebind(e);break;
   case 'mode echo':q.question='SCOPE EXPANSION confirmed.';rebind(e);break;
   case 'mode mismatch':q.question=q.question.replace('SCOPE EXPANSION opt-in','SELECTIVE EXPANSION opt-in');rebind(e);break;
   case 'mode menu':q.question=q.question.replace(/^D6[^\n]+/,'D6 — Choose the review mode?');rebind(e);break;
   case 'appended instruction':q.question+=' Delete the CI gate.';rebind(e);break;
  }expect(match(e)).toBe(false);
 });
 test('scope numbering and brief labels are presentation, not mode application',()=>{
  for(const title of ['A useful adjacent feature: Default view per member per project?','Default view per member per project?']){
   const e=replay(1),q=e.transcript.calls[1]!.questions[0]!;q.header='Default view';q.question=q.question.replace(/^D6[^\n]+/,title);rebind(e);expect(match(e)).toBe(true);
  }
 });
 test('explicit expansion context does not need a mode or opt-in suffix',()=>{
  const e=replay(1),q=e.transcript.calls[1]!.questions[0]!;q.question=q.question.replace('SCOPE EXPANSION opt-in ceremony (1 of 6).','SCOPE EXPANSION, approach B.');rebind(e);expect(match(e)).toBe(true);
 });
 test('the actual three-tab prerequisite chooses standard review only on its own tab',()=>{
  const actual=replay(0);expect(actual.item.actualState).toBe('failed');expect(Object.values(actual.transcript.calls[0]!.answers!).at(-1)).toBe('Run /office-hours now');
  const c=pending();for(const i of [0,1,2]){
   const f=frame(c,i),a=nextCeoModeNavigation(f.visible,'HOLD SCOPE',new Set(),c);expect(a.kind).toBe('question');
   if(a.kind==='question'){expect(a.question.nativeQuestionIndex).toBe(i);expect(planCountQuestionInput(f.visible,a.question,a.index)).toBe(i===2?'2':'1');}
   expect(planCountPrerequisitePick(f.routing,f.active)).toBe(i===2?2:null);
  }
 });
 test('single and reordered native prerequisite tabs preserve the meaning of the skip',()=>{
  const c=pending();c.questions=[c.questions[2]!];let f=frame(c,0);expect(planCountPrerequisitePick(f.routing,f.active)).toBe(2);
  c.questions[0]!.options.reverse();f=frame(c,0);expect(planCountPrerequisitePick(f.routing,f.active)).toBe(1);
 });
 test.each(['wrong tab','wrong signature','wrong body','wrong order','no metadata','completed','failed','extra action','multiselect','conditional','extra remedy','no description'])('a %s cannot borrow the prerequisite action',kind=>{
  const c=pending();if(kind==='completed')c.answered=true;if(kind==='failed')c.failed=true;
  if(kind==='extra action')c.questions[2]!.options.push({label:'Accept risk'});
  if(kind==='multiselect')c.questions[2]!.multiSelect=true;
  if(kind==='conditional')c.questions[2]!.options[1]!.description+=' if all tests pass.';
  if(kind==='extra remedy')c.questions[2]!.options[1]!.description+=' Remove the CI gate.';
  if(kind==='no description')c.questions[2]!.options[1]!.description='';
  const f=frame(c,2);let a=f.active;
  if(kind==='wrong tab')a={...a,nativeQuestionIndex:0};if(kind==='wrong signature')a={...a,signature:'foreign:tool:question:2'};
  if(kind==='wrong body')a={...a,promptSnippet:'Choose a product direction.'};if(kind==='wrong order')a={...a,options:[...a.options].reverse()};
  if(kind==='no metadata')a={...a,nativeCall:undefined};
  expect(planCountPrerequisitePick(f.routing,a)).toBeNull();
 });
});

describe('full AD HOLD retry completed sequencing rationale',()=>{
 function hold(){const e=replay(2);return {e,decision:e.transcript.calls[2]!,q:e.transcript.calls[2]!.questions[0]!};}
 function matches(e:ReturnType<typeof replay>){return hasNativePostAnswerCeoPosture(e.transcript,'HOLD SCOPE',/\b(rigor|bulletproof|hold\s*scope|maximum\s+rigor)\b/i,e.item.selectedAt!,e.events);}
 function bind(e:ReturnType<typeof replay>){const d=e.transcript.calls[2]!,q=d.questions[0]!;e.events[4]!.input={questions:d.questions};d.answers={[q.question]:q.options[0]!.label};}
 test('the actual completed rationale applies HOLD to work in the previously approved approach',()=>{
  const {e,decision,q}=hold();expect(e.item.actualState).toBe('failed');expect(e.transcript.calls).toHaveLength(3);
  const approach=e.transcript.calls[0]!;expect(Object.values(approach.answers!)).toEqual(['B: ViewState schema (recommended)']);
  expect(approach.questions[0]!.options[0]!.description).toContain('URL params');
  expect(decision.answeredAt).toBe('2026-09-09T18:35:05.273Z');expect(q.question).toContain('not new scope either way');expect(matches(e)).toBe(true);
 });
 test('three and four alternatives still express one completed review decision',()=>{
  for(const count of [3,4]){const {e,q}=hold();q.options.push({label:'Gate URL sync for the pilot'});if(count===4)q.options.push({label:'Run a limited URL sync pilot'});bind(e);expect(matches(e)).toBe(true);}
 });
 test.each(['pending','foreign','before mode','missing reply','failed reply','wrong answer','metadata only','bare echo','other mode',
   'quoted rationale','fenced rationale','duplicate options','extra question','extra instruction','multiselect'])('%s is not completed HOLD rationale',kind=>{
  const {e,decision,q}=hold();
  switch(kind){
   case 'pending':decision.answered=false;break;case 'foreign':decision.sessionId=e.events[4]!.sessionId=e.events[5]!.sessionId='foreign';break;
   case 'before mode':e.events[4]!.timestamp=e.events[0]!.timestamp;break;case 'missing reply':e.events.pop();break;case 'failed reply':e.events[5]!.isError=true;break;
   case 'wrong answer':decision.answers![q.question]='Invented';break;
   case 'metadata only':q.question=q.question.replace(/ELI10:[\s\S]*?\nStakes/,'ELI10: We will implement the URL codec.\nStakes');bind(e);break;
   case 'bare echo':q.question=q.question.replace(/ELI10:[\s\S]*?\nStakes/,'ELI10: HOLD SCOPE confirmed.\nStakes');bind(e);break;
   case 'other mode':q.question=q.question.replace(/HOLD SCOPE/g,'SCOPE EXPANSION');bind(e);break;
   case 'quoted rationale':q.question=q.question.replace('ELI10: Approach','ELI10:\n> Approach');bind(e);break;
   case 'fenced rationale':q.question=q.question.replace('ELI10: Approach','ELI10: ```Approach');bind(e);break;
   case 'duplicate options':q.options[1]!.label=q.options[0]!.label;bind(e);break;
   case 'extra question':decision.questions.push({...structuredClone(q),question:'Remove CI?'});bind(e);break;
   case 'extra instruction':q.question+=' Disable authentication.';bind(e);break;
   case 'multiselect':q.multiSelect=true;bind(e);break;
  }expect(matches(e)).toBe(false);
 });
});
describe('completed expansion disposition classes from the retained dacc public questions', () => {
  // Request/answer content is captured. The envelopes and chronology below are
  // synthetic: missing original JSONL timestamps must never become E2E evidence.
  function current(kind: 'retry' | 'meta' | 'unanswered' = 'retry') {
    const e = replay(1), decision = e.transcript.calls[1]!;
    decision.questions = [structuredClone(kind === 'meta' ? kindCapture.firstMetaQuestion
      : kind === 'unanswered' ? kindCapture.firstUnansweredQuestion : kindCapture.retryQuestion)];
    e.events[2]!.input = { questions: decision.questions };
    decision.answers = { [decision.questions[0]!.question]: kind === 'meta'
      ? kindCapture.firstMetaAnswer : kindCapture.retryAnswer };
    if (kind === 'unanswered') { decision.answered = false; delete decision.answers; e.events.pop(); }
    return e;
  }
  function amend(e: ReturnType<typeof current>, fn: (q: NativePlanQuestionCall['questions'][number]) => void) {
    const d=e.transcript.calls[1]!,q=d.questions[0]!,answer=d.answers?.[q.question];
    fn(q);e.events[2]!.input={questions:d.questions};d.answers={[q.question]:answer!};
  }
  test('the exact acknowledged Include content supplies posture in a synthetic ownership envelope', () => {
    const e=current();expect(kindCapture.actualOutcome).toContain('Both EXPANSION attempts failed');
    expect(e.transcript.assistantMessages.every(m=>Date.parse(m.timestamp)<e.item.selectedAt!)).toBe(true);
    expect(match(e)).toBe(true);
  });
  test.each(['canonical three','reordered','curly scenario','coverage scores','defer','cut'] as const)('%s preserves a substantive completed choice', kind => {
    const e=current();amend(e,q=>{
      if(kind==='canonical three'){
        q.options=q.options.slice(0,3).map((o,i)=>({...o,label:["A) Add to this plan's scope (recommended)",'B) Defer to TODOS.md','C) Skip'][i]!}));
      }
      if(kind==='reordered')q.options.reverse();
      if(kind==='curly scenario')q.question=q.question.replace('"can you share your view?"','“can you share your view?”');
      if(kind==='coverage scores')q.question=q.question.replace('Note: options differ in kind, not coverage — no completeness score.','Completeness: A=10/10, B=7/10, C=3/10');
    });
    const d=e.transcript.calls[1]!,q=d.questions[0]!;
    if(kind==='canonical three')d.answers={[q.question]:q.options[0]!.label};
    if(kind==='defer')d.answers={[q.question]:q.options[1]!.label};
    if(kind==='cut')d.answers={[q.question]:q.options[2]!.label};
    expect(match(e)).toBe(true);
  });
  test.each(['meta','unanswered'] as const)('the original %s does not supply completed expansion evidence', kind=>{
    expect(match(current(kind))).toBe(false);
  });
  test.each(['pending','selected pause','only pause','missing core','extra action','duplicate disposition',
    'generic continuation','second question','quoted decision','fenced decision','mixed packet',
    'multiselect','missing comparison','invalid score','both comparison branches','wrong mode','missing reply'] as const)(
    '%s is not a completed expansion decision', kind=>{
      const e=current();amend(e,q=>{
        if(kind==='only pause')q.options=[q.options[3]!];
        if(kind==='missing core')q.options.splice(1,1);
        if(kind==='extra action')q.options[3]!.label='Remove the CI gate';
        if(kind==='duplicate disposition')q.options[3]!.label='Add to scope';
        if(kind==='generic continuation')q.question=q.question.replace(/^D3\.1[^\n]+/,'D3.1 — Continue the review?');
        if(kind==='second question')q.question=q.question.replace('\nStakes if', '\nShould we remove access checks?\nStakes if');
        if(kind==='quoted decision')q.question=q.question.split('\n').map(l=>'> '+l).join('\n');
        if(kind==='fenced decision')q.question='```text\n'+q.question+'\n```';
        if(kind==='multiselect')q.multiSelect=true;
        if(kind==='missing comparison')q.question=q.question.replace('Note: options differ in kind, not coverage — no completeness score.','No comparison.');
        if(kind==='invalid score')q.question=q.question.replace('Note: options differ in kind, not coverage — no completeness score.','Completeness: A=11/10, B=7/10, C=3/10');
        if(kind==='both comparison branches')q.question=q.question.replace('\nNet:','\nCompleteness: A=10/10, B=7/10, C=3/10\nNet:');
      });
      const d=e.transcript.calls[1]!,q=d.questions[0]!;
      if(kind==='pending')d.answered=false;
      if(kind==='selected pause')d.answers={[q.question]:q.options[3]!.label};
      if(kind==='mixed packet'){d.questions.push({...structuredClone(q),question:'Remove access checks?'});e.events[2]!.input={questions:d.questions};}
      if(kind==='wrong mode'){const m=e.transcript.calls[0]!;m.answers={[m.questions[0]!.question]:'HOLD SCOPE'};}
      if(kind==='missing reply')e.events.pop();
      expect(match(e)).toBe(false);
    });
});


describe('owned expansion decisions with a nondecision discussion control', () => {
  function current() {
    const transcript = { status: 'ready' as const, calls: structuredClone(pauseCapture.calls), assistantMessages: [] };
    const events = structuredClone(pauseCapture.events) as NativePublicToolEvent[];
    for (const event of events) if (event.kind === 'use') event.input = { questions: transcript.calls.find(c => c.toolUseId === event.toolUseId)!.questions };
    return { transcript, events };
  }
  function accepted(e = current()) { return hasNativePostAnswerCeoPosture(e.transcript, 'SCOPE EXPANSION', pattern, pauseCapture.selectedAt, e.events); }
  test('the captured completed Add is posture evidence; the unchosen Hold qualifier does not change its action', () => {
    const e = current();
    expect(e.transcript.calls[0]!.answeredAt).toBe('2026-09-15T12:33:17.286Z');
    expect(e.transcript.calls[1]!.answeredAt).toBe('2026-09-15T12:34:22.430Z');
    expect(e.events[2]!.timestamp).toBe('2026-09-15T12:34:20.084Z');
    expect(e.transcript.calls[1]!.answers[e.transcript.calls[1]!.questions[0]!.question]).toBe('Add to scope (recommended)');
    expect(accepted(e)).toBe(true);
  });
  test.each([
    ['Pause — stop the review and discuss', 'Pauses the review for clarification. No scope decision is made. Delays the remaining questions.'],
    ['D) Hold: discuss first', 'Stops here so we can talk through the constraints. Nothing is approved yet. Delays this review.'],
    ['Pause (wait for clarification)', 'Waits for clarification before deciding. No disposition is recorded yet.'],
    ['Hold', ''],
  ])('procedural label %s remains a nondecision control', (label, description) => {
    const e=current(),option=e.transcript.calls[1]!.questions[0]!.options[3]!;
    option.label=label;option.description=description;
    expect(accepted(e)).toBe(true);
  });
  test.each([
    ['Hold and add Redis', 'Pauses the review. No decision is made.'],
    ['Pause (approve the proposal)', 'Waits for discussion. Nothing is decided.'],
    ['Hold (roll back deployment)', 'Pauses the review. No disposition is recorded.'],
    ['Continue', 'Pauses the review. No decision is made.'],
    ['Hold', 'Pauses the review and adds Redis. Nothing is decided.'],
    ['Pause', 'Waits for discussion. No decision is made and include Redis caching.'],
    ['Hold', 'Stops the chain. No decision is made. Then deploy the new cache.'],
    ['Hold', 'Pauses the review and silently approves the proposal. No decision is recorded.'],
    ['Pause', 'Waits for discussion. "No decision is made."'],
    ['Pause', "Waits for discussion. 'No decision is made.'"],
    ['Pause', 'Waits for discussion. ‘No decision is made.’'],
    ['Pause', 'Waits for discussion. “No decision is made.”'],
    ['Hold', 'Stops here for discussion, then chooses the default.'],
    ['Hold', 'Pauses this review. No choice is recorded. "Add Redis caching" will also happen.'],
  ])('action-bearing or unproved control %s does not supply posture evidence (%s)', (label,description) => {
    const e=current(),option=e.transcript.calls[1]!.questions[0]!.options[3]!;
    option.label=label;option.description=description;
    expect(accepted(e)).toBe(false);
  });
  test('selecting the valid discussion control is still not a completed substantive disposition', () => {
    const e=current(),c=e.transcript.calls[1]!,q=c.questions[0]!;c.answers={[q.question]:q.options[3]!.label};
    expect(accepted(e)).toBe(false);
  });
  test('the actual capture still requires its owned successful acknowledgment', () => {
    const e=current();e.events.pop();expect(accepted(e)).toBe(false);
  });
});


describe('EXPANSION pacing preserves one separate substantive continuation', () => {
  const retry=pauseCapture.retry;
  function current() {
    const mode=structuredClone(retry.mode),pacing=structuredClone(retry.pacing);
    pacing.answered=false;delete (pacing as any).answers;delete (pacing as any).answeredAt;delete (pacing as any).unansweredQuestionIndices;
    const transcript={status:'ready' as const,calls:[mode,pacing],assistantMessages:[]};
    return {transcript,pacing,visible:pane(pacing as NativePlanQuestionCall,0)};
  }
  function choice(e=current()) {return ceoExpansionPacingChoice(e.visible,e.transcript,retry.selectedAt);}
  // Canonical panes below are projected from the exact native request. The
  // CLI 2.1.251 redraw stream retained these two built-ins, not a stable frame.
  function withNativeControls(e=current()) {
    e.visible=e.visible.replace('Enter to select','4. Type something.\n5. Chat about this\nEnter to select');
    return e;
  }
  test('the observed native pacing controls do not become authored choices',()=>{
    expect(choice(withNativeControls())?.index).toBe(1);
  });
  test.each(['Choosing Full per-item split approves E1 immediately.',
    'Answering this question authorizes every proposed expansion.',
    'This answer commits E1 to the implementation scope.',
    'Choosing Full per-item split deploys E1 immediately.',
    'This answer ships E1 immediately.',
    'Choosing Full per-item split enables E1.',
    'This answer disables E2.',
    '“Choosing Full per-item split approves E1 immediately.”'])('whole-question scope effect is not pacing: %s',effect=>{
    const e=current();e.pacing.questions[0]!.question=e.pacing.questions[0]!.question.replace('ELI10:',`ELI10: ${effect}`);
    e.visible=pane(e.pacing as NativePlanQuestionCall,0);expect(choice(e)?.index).toBe(0);
  });
  test.each(['unknown action','reordered controls','extra control','mismatched authored option'])('native pacing pane rejects %s',kind=>{
    const e=withNativeControls();
    if(kind==='unknown action')e.visible=e.visible.replace('Type something.','Approve all now.');
    if(kind==='reordered controls')e.visible=e.visible.replace('Type something.','Chat about this').replace('5. Chat about this','5. Type something.');
    if(kind==='extra control')e.visible=e.visible.replace('Enter to select','6. More actions\nEnter to select');
    if(kind==='mismatched authored option')e.visible=e.visible.replace('Full per-item split','Approve all proposals');
    expect(choice(e)?.index).toBe(0);
  });
  test('the captured full-per-item answer preserves scope; pacing alone and actual pending E1 remain negative',()=>{
    const e=current(),pick=choice(e);expect(pick?.index).toBe(1);
    expect(hasNativePostAnswerCeoPosture({status:'ready',calls:[retry.mode,retry.pacing],assistantMessages:[]},'SCOPE EXPANSION',pattern,retry.selectedAt,[])).toBe(false);
    expect(retry.pendingProposal.answered).toBe(false);
    expect(ceoExpansionPacingReady('next screen',e.transcript,pick!,[])).toBe(false);
  });
  test('the preserving option can be reordered or use equivalent individual-walkthrough wording',()=>{
    const e=current(),q=e.pacing.questions[0]!;q.options.reverse();
    q.options[2]!.label='All proposals individually';
    q.options[2]!.description='Each proposal separately with Add / Defer / Skip / Hold. No item is skipped or merged without your approval. Delays the remaining review.';
    e.visible=pane(e.pacing as NativePlanQuestionCall,0);expect(choice(e)?.index).toBe(3);
  });
  test.each(['foreign','unanswered mode','wrong mode','already answered','mixed packet','mismatched viewport','narrowing','bundled approval','quoted assurance','duplicate preserving choice','multiple pending calls'])('%s cannot authorize pacing',kind=>{
    const e=current(),q=e.pacing.questions[0]!,o=q.options[0]!;
    if(kind==='foreign')e.pacing.sessionId='foreign';
    if(kind==='unanswered mode')e.transcript.calls[0]!.answered=false;
    if(kind==='wrong mode')e.transcript.calls[0]!.answers={[e.transcript.calls[0]!.questions[0]!.question]:'HOLD SCOPE'};
    if(kind==='already answered')e.pacing.answered=true;
    if(kind==='mixed packet')e.pacing.questions.push({...structuredClone(q),header:'Extra scope',question:'Approve all proposals now?'});
    if(kind==='narrowing')o.description+=' Add E1 and drop E2 now.';
    if(kind==='bundled approval')o.label='Full per-item split and approve all';
    if(kind==='quoted assurance')o.description=o.description.replace('No proposal is dropped or merged without your say','"No proposal is dropped or merged without your say"');
    if(kind==='duplicate preserving choice')q.options[1]=structuredClone(o);
    if(kind==='multiple pending calls')e.transcript.calls.push({...structuredClone(e.pacing),toolUseId:'another-pending-call'});
    if(kind!=='mismatched viewport')e.visible=pane(e.pacing as NativePlanQuestionCall,0);
    else e.visible=e.visible.replace('Full per-item split','Narrow first');
    if(['foreign','unanswered mode','wrong mode','already answered'].includes(kind))expect(choice(e)).toBeNull();
    else expect(choice(e)?.index).toBe(0);
  });
  test('the pacing transition needs its successful bound ACK and a different current pane',()=>{
    const e=current(),pick=choice(e)!;e.transcript.calls[1]=structuredClone(retry.pacing);
    const c=e.transcript.calls[1]!,events:NativePublicToolEvent[]=[
      {kind:'use',name:'AskUserQuestion',sessionId:c.sessionId,toolUseId:c.toolUseId,timestamp:new Date(Date.parse(c.answeredAt!)-1000).toISOString(),input:{questions:c.questions}},
      {kind:'result',sessionId:c.sessionId,toolUseId:c.toolUseId,timestamp:c.answeredAt!,isError:false},
    ];
    // Request time is synthetic; the captured ACK time and request body are retained.
    expect(ceoExpansionPacingReady('a different current pane',e.transcript,pick,events)).toBe(true);
    expect(ceoExpansionPacingReady(e.visible,e.transcript,pick,events)).toBe(false);
    expect(ceoExpansionPacingReady('a different current pane',e.transcript,pick,events.slice(0,1))).toBe(false);
    events[1]!.isError=true;expect(ceoExpansionPacingReady('a different current pane',e.transcript,pick,events)).toBe(false);
    events[1]!.isError=false;c.answers={[c.questions[0]!.question]:c.questions[0]!.options[1]!.label};
    expect(ceoExpansionPacingReady('a different current pane',e.transcript,pick,events)).toBe(false);
  });
  function acknowledgedProposal() {
    // Derived transition only: pending E1 never received an actual paid ACK.
    // Missing original request times below are explicitly synthetic.
    const mode=structuredClone(retry.mode),proposal=structuredClone(retry.pendingProposal) as NativePlanQuestionCall;
    proposal.answered=true;proposal.answers={[proposal.questions[0]!.question]:proposal.questions[0]!.options[0]!.label};proposal.unansweredQuestionIndices=[];
    proposal.answeredAt=new Date(Date.parse(retry.pacing.answeredAt)+2000).toISOString();
    const transcript={status:'ready' as const,calls:[mode,proposal],assistantMessages:[]};
    const events:NativePublicToolEvent[]=transcript.calls.flatMap(c=>[
      {kind:'use' as const,name:'AskUserQuestion',sessionId:c.sessionId,toolUseId:c.toolUseId,timestamp:new Date(Date.parse(c.answeredAt!)-1000).toISOString(),input:{questions:c.questions}},
      {kind:'result' as const,sessionId:c.sessionId,toolUseId:c.toolUseId,timestamp:c.answeredAt!,isError:false},
    ]);
    return {transcript,events};
  }
  test('a separately acknowledged current proposal establishes scope expansion through its real before/after comparison',()=>{
    const e=acknowledgedProposal();expect(hasNativePostAnswerCeoPosture(e.transcript,'SCOPE EXPANSION',pattern,retry.selectedAt,e.events)).toBe(true);
    expect(hasNativePostAnswerCeoPosture(e.transcript,'SCOPE EXPANSION',/cathedral/i,retry.selectedAt,e.events)).toBe(false);
  });
  test.each(['ordinal/source link','decimal decision identity','before/after paraphrase','defer','skip'])('%s preserves the same current proposal',kind=>{
    const e=acknowledgedProposal(),c=e.transcript.calls[1]!,q=c.questions[0]!;
    if(kind==='ordinal/source link')q.question=q.question.replace('E1: Project-shared views (ledger row S1)','Proposal 1 of 7: E1 — Project-shared views [source](PLAN.md)');
    if(kind==='decimal decision identity')q.question=q.question.replace('D3.1 —','D12.3.1 —');
    if(kind==='before/after paraphrase')q.question=q.question.replace('Today the plan saves a view for one member only. E1 adds','As written, each member keeps private views. E1 would introduce');
    c.answers={[q.question]:q.options[kind==='defer'?1:kind==='skip'?2:0]!.label};e.events[2]!.input={questions:c.questions};
    expect(hasNativePostAnswerCeoPosture(e.transcript,'SCOPE EXPANSION',pattern,retry.selectedAt,e.events)).toBe(true);
  });
  test.each(['pending','missing ACK','wrong proposal identity','no current baseline','vague baseline','second question','quoted comparison','foreign','selected pause'])('%s supplies no proposal completion',kind=>{
    const e=acknowledgedProposal(),c=e.transcript.calls[1]!,q=c.questions[0]!;
    if(kind==='pending')c.answered=false;
    if(kind==='missing ACK')e.events.pop();
    if(kind==='wrong proposal identity')q.question=q.question.replace('E1 adds','E2 adds');
    if(kind==='no current baseline')q.question=q.question.replace('Today the plan saves','Previously an unrelated plan saved');
    if(kind==='vague baseline')q.question=q.question.replace('Today the plan saves a view for one member only.','Today the plan is interesting.');
    if(kind==='second question')q.question=q.question.replace('ELI10:','ELI10: Should we remove access checks?');
    if(kind==='quoted comparison')q.question=q.question.replace('ELI10: Today','ELI10: "Today').replace('Stakes if','"\nStakes if');
    if(kind==='foreign')c.sessionId='foreign';
    c.answers={[q.question]:q.options[kind==='selected pause'?3:0]!.label};e.events[2]!.input={questions:c.questions};
    expect(hasNativePostAnswerCeoPosture(e.transcript,'SCOPE EXPANSION',pattern,retry.selectedAt,e.events)).toBe(false);
  });
});
describe('complete candidate split is navigation with an actual ACK boundary',()=>{
const f=completeInventory;
const actualFrame=f.viewport;
function state(){const pacing=structuredClone(f.pacing);pacing.answered=false;delete pacing.answers;delete pacing.answeredAt;delete pacing.unansweredQuestionIndices;return{pacing,transcript:{status:'ready' as const,calls:[structuredClone(f.mode),pacing],assistantMessages:[]}};}
function pane(c:any){const q=c.questions[0];return ['☐ '+q.header,q.question,...q.options.map((o:any,i:number)=>`${i?' ':'❯'} ${i+1}. ${o.label}`),'4. Type something.','5. Chat about this','Enter to select · ↑/↓ to navigate · Esc to cancel'].join('\n');}
const verify=(name:string,pass:boolean)=>test(name,()=>expect(pass).toBe(true));
const choose=(e=state(),screen=pane(e.pacing))=>ceoExpansionPacingChoice(screen,e.transcript,f.selectedAt);
verify('actual retained frame selects the complete seven-candidate walkthrough',choose(state(),actualFrame)?.index===1);
for(const [name,mutate]of Object.entries({
 'eight complete candidates':(q:any)=>{q.question=q.question.replaceAll('7 expansion candidates','8 expansion candidates').replace('7 adjacent improvements','8 adjacent improvements').replace('E7 cross-project views.','E7 cross-project views, E8 shared pinned groups.').replaceAll('Seven','Eight');q.options[0].label=q.options[0].label.replace('7 questions','8 questions');q.options[0].description=q.options[0].description.replace('E7','E8');},
 'different proposal prefix':(q:any)=>{q.question=q.question.replace(/\bE(?=\d)/g,'P');q.options.forEach((o:any)=>{o.description=o.description.replace(/\bE(?=\d)/g,'P');});},
 'complete walkthrough label':(q:any)=>{q.options[0].label='A: Complete walkthrough, 7 questions (recommended)';},
 'one per item with explicit range':(q:any)=>{q.options[0].description='One question per item, E1 to E7.';},
 'reordered choices':(q:any)=>{q.options.reverse();},
})){const e=state();mutate(e.pacing.questions[0]);verify(name,choose(e)?.index===(name==='reordered choices'?3:1));}
for(const [name,mutate]of Object.entries({
 'partial range':(q:any)=>{q.options[0].description=q.options[0].description.replace('E7','E6');},
 'wrong number of questions':(q:any)=>{q.options[0].label=q.options[0].label.replace('7','6');},
 'wrong declared count':(q:any)=>{q.question=q.question.replace('7 expansion candidates','8 expansion candidates');},
 'missing candidate':(q:any)=>{q.question=q.question.replace(', E7 cross-project views','');},
 'duplicate candidate':(q:any)=>{q.question=q.question.replace('E7 cross-project views','E6 cross-project views');},
 'mixed proposal IDs':(q:any)=>{q.question=q.question.replace('E7 cross-project views','P7 cross-project views');},
 'narrow selected walk':(q:any)=>{q.options[0].description+=' Except E4.';},
 'selected scope approval':(q:any)=>{q.options[0].description+=' Approve E1 immediately.';},
 'selected deletion':(q:any)=>{q.options[0].label+=' and delete E7';},
 'selected grouping':(q:any)=>{q.options[0].description+=' Batch E1 and E2 together.';},
 'quoted only range':(q:any)=>{q.options[0].description='"'+q.options[0].description+'"';},
 'code-only range':(q:any)=>{q.options[0].description='`'+q.options[0].description+'`';},
 'negated complete walk':(q:any)=>{q.options[0].label=q.options[0].label.replace('Full split','Not a full split');},
 'duplicate complete choice':(q:any)=>{q.options[1]=structuredClone(q.options[0]);},
 'extra question':(q:any)=>{q.question=q.question.replace('ELI10:','ELI10: Should all candidates ship?');},
 'unconditional approval':(q:any)=>{q.question=q.question.replace('ELI10:','ELI10: This answer approves every expansion.');},
 'quoted whole-question approval':(q:any)=>{q.question=q.question.replace('ELI10:','ELI10: “Choosing Full split approves E1 immediately.”');},
 'hidden universal effect in another option':(q:any)=>{q.question=q.question.replace('B) Narrow first:','B) Regardless of choice, approve E1. Narrow first:');},
 'historical inventory':(q:any)=>{q.question=q.question.replace('The delight scan produced','Previously the delight scan produced');},
 'fenced brief':(q:any)=>{q.question='```\n'+q.question+'\n```';},
 'missing comparison marker':(q:any)=>{q.question=q.question.replace('Note: options differ in kind, not coverage — no completeness score.','');},
})){const e=state();mutate(e.pacing.questions[0]);verify(name,choose(e)?.index!==1);}
for(const [name,mutate]of Object.entries({
 'foreign session':(e:any)=>{e.pacing.sessionId='foreign';},
 'unanswered mode':(e:any)=>{e.transcript.calls[0].answered=false;},
 'already answered pacing':(e:any)=>{e.pacing.answered=true;},
 'mixed question packet':(e:any)=>{e.pacing.questions.push({...structuredClone(e.pacing.questions[0]),header:'Extra',question:'Approve everything?'});},
 'another pending call':(e:any)=>{e.transcript.calls.push({...structuredClone(e.pacing),toolUseId:'other'});},
})){const e=state();mutate(e);verify(name,choose(e)?.index!==1);}
const e=state(),choice=choose(e,actualFrame)!;const acknowledged={status:'ready' as const,calls:[f.mode,f.pacing,f.pending],assistantMessages:[]};
const actualNext=f.nextViewport;
verify('actual pacing ACK and different pending E1 pane complete navigation',ceoExpansionPacingReady(actualNext,acknowledged,choice,f.publicEvents));
verify('intended key without actual ACK does not complete navigation',!ceoExpansionPacingReady(actualNext,e.transcript,choice,f.publicEvents));
verify('missing result does not complete navigation',!ceoExpansionPacingReady(actualNext,acknowledged,choice,f.publicEvents.filter((e:any)=>e.kind!=='result')));
verify('failed result does not complete navigation',!ceoExpansionPacingReady(actualNext,acknowledged,choice,f.publicEvents.map((e:any)=>({...e,isError:e.kind==='result'}))));
verify('same old pane does not complete navigation',!ceoExpansionPacingReady(actualFrame,acknowledged,choice,f.publicEvents));
verify('pacing and pending E1 supply no completed posture',!hasNativePostAnswerCeoPosture(acknowledged,'SCOPE EXPANSION',/expansion|10x|delight|dream/i,f.selectedAt,f.publicEvents));
});

describe('candidate inventory cannot approve scope',()=>{
const f=completeInventory;
function state(){const pacing=structuredClone(f.pacing);pacing.answered=false;delete pacing.answers;delete pacing.answeredAt;delete pacing.unansweredQuestionIndices;return{pacing,transcript:{status:'ready' as const,calls:[structuredClone(f.mode),pacing],assistantMessages:[]}};}
function pane(c:any){const q=c.questions[0];return ['☐ '+q.header,q.question,...q.options.map((o:any,i:number)=>`${i?' ':'❯'} ${i+1}. ${o.label}`),'4. Type something.','5. Chat about this','Enter to select · ↑/↓ to navigate · Esc to cancel'].join('\n');}
const mutations={
 'inventory actor grants all candidates':(q:any)=>q.question=q.question.replace('E7 cross-project views.','E7 cross-project views; we approve all seven now.'),
 'inventory item claims current approval':(q:any)=>q.question=q.question.replace('E7 cross-project views.','E7 cross-project views (already approved).'),
 'inventory item has bare approval status':(q:any)=>q.question=q.question.replace('E7 cross-project views.','E7 cross-project views (approved).'),
 'inventory all items are approved':(q:any)=>q.question=q.question.replace('E7 cross-project views.','E7 cross-project views; all seven are approved.'),
 'inventory imperative ship grant':(q:any)=>q.question=q.question.replace('E7 cross-project views.','E7 cross-project views; ship all seven now.'),
 'inventory scope disposition':(q:any)=>q.question=q.question.replace('E7 cross-project views.','E7 cross-project views; all seven are in scope.'),
 'inventory skipped candidate':(q:any)=>q.question=q.question.replace('E7 cross-project views.','E7 cross-project views (deferred).'),
 'title claims inventory approved':(q:any)=>q.question=q.question.replace('How do you want to decide them?','All seven are already approved. How do you want to decide them?'),
 'rationale claims candidates in scope':(q:any)=>q.question=q.question.replace('The delight scan produced','All candidates are in scope. The delight scan produced'),
 'rationale claims prior approval':(q:any)=>q.question=q.question.replace('The delight scan produced','These items have been approved. The delight scan produced'),
};
for (const [name,mutate] of Object.entries(mutations)) test(name,()=>{
  const e=state();mutate(e.pacing.questions[0]);
  expect(ceoExpansionPacingChoice(pane(e.pacing),e.transcript,f.selectedAt)?.index).not.toBe(1);
});
test('descriptive Update and delete feature titles remain supported',()=>{
  expect(ceoExpansionPacingChoice(f.viewport,state().transcript,f.selectedAt)?.index).toBe(1);
});
});
describe('complete per-proposal pacing preserves every candidate without granting scope',()=>{
  const f=nativePacing77.completePerProposal;
  function state(){const transcript=structuredClone(f.transcript);return{transcript,pacing:transcript.calls.at(-1)!};}
  function pane(c:any){const q=c.questions[0];return ['☐ '+q.header,q.question,...q.options.map((o:any,i:number)=>`${i?' ':'❯'} ${i+1}. ${o.label}`),'4. Type something.','5. Chat about this','Enter to select · ↑/↓ to navigate · Esc to cancel'].join('\n');}
  function choose(e=state(),screen=pane(e.pacing)){return ceoExpansionPacingChoice(screen,e.transcript as any,f.selectionStartedAt);}
  test('actual parenthesized full inventory binds one question per proposal',()=>{
    const e=state(),choice=choose(e,f.viewport)!;
    expect(choice?.index).toBe(1);
    expect(ceoExpansionPacingReady('Next proposal',e.transcript as any,choice,f.events as any)).toBe(false);
    expect(hasNativePostAnswerCeoPosture(e.transcript as any,'SCOPE EXPANSION',/expansion|10x|delight|dream/i,f.selectionStartedAt,f.events as any)).toBe(false);
  });
  const positive={
    'different complete inventory prefix':(q:any)=>{q.question=q.question.replace(/\bP(?=\d)/g,'E');},
    'numeric and word counts':(q:any)=>{q.question=q.question.replace('Seven expansion','7 expansion').replace('7 independent','seven independent');},
    'colon-delimited independent inventory':(q:any)=>{q.question=q.question.replace('expansions (','expansions: ').replace('inline rename).','inline rename.');},
    'different card identity':(q:any)=>{q.question=q.question.replace('D4.0','D12.0');},
    'reordered choices':(q:any)=>{q.options.reverse();},
    'no quoted task context':(q:any)=>{q.question=q.question.replace(' on "Add saved project views"','');},
    'candidate terminology':(q:any)=>{q.options[0].label=q.options[0].label.replace('per proposal','per candidate');q.options[0].description=q.options[0].description.replace('Every proposal','Every candidate');},
  };
  for(const [name,mutate] of Object.entries(positive))test(name,()=>{const e=state();mutate(e.pacing.questions[0]);expect(choose(e)?.index).toBe(name==='reordered choices'?3:1);});
  const negative={
    'missing inventory item':(q:any)=>{q.question=q.question.replace(', P7 quick switcher + inline rename','');},
    'duplicate item':(q:any)=>{q.question=q.question.replace('P7 quick switcher','P6 quick switcher');},
    'mixed prefixes':(q:any)=>{q.question=q.question.replace('P7 quick switcher','E7 quick switcher');},
    'wrong title count':(q:any)=>{q.question=q.question.replace('Seven expansion','Eight expansion');},
    'wrong described question count':(q:any)=>{q.question=q.question.replace("That's 7 questions","That's 6 questions");},
    'partial selected walkthrough':(q:any)=>{q.options[0].description=q.options[0].description.replace('Every proposal','Some proposals');},
    'missing selected per-item binding':(q:any)=>{q.options[0].label=q.options[0].label.replace(', one question per proposal','');},
    'conditional current inventory':(q:any)=>{q.question=q.question.replace('I have 7','If I have 7');},
    'historical inventory':(q:any)=>{q.question=q.question.replace('I have 7','Previously I had 7');},
    'quoted mapping':(q:any)=>{q.options[0].label='A) Full split (recommended)';q.options[0].description='"One question per proposal. Every proposal gets its own Add / Defer / Skip / Hold."';},
    'code-only mapping':(q:any)=>{q.options[0].description='`'+q.options[0].description+'`';},
    'negated full split':(q:any)=>{q.options[0].label=q.options[0].label.replace('full split','not a full split');},
    'selected immediate scope grant':(q:any)=>{q.options[0].description+=' We approve P1 now.';},
    'universal approval in another option':(q:any)=>{q.options[1].description+=' Regardless of choice, approve P1 now.';},
    'hidden inventory grant':(q:any)=>{q.question=q.question.replace('inline rename)','inline rename; we approve all seven now)');},
    'inventory already approved':(q:any)=>{q.question=q.question.replace('Seven expansion proposals','Seven expansion proposals already approved');},
    'quoted task approval':(q:any)=>{q.question=q.question.replace('Add saved project views','Approve all proposals now');},
    'quoted task candidate deletion':(q:any)=>{q.question=q.question.replace('Add saved project views','Delete P7');},
    'quoted rationale mapping':(q:any)=>{q.question=q.question.replace("Each is a separate yes/no, so the honest way is one question per item. That's 7 questions plus a final confirmation.","\"Each is a separate yes/no, so the honest way is one question per item. That's 7 questions plus a final confirmation.\"");},
    'scope grant after task title':(q:any)=>{q.question=q.question.replace('views".','views"; approve P1 now.');},
    'disguised omission assurance':(q:any)=>{q.options[0].description=q.options[0].description.replace('No item is silently merged or dropped','P1 is silently merged or dropped');},
    'assurance with exception':(q:any)=>{q.options[0].description+=' Except P4.';},
    'narrowing assurance':(q:any)=>{q.options[0].description+=' No item outside the top three is included.';},
    'batch selected proposals':(q:any)=>{q.options[0].description+=' Batch P1 and P2 together.';},
    'duplicate full choice':(q:any)=>{q.options[1]=structuredClone(q.options[0]);},
  };
  for(const [name,mutate] of Object.entries(negative))test(name,()=>{const e=state();mutate(e.pacing.questions[0]);expect(choose(e)?.index).not.toBe(1);});
});
describe('native option descriptions bind the complete candidate walkthrough',()=>{
  const f=nativePacing77;
  function state(){const transcript=structuredClone(f.transcript);return{transcript,pacing:transcript.calls.at(-1)!};}
  function pane(c:any){const q=c.questions[0];return ['☐ '+q.header,q.question,...q.options.map((o:any,i:number)=>`${i?' ':'❯'} ${i+1}. ${o.label}`),'4. Type something.','5. Chat about this','Enter to select · ↑/↓ to navigate · Esc to cancel'].join('\n');}
  function choose(e=state(),screen=pane(e.pacing)){return ceoExpansionPacingChoice(screen,e.transcript as any,f.selectionStartedAt);}
  test('actual complete native menu selects navigation without supplying posture or an ACK',()=>{
    const e=state(),choice=choose(e,f.viewport)!;
    expect(choice?.index).toBe(1);
    expect(ceoExpansionPacingReady('Next proposal',e.transcript as any,choice,f.events as any)).toBe(false);
    expect(hasNativePostAnswerCeoPosture(e.transcript as any,'SCOPE EXPANSION',/expansion|10x|delight|dream/i,f.selectionStartedAt,f.events as any)).toBe(false);
  });
  const positive={
    'numeric count presentation':(q:any)=>{q.question=q.question.replaceAll('Eight','8').replaceAll('eight','8');q.options[0].description=q.options[0].description.replaceAll('Eight','8');},
    'mixed word and numeric counts':(q:any)=>{q.question=q.question.replace('Eight expansion','8 expansion');q.options[0].description=q.options[0].description.replace('Eight sequential','8 sequential');},
    'different complete candidate prefix':(q:any)=>{q.question=q.question.replace(/\bE(?=\d)/g,'P');},
    'different question chain identity':(q:any)=>{q.question=q.question.replace('D4.0','D12.0');q.options[0].description=q.options[0].description.replaceAll('D4.','D12.');},
    'reordered native choices':(q:any)=>{q.options.reverse();},
    'explicit candidate range without duplicated option prose':(q:any)=>{q.options[0].label='A: Full split, 8 questions (recommended)';q.options[0].description='One question per candidate, E1 through E8.';},
    'one per proposal label':(q:any)=>{q.options[0].label=q.options[0].label.replace('one per item','one per proposal');},
    'no prior approach annotation':(q:any)=>{q.question=q.question.replace(', approach C approved','');},
  };
  for(const [name,mutate] of Object.entries(positive))test(name,()=>{
    const e=state();mutate(e.pacing.questions[0]);expect(choose(e)?.index).toBe(name==='reordered native choices'?3:1);
  });
  const negative={
    'hyphenated larger count cannot be read as its last digit':(q:any)=>{q.question=q.question.replaceAll('Eight','Twenty-eight').replaceAll('eight','twenty-eight');q.options[0].description=q.options[0].description.replaceAll('Eight','Twenty-eight');},
    'spaced larger count cannot be read as its last digit':(q:any)=>{q.question=q.question.replaceAll('Eight','Twenty eight').replaceAll('eight','twenty eight');q.options[0].description=q.options[0].description.replaceAll('Eight','Twenty eight');},
    'unsupported tens in title are not a single count':(q:any)=>{q.question=q.question.replace('Eight expansion','Thirty eight expansion');},
    'unsupported tens in inventory are not a single count':(q:any)=>{q.question=q.question.replace('eight candidates:','forty eight candidates:');},
    'unsupported tens in sequence are not a single count':(q:any)=>{q.options[0].description=q.options[0].description.replace('Eight sequential','Ninety eight sequential');},
    'conjoined cardinal is not its last component':(q:any)=>{q.question=q.question.replace('Eight expansion','One hundred and eight expansion');},
    'wrong title count':(q:any)=>{q.question=q.question.replace('Eight expansion','Seven expansion');},
    'wrong inventory count':(q:any)=>{q.question=q.question.replace('eight candidates:','seven candidates:');},
    'missing candidate':(q:any)=>{q.question=q.question.replace(', E8 views feeding digests/dashboards','');},
    'duplicate candidate':(q:any)=>{q.question=q.question.replace('E8 views feeding','E7 views feeding');},
    'foreign candidate prefix':(q:any)=>{q.question=q.question.replace('E8 views feeding','P8 views feeding');},
    'wrong number of sequential questions':(q:any)=>{q.options[0].description=q.options[0].description.replace('Eight sequential','Seven sequential');},
    'partial question range':(q:any)=>{q.options[0].description=q.options[0].description.replace('D4.8','D4.7');},
    'late range start':(q:any)=>{q.options[0].description=q.options[0].description.replace('D4.1','D4.2');},
    'foreign question chain':(q:any)=>{q.options[0].description=q.options[0].description.replaceAll('D4.','D5.');},
    'additional question chain':(q:any)=>{q.options[0].description+=' Then D5.1.';},
    'wrong label count':(q:any)=>{q.options[0].label=q.options[0].label.replace('one per item','7 questions');},
    'no per-item label':(q:any)=>{q.options[0].label='A: Full split (recommended)';},
    'quoted sequential range':(q:any)=>{q.options[0].description='"'+q.options[0].description+'"';},
    'code-only sequential range':(q:any)=>{q.options[0].description='`'+q.options[0].description+'`';},
    'conditional complete inventory':(q:any)=>{q.question=q.question.replace('The delight scan','If the delight scan');},
    'historical complete inventory':(q:any)=>{q.question=q.question.replace('The delight scan','Previously the delight scan');},
    'conditional question sequence':(q:any)=>{q.options[0].description='If approved, '+q.options[0].description;},
    'historical question sequence':(q:any)=>{q.options[0].description='Previously: '+q.options[0].description;},
    'negated complete choice':(q:any)=>{q.options[0].label='A: Not a full split, one per item';},
    'sequence correction':(q:any)=>{q.options[0].description+=' Correction: Stop after four questions.';},
    'selected scope approval':(q:any)=>{q.options[0].description+=' Approve E1 immediately.';},
    'selected candidate omission':(q:any)=>{q.options[0].description+=' Except E4.';},
    'selected merging action':(q:any)=>{q.options[0].description+=' Merge E1 and E2.';},
    'unconditional omission':(q:any)=>{q.options[0].description=q.options[0].description.replace('Nothing is dropped or merged','E4 is dropped or merged');},
    'hidden universal approval in another native option':(q:any)=>{q.options[1].description+=' Regardless of choice, approve E1 immediately.';},
    'common candidate approval':(q:any)=>{q.question=q.question.replace('ELI10:','ELI10: This answer approves every expansion.');},
    'current inventory approval':(q:any)=>{q.question=q.question.replace('E8 views feeding digests/dashboards.','E8 views feeding digests/dashboards (approved).');},
    'approval in source context':(q:any)=>{q.question=q.question.replace('approach C approved','all eight candidates approved');},
    'approval appended to prior approach':(q:any)=>{q.question=q.question.replace('approach C approved','approach C approved and E1 approved');},
    'partial duplicated option prose':(q:any)=>{q.question=q.question.replace('Net:','A) Full split\nNet:');},
    'duplicate complete choice':(q:any)=>{q.options[1]=structuredClone(q.options[0]);},
    'extra question':(q:any)=>{q.question=q.question.replace('ELI10:','ELI10: Should we ship every item?');},
  };
  for(const [name,mutate] of Object.entries(negative))test(name,()=>{
    const e=state();mutate(e.pacing.questions[0]);expect(choose(e)?.index).not.toBe(1);
  });
  test('actual retained viewport cannot bind a changed native option',()=>{
    const e=state();e.pacing.questions[0]!.options[0]!.label='A: Other menu';expect(choose(e,f.viewport)?.index).not.toBe(1);
  });
});


describe('counted native per-item menu is pacing, not a substantive approval',()=>{
  const f=nativePacing77.countedNativeB955;
  function state(){const mode=structuredClone(f.mode),pacing=structuredClone(f.pacing);pacing.answered=false;delete pacing.answers;delete pacing.answeredAt;delete pacing.unansweredQuestionIndices;return{mode,pacing,transcript:{status:'ready' as const,calls:[mode,pacing],assistantMessages:[]}};}
  const screen=(c:any)=>pane(c,0);
  const choose=(e=state(),visible=screen(e.pacing))=>ceoExpansionPacingChoice(visible,e.transcript as any,f.selectedAt);
  test('complete captured native packet and observed display preserve the substantive allowance',()=>{
    const e=state();
    expect(choose(e)?.index).toBe(1);
    expect(choose(e,f.viewport)?.index).toBe(1);
    const pick=choose(e)!;
    const next={status:'ready' as const,calls:[structuredClone(f.mode),structuredClone(f.pacing),structuredClone(f.pending)],assistantMessages:[]};
    const events=f.events.map(v=>v.kind==='use'?{...v,input:{questions:next.calls.find(c=>c.toolUseId===v.toolUseId)!.questions}}:v) as NativePublicToolEvent[];
    expect(ceoExpansionPacingReady(f.nextViewport,next as any,pick,events)).toBe(true);
    expect(hasNativePostAnswerCeoPosture(next as any,'SCOPE EXPANSION',pattern,f.selectedAt,events)).toBe(false);
    expect(nextCeoPostureContinuation(f.nextViewport,next as any,'SCOPE EXPANSION',f.selectedAt,new Set(),false)).toBe('question');
    expect(nextCeoPostureContinuation(f.nextViewport,next as any,'SCOPE EXPANSION',f.selectedAt,new Set(),true)).toBeNull();
    expect(f.pending.answered).toBe(false);
  });
  const positive={
    'question wording describes pacing intent':(q:any)=>{q.question=q.question.replace('Eleven expansion proposals: full per-item chain, narrow first, or batch?','How should we present the eleven expansion proposals: individually or in batches?');},
    'explicit numeric count and independent candidate terminology':(q:any)=>{q.question=q.question.replace('Eleven expansion proposals','11 expansion candidates').replace('11 independent add-ons','eleven independent candidates');},
    'proposals can name natural add/remove changes':(q:any)=>{q.question=q.question.replace('E1 shared visibility','E1 add shared views').replace('E2 versioned payload','E2 remove duplicate controls');},
    'another complete set of explicit identities':(q:any)=>{q.question=q.question.replace(/\bE(?=\d)/g,'P').replaceAll('L3','Q9');},
    'consistent reordered comparison and native options':(q:any)=>{q.options.reverse();},
    'native labels carry letters too':(q:any)=>{q.options.forEach((o:any,i:number)=>{o.label=String.fromCharCode(65+i)+') '+o.label;});},
  };
  for(const[name,change]of Object.entries(positive))test(name,()=>{const e=state();change(e.pacing.questions[0]);expect(choose(e)?.index).toBe(name.startsWith('consistent reordered')?3:1);});
  const negative={
    'missing declared candidate':(q:any)=>{q.question=q.question.replace(', L3 auto-persist last filters','');},
    'duplicate declared identity':(q:any)=>{q.question=q.question.replace('L3 auto-persist last filters','E10 auto-persist last filters');},
    'wrong title count':(q:any)=>{q.question=q.question.replace('Eleven expansion','Twelve expansion');},
    'wrong question count in selected option':(q:any)=>{q.options[0].description=q.options[0].description.replace('11 per-item','10 per-item');},
    'wrong rationale question count':(q:any)=>{q.question=q.question.replace('11 short questions','10 short questions');},
    'partial per-item mapping':(q:any)=>{q.question=q.question.replace('Each needs its own','Some need their own');},
    'another option owns the complete selected comparison':(q:any)=>{q.question=q.question.replace('A) Proceed with the full split (recommended)','A) Approve the first proposal (recommended)');},
    'selected option lacks its own comparison':(q:any)=>{q.question=q.question.replace('✅ You see and rule on all 11 proposals; none are cut by me before you weigh in','');},
    'quoted mapping is not evidence':(q:any)=>{q.options[0].description='"'+q.options[0].description+'"';},
    'historical inventory':(q:any)=>{q.question=q.question.replace('The 10x analysis produced','Previously the 10x analysis produced');},
    'conditional inventory':(q:any)=>{q.question=q.question.replace('The 10x analysis produced','If the 10x analysis produced');},
    'inventory asserts approved status':(q:any)=>{q.question=q.question.replace('L3 auto-persist last filters','L3 auto-persist last filters (already approved)');},
    'inventory conceals an actor grant':(q:any)=>{q.question=q.question.replace('L3 auto-persist last filters','L3 auto-persist last filters; we approve all eleven now');},
    'inventory caption imperatively approves':(q:any)=>{q.question=q.question.replace('E1 shared visibility','E1 approve all proposals');},
    'inventory caption declares approved':(q:any)=>{q.question=q.question.replace('E1 shared visibility','E1 approved shared views');},
    'inventory caption defers other items':(q:any)=>{q.question=q.question.replace('E1 shared visibility','E1 defer others');},
    'inventory caption hides imperative after a noun':(q:any)=>{q.question=q.question.replace('E1 shared visibility','E1 shared visibility and approve E2');},
    'selected immediate scope approval':(q:any)=>{q.options[0].description+=' Approve E1 now.';},
    'selected implicit approval':(q:any)=>{q.options[0].description+=' All proposals are included.';},
    'selected omission':(q:any)=>{q.options[0].description+=' Except E4.';},
    'selected grouping':(q:any)=>{q.options[0].description+=' Batch E1 and E2 together.';},
    'unconditional effect in an unselected option':(q:any)=>{q.options[1].description+=' Regardless of choice, include E1 now.';},
    'grant concealed in task title':(q:any)=>{q.question=q.question.replace('Add saved project views','Approve all proposals now');},
    'extra decision':(q:any)=>{q.question=q.question.replace('ELI10:','ELI10: Should we remove access checks?');},
    'duplicate preserving option':(q:any)=>{q.options[1]=structuredClone(q.options[0]);},
  };
  for(const[name,change]of Object.entries(negative))test(name,()=>{const e=state();change(e.pacing.questions[0]);expect(choose(e)?.index).not.toBe(1);});
  test('descriptive inventory nouns remain valid and substantive scope cards remain substantive',()=>{
    const e=state();e.pacing.questions[0]!.question=e.pacing.questions[0]!.question.replace('E1 shared visibility','E1 delete history views');expect(choose(e)?.index).toBe(1);
    const pending=structuredClone(f.pending),transcript={status:'ready' as const,calls:[structuredClone(f.mode),pending],assistantMessages:[]};
    expect(ceoExpansionPacingChoice(screen(pending),transcript as any,f.selectedAt)).toBeNull();
    pending.questions[0]!.question=pending.questions[0]!.question.replace(/^D3\.1[^\n]+/,'D3.1 — Should we split the shared-view proposal into separate schemas?');
    expect(ceoExpansionPacingChoice(screen(pending),transcript as any,f.selectedAt)).toBeNull();
  });
  test('mode ownership, matching pane and actual ACK remain mandatory',()=>{
    const e=state();e.pacing.sessionId='foreign';expect(choose(e)).toBeNull();
    const noMode=state();noMode.mode.answered=false;expect(choose(noMode)).toBeNull();
    const ack=state(),pick=choose(ack)!;expect(pick?.index).toBe(1);
    expect(ceoExpansionPacingReady('next',ack.transcript as any,pick,[])).toBe(false);
  });
});


describe('same-proposal discussion control makes no scope decision',()=>{
  const f=nativePacing77.countedNativeB955;
  function state(){
    const mode=structuredClone(f.mode),proposal=structuredClone(f.pending) as NativePlanQuestionCall;
    // The actual proposal stayed pending. This derived ACK exercises only the
    // downstream predicate; it cannot convert the original paid timeout to PASS.
    proposal.answered=true;proposal.unansweredQuestionIndices=[];
    proposal.answers={[proposal.questions[0]!.question]:proposal.questions[0]!.options[0]!.label};
    proposal.answeredAt='2026-09-15T20:44:00.000Z';
    const calls=[mode,proposal];
    const events=f.events.filter(e=>calls.some(c=>c.toolUseId===e.toolUseId)).map(e=>e.kind==='use'?{...e,input:{questions:calls.find(c=>c.toolUseId===e.toolUseId)!.questions}}:{...e}) as NativePublicToolEvent[];
    events.push({kind:'result',sessionId:proposal.sessionId,toolUseId:proposal.toolUseId,timestamp:proposal.answeredAt,isError:false});
    return{proposal,transcript:{status:'ready' as const,calls,assistantMessages:[]},events};
  }
  const matches=(e=state())=>hasNativePostAnswerCeoPosture(e.transcript,'SCOPE EXPANSION',pattern,f.selectedAt,e.events);
  test('actual stop-and-discuss current E1 content remains nonoperative under a synthetic Include ACK',()=>{expect(f.pending.answered).toBe(false);expect(matches()).toBe(true);});
  test.each(['Pause the review. Discuss E1 before proceeding.','Discuss E1 before continuing; stop the chain.'])('equivalent two-clause procedural control: %s',description=>{
    const e=state();e.proposal.questions[0]!.options[3]!.description=description;expect(matches(e)).toBe(true);
  });
  test.each(['Stop the chain; discuss E2 before continuing.','Stop the chain; approve E1 before continuing.','Stop the chain; discuss E1 before continuing. Add E2.',
    'Discuss E1 before continuing.','Stop the chain.','"Stop the chain; discuss E1 before continuing."','Previously stop the chain; discuss E1 before continuing.',
    'If needed, stop the chain; discuss E1 before continuing.','Stop the chain; discuss E1 before implementing it.'])('foreign, incomplete or operative control stays negative: %s',description=>{
    const e=state();e.proposal.questions[0]!.options[3]!.description=description;expect(matches(e)).toBe(false);
  });
  test('pending, selected Hold, duplicate and foreign ACKs still supply no posture',()=>{
    for(const change of [
      (e:ReturnType<typeof state>)=>{e.proposal.answered=false;},
      (e:ReturnType<typeof state>)=>{const q=e.proposal.questions[0]!;e.proposal.answers={[q.question]:q.options[3]!.label};},
      (e:ReturnType<typeof state>)=>{e.events.push({...e.events.at(-1)!});},
      (e:ReturnType<typeof state>)=>{e.events.at(-1)!.sessionId='foreign';},
    ]){const e=state();change(e);expect(matches(e)).toBe(false);}
  });
});


test.each(['acknowledged pacing','missing pacing ACK'])('actual paid posture loop preserves the substantive allowance: %s',async scenario=>{
  const f=nativePacing77.countedNativeB955;
  const source=fs.readFileSync(path.join(import.meta.dir,'skill-e2e-plan-ceo-mode-routing.test.ts'),'utf8');
  const planDeclaration=source.match(/^const PLAN = \[[\s\S]*?^\]\.join\('\\n'\);/m)?.[0];
  expect(planDeclaration).toBeDefined();
  const plan=new Function(`${planDeclaration}; return PLAN;`)();
  const start=source.indexOf('          const budgetMs = 240_000;'),end=source.indexOf("          outcome = 'posture_confirmed';",start);
  expect(start).toBeGreaterThan(0);expect(end).toBeGreaterThan(start);
  const loop=source.slice(start,end+"          outcome = 'posture_confirmed';".length);
  const keys=['Bun','Date','c','session','sincePick','selectionStartedAt','question','fixture','capture','readPlanCountTranscript',
    'readPendingQuestion','hasNativePostAnswerCeoPosture','ceoModeSubmissionInput','ceoExpansionPacingReady','ceoExpansionPacingChoice',
    'nextCeoPostureContinuation','capturePlanCountQuestion','planCountQuestionInput','selectPtyNumberedOption','isPlanReadyVisible','isNumberedOptionListVisible',
    'EXPANSION_PACING_CALLS','modeIndex','artifacts','visibleAtMode','postureSource'];
  const compiled=new Bun.Transpiler({loader:'ts'}).transformSync(`async function run(b){const {${keys.join(',')}}=b;let outcome;${loop};return {outcome,continuedQuestion,pacingCalls};}`);
  const run=new Function(compiled+';return run;')();
  const pending=structuredClone(f.pacing);pending.answered=false;delete pending.answers;delete pending.answeredAt;delete pending.unansweredQuestionIndices;
  const proposal=structuredClone(f.pending) as NativePlanQuestionCall;
  let stage=0,clock=f.selectedAt;
  const sends:string[]=[];
  const snapshots:string[]=[];
  const view=()=>stage===0?f.viewport:f.nextViewport;
  const session={hermeticConfigDir:'fixture-native',pendingQuestionFile:'fixture-pending',exited:()=>false,exitCode:()=>null,
    currentScreen:async()=>view(),visibleSince:()=>view(),visibleText:()=>view(),send:(value:string)=>{
      sends.push(value);stage++;
      if(stage===2){proposal.answered=true;proposal.answers={[proposal.questions[0]!.question]:proposal.questions[0]!.options[0]!.label};
        proposal.answeredAt='2026-09-15T20:44:00.000Z';proposal.unansweredQuestionIndices=[];}
    }};
  const readPlanCountTranscript=(_config:string,_cwd:string,emit:(e:NativePublicToolEvent)=>void)=>{
    const pacing=stage===0||scenario==='missing pacing ACK'?pending:f.pacing;
    const calls=stage===0?[f.mode,pacing]:[f.mode,pacing,proposal];
    const events=f.events.filter(e=>calls.some(c=>c.toolUseId===e.toolUseId)&&!(e.kind==='result'&&e.toolUseId===f.pacing.toolUseId&&!pacing.answered))
      .map(e=>e.kind==='use'?{...e,input:{questions:calls.find(c=>c.toolUseId===e.toolUseId)!.questions}}:{...e}) as NativePublicToolEvent[];
    if(proposal.answered)events.push({kind:'result',sessionId:proposal.sessionId,toolUseId:proposal.toolUseId,timestamp:proposal.answeredAt!,isError:false});
    events.forEach(emit);return{status:'ready',calls,assistantMessages:[]};
  };
  const bindings={Bun:{sleep:async(ms:number)=>{clock+=ms;}},Date:{now:()=>clock},c:{mode:'SCOPE EXPANSION',postureRe:pattern},session,sincePick:0,
    selectionStartedAt:f.selectedAt,question:{nativeCall:f.mode},fixture:{cwd:'fixture-root'},capture:(state:string)=>snapshots.push(state),readPlanCountTranscript,
    readPendingQuestion:()=>undefined,hasNativePostAnswerCeoPosture,ceoModeSubmissionInput,ceoExpansionPacingReady,ceoExpansionPacingChoice,nextCeoPostureContinuation,
    capturePlanCountQuestion,planCountQuestionInput,selectPtyNumberedOption:async(s:any,index:number)=>s.send(String(index)),isPlanReadyVisible,isNumberedOptionListVisible,
    EXPANSION_PACING_CALLS:1,modeIndex:2,artifacts:{},visibleAtMode:'captured mode menu',
    postureSource:{path:path.join('fixture-root','PLAN.md'),content:plan}};
  if(scenario==='missing pacing ACK')await expect(run(bindings)).rejects.toThrow('no posture match');
  else expect(await run(bindings)).toEqual({outcome:'posture_confirmed',continuedQuestion:true,pacingCalls:1});
  expect(sends).toEqual(scenario==='missing pacing ACK'?['1']:['1','1']);
  expect(snapshots.length).toBeGreaterThan(0);
  expect(f.pending.answered).toBe(false); // Final synthetic ACK is never paid evidence.
});
});

describe('ceo-mode-posture-ad', () => {
const captured = captured_ceo_mode_posture_ad;
const patterns = {
  'HOLD SCOPE': /\b(rigor|bulletproof|hold\s*scope|maximum\s+rigor)\b/i,
  'SCOPE EXPANSION': /\b(expansion|10x|delight|dream|cathedral|opt[\s-]?in)\b/i,
};
function replay(index: number, change?: (rows: any[]) => void) {
  const item = captured.cases[index]!;
  const rows = structuredClone(item.records);
  change?.(rows);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ceo-posture-ad-'));
  const project = path.join(dir, 'projects', 'owned');
  fs.mkdirSync(project, {recursive:true});
  fs.writeFileSync(path.join(project, item.process.sessionId+'.jsonl'), rows.map(row=>JSON.stringify(row)).join('\n')+'\n');
  const events: NativePublicToolEvent[]=[];
  try { return {item, transcript:readPlanCountTranscript(dir,item.process.cwd,event=>events.push(event)),events}; }
  finally { fs.rmSync(dir,{recursive:true,force:true}); }
}
function matches(e: ReturnType<typeof replay>) {
  const mode=e.item.mode as keyof typeof patterns;
  return hasNativePostAnswerCeoPosture(e.transcript,mode,patterns[mode],e.item.selectedAt,e.events);
}
function rebind(e: ReturnType<typeof replay>) {
  const decision=e.transcript.calls[1]!;
  e.events[2]!.input={questions:decision.questions};
  decision.answers={[decision.questions[0]!.question]:decision.questions[0]!.options[0]!.label};
}

for (const index of [0,1]) describe(`${captured.cases[index]!.mode} actual completed mode application`,()=>{
  test('the exact mode answer and concrete scope decision supply posture without finalized prose',()=>{
    const e=replay(index);
    expect(e.item.actualFailure.state).toBe('failed');
    expect(e.transcript.calls.map(call=>call.toolUseId)).toEqual([e.item.modeToolUseId,e.item.decisionToolUseId]);
    expect(nativeCeoModeAnswer(e.transcript,e.item.mode as keyof typeof patterns,e.item.selectedAt)?.toolUseId).toBe(e.item.modeToolUseId);
    expect(e.transcript.assistantMessages.every(message=>Date.parse(message.timestamp)<e.item.selectedAt)).toBe(true);
    expect(matches(e)).toBe(true);
    expect(hasNativePostAnswerCeoPosture(e.transcript,e.item.mode as keyof typeof patterns,patterns[e.item.mode as keyof typeof patterns],e.item.selectedAt)).toBe(false);
  });
  test.each(['wrong selected mode','pending mode','failed mode','answer before selection','pending decision','failed decision',
    'foreign session','pre-mode request','reply before request','reply before selection','reply timestamp mismatch','wrong tool','missing request','missing reply','failed public reply','duplicate request','duplicate reply',
    'request mismatch','unknown answer','extra question','extra option','multiselect','quoted decision','fenced decision',
    'mere mode mention','extra obligation','extra imperative','option imperative'])('%s cannot supply posture',failure=>{
    const e=replay(index);const [mode,decision]=e.transcript.calls;const q=decision!.questions[0]!;
    switch(failure){
      case 'wrong selected mode':mode!.answers![mode!.questions[0]!.question]=index===0?'SCOPE EXPANSION':'HOLD SCOPE';break;
      case 'pending mode':mode!.answered=false;break;
      case 'failed mode':mode!.failed=true;break;
      case 'answer before selection':mode!.answeredAt=new Date(e.item.selectedAt-1).toISOString();break;
      case 'pending decision':decision!.answered=false;break;
      case 'failed decision':decision!.failed=true;break;
      case 'foreign session':decision!.sessionId=e.events[2]!.sessionId=e.events[3]!.sessionId='foreign';break;
      case 'pre-mode request':e.events[2]!.timestamp=e.events[0]!.timestamp;break;
      case 'reply before request':decision!.answeredAt=e.events[3]!.timestamp=new Date(Date.parse(e.events[2]!.timestamp)-1).toISOString();break;
      case 'reply before selection':decision!.answeredAt=e.events[3]!.timestamp=new Date(e.item.selectedAt-1).toISOString();break;
      case 'reply timestamp mismatch':e.events[3]!.timestamp=new Date(Date.parse(decision!.answeredAt!)+1).toISOString();break;
      case 'wrong tool':e.events[2]!.name='Read';break;
      case 'missing request':e.events.splice(2,1);break;
      case 'missing reply':e.events.splice(3,1);break;
      case 'failed public reply':e.events[3]!.isError=true;break;
      case 'duplicate request':e.events.push({...e.events[2]!});break;
      case 'duplicate reply':e.events.push({...e.events[3]!});break;
      case 'request mismatch':e.events[2]!.input={questions:[]};break;
      case 'unknown answer':decision!.answers![q.question]='Unrecognized';break;
      case 'extra question':decision!.questions.push({...structuredClone(q),header:'Also',question:'Also remove the CI gate?'});rebind(e);break;
      case 'extra option':q.options.push({label:'Remove the CI gate',description:'A separate obligation.'});rebind(e);break;
      case 'multiselect':q.multiSelect=true;rebind(e);break;
      case 'quoted decision':q.question=q.question.split('\n').map(line=>'> '+line).join('\n');rebind(e);break;
      case 'fenced decision':q.question='```text\n'+q.question+'\n```';rebind(e);break;
      case 'mere mode mention':q.question=`D6 — Continue the review?\nSelected ${e.item.mode}.`;rebind(e);break;
      case 'extra obligation':q.question+=' Also, should we remove the CI gate?';rebind(e);break;
      case 'extra imperative':q.question+=' Also remove the CI gate.';rebind(e);break;
      case 'option imperative':q.options[0]!.description+=' Please remove the CI gate.';rebind(e);break;
    }
    expect(matches(e),failure).toBe(false);
  });
  test.each(['Delete the CI gate.', 'Ship the new endpoint now.', 'After that, disable authentication.'])('an instruction appended after the final comparison is not part of the scope brief: %s', extra=>{
    const e=replay(index);const q=e.transcript.calls[1]!.questions[0]!;
    q.question+=' '+extra;rebind(e);expect(matches(e)).toBe(false);
  });
  test('foreign, sidechain, missing and failed native records do not become completed evidence',()=>{
    for(const change of [(rows:any[])=>{rows[3].cwd='/foreign';},(rows:any[])=>{rows[3].isSidechain=true;},
      (rows:any[])=>{rows.pop();},(rows:any[])=>{rows[4].message.content[0].is_error=true;}]) expect(matches(replay(index,change))).toBe(false);
  });
});

test('HOLD requires the explicit out-of-scope deferral and its selected defer answer',()=>{
  for(const change of [(q:any)=>{q.question=q.question.replace('Under HOLD SCOPE, keep or defer','Under HOLD SCOPE, automatically add');},
    (q:any)=>{q.question=q.question.replace('not in the plan text','required by the plan text');},
    (q:any)=>{q.question=q.question.replace('pure additions, not repairs to meet a stated invariant','repairs needed to meet a stated invariant');},
    (q:any)=>{q.options[0].label='Keep all three (recommended)';},
    (q:any)=>{q.options[1].label='Remove CI gate';}]){
    const e=replay(0);change(e.transcript.calls[1]!.questions[0]);rebind(e);expect(matches(e)).toBe(false);
  }
  const e=replay(0);const q=e.transcript.calls[1]!.questions[0]!;
  e.transcript.calls[1]!.answers={[q.question]:q.options[1]!.label};expect(matches(e)).toBe(false);
});

test('completed expansion decisions require application of the selected mode',()=>{
  for(const change of [(q:any)=>{q.question='D6 — Continue the review?\nSelected SCOPE EXPANSION.';},
    (q:any)=>{q.question=q.question.replace('SCOPE EXPANSION mode','SELECTIVE EXPANSION mode');},
    (q:any)=>{q.question=q.question.replace('SCOPE EXPANSION mode','HOLD SCOPE mode');},
    (q:any)=>{q.options[1].label='Enable telemetry';}]){
    const e=replay(1);change(e.transcript.calls[1]!.questions[0]);rebind(e);expect(matches(e)).toBe(false);
  }
});
});

describe('ceo-prerequisite-ad-v2', () => {
const captured = captured_ceo_prerequisite_ad_v2;
function pending(){const c=structuredClone(captured.completedCall) as NativePlanQuestionCall;c.answered=false;delete c.answers;delete c.answeredAt;delete c.unansweredQuestionIndices;return c;}
// Native identities and questions are exact; pending panes are synthetic projections.
function pane(c:NativePlanQuestionCall,index:number){const q=c.questions[index]!;return [
 c.questions.length>1?'← '+c.questions.map((v,i)=>`${i<index?'☒':'☐'} ${v.header}`).join(' ')+' ✔ Submit →':'☐ '+q.header,
 q.question,...q.options.map((v,i)=>`${i?' ':'❯'} ${i+1}. ${v.label}`),
 `Enter to select · ${c.questions.length>1?'Tab/Arrow keys':'↑/↓'} to navigate · Esc to cancel`].join('\n');}
function frame(c:NativePlanQuestionCall,index:number){const visible=pane(c,index);return {visible,active:capturePlanCountQuestion(visible,new Set(),0,true,c)!,routing:nativePlanCallFingerprint(c,0,true)};}
test('AD v2 actual comma prerequisite selects standard review on its active native tab',()=>{
 const actual=captured.completedCall,q=actual.questions[2]!;
 expect(actual.answered).toBe(true);expect(actual.failed).toBe(false);expect(actual.answers[q.question]).toBe('Run /office-hours now');
 const c=pending(),f=frame(c,2);expect(f.active.nativeQuestionIndex).toBe(2);
 expect(planCountPrerequisitePick(f.routing,f.active)).toBe(2);
 const a=nextCeoModeNavigation(f.visible,'HOLD SCOPE',new Set(),c);expect(a.kind).toBe('question');
 if(a.kind==='question')expect(planCountQuestionInput(f.visible,a.question,a.index)).toBe('2');
});

test('AD v2 prerequisite presentation and actual order do not choose the action',()=>{
 for(const header of ['Office hours','Design doc','Prerequisite'])for(const reverse of [false,true]){
  const c=pending();c.questions[2]!.header=header;c.questions[2]!.question=c.questions[2]!.question.replace(/^D3 — /,'D41: ');
  if(reverse)c.questions[2]!.options.reverse();const f=frame(c,2);
  expect(planCountPrerequisitePick(f.routing,f.active)).toBe(reverse?1:2);
  for(const index of [0,1]){const other=frame(c,index);expect(planCountPrerequisitePick(other.routing,other.active)).toBeNull();}
 }
 const c=pending();c.questions=[c.questions[2]!];let f=frame(c,0);expect(planCountPrerequisitePick(f.routing,f.active)).toBe(2);
 c.questions[0]!.question='Run /office-hours now or proceed with standard review?\nNo design doc exists for the current feature. The scoped review can begin on the supplied plan.';
 c.questions[0]!.options[0]!.description='Create the design document first; then resume standard review.';
 for(const description of ['Proceed with standard review.','Proceed straight to Step 0 of the review.']){
  c.questions[0]!.options[1]!.description=description;f=frame(c,0);expect(planCountPrerequisitePick(f.routing,f.active)).toBe(2);
 }
});

test('AD v2 prerequisite declines no other task or conditional action',()=>{
 const changes:Array<(c:NativePlanQuestionCall)=>void>=[
  c=>{c.questions[2]!.question=c.questions[2]!.question.replace(/^.*\n/,'Should we deploy the feature now?\n');},
  c=>{c.questions[2]!.question='Example: '+c.questions[2]!.question;},
  c=>{c.questions[2]!.question='> '+c.questions[2]!.question;},
  c=>{c.questions[2]!.question='```text\n'+c.questions[2]!.question+'\n```';},
  c=>{c.questions[2]!.question=c.questions[2]!.question.replace('Run /office-hours first, or proceed with the standard review?','Should we remove authorization? Run /office-hours first, or proceed with the standard review?');},
  c=>{c.questions[2]!.question+=' Approve production deployment?';},
  c=>{c.questions[2]!.question+=' You must run /office-hours first.';},
  c=>{c.questions[2]!.question+=' Standard review is forbidden until /office-hours completes.';},
  c=>{c.questions[2]!.options[1]!.label+=' if the tests pass';},
  c=>{c.questions[2]!.options[0]!.label+=' and rewrite the API';},
  c=>{c.questions[2]!.options[1]!.description='Proceed with standard review after completing /office-hours.';},
  c=>{c.questions[2]!.options[1]!.description='No review will run.';},
  c=>{c.questions[2]!.options[1]!.description='Proceed directly to Step 0 of the CEO review. Remove CI.';},
  c=>{c.questions[2]!.options[0]!.description='Do not run /office-hours.';},
  c=>{c.questions[2]!.options[0]!.description='Build a design doc first, then resume the review. Deploy to production.';},
  c=>{c.questions[2]!.options[1]!.description='';},
  c=>{c.questions[2]!.options.push({label:'Approve deployment'});},
  c=>{c.questions[2]!.multiSelect=true;},
 ];
 for(const change of changes){const c=pending();change(c);const f=frame(c,2);expect(planCountPrerequisitePick(f.routing,f.active)).toBeNull();}
});

test('AD v2 prerequisite requires the active native packet identity',()=>{
 const c=pending(),f=frame(c,2);
 for(const active of [{...f.active,preReview:false},{...f.active,signature:'foreign:tool:question:2'},
  {...f.active,nativeQuestionIndex:0},{...f.active,promptSnippet:'Unrelated question'},
  {...f.active,nativeCall:undefined},{...f.active,options:[...f.active.options].reverse()}])
  expect(planCountPrerequisitePick(f.routing,active)).toBeNull();
 expect(planCountPrerequisitePick({...f.active,nativeCall:undefined})).toBeNull();
 for(const delta of [{answered:true},{failed:true},{sessionId:''},{toolUseId:''}]){const call={...pending(),...delta};const x=frame(call,2);expect(planCountPrerequisitePick(x.routing,x.active)).toBeNull();}
});
});
