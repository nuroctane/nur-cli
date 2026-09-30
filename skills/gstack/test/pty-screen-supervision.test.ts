import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const ROOT = path.resolve(import.meta.dir, '..');
const terminal = `class {
  unicode = { register() {}, activeVersion: '' };
  buffer = { active: { baseY: 0, getLine: () => ({ getCell: () => undefined, translateToString: () => 'partial viewport' }) } };
  write(text, done) {
    controls.writes++;
    if (text.includes('reject')) throw controls.original;
    if (text.includes('stall')) { controls.callback = done; return; }
    done(); done();
  }
  dispose() { controls.disposals++; if (controls.disposeThrows) throw new Error('secondary disposal failure'); }
}`;

function adapter() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pty-drain-'));
  const screen = path.join(dir, 'screen.ts');
  const runner = path.join(dir, 'runner.ts');
  fs.writeFileSync(screen, fs.readFileSync(path.join(ROOT, 'test/helpers/pty-screen.ts'), 'utf8')
    .replace('const Terminal = await loadTerminal();', `const Terminal = ${terminal};`) +
    `\nexport const controls = { writes: 0, disposals: 0, callback: undefined, disposeThrows: false, original: new Error('original parser rejection') };\n`);
  fs.writeFileSync(runner, fs.readFileSync(path.join(ROOT, 'test/helpers/claude-pty-runner.ts'), 'utf8')
    .replaceAll('fixture.cleanup();', 'globalThis.beforeFixtureCleanup?.(fixture); fixture.cleanup();')
    .replace(/from (['"])(\.\.?\/[^'"]+)\1/g, (_match, _quote, relative) =>
      'from ' + JSON.stringify(pathToFileURL(relative === './pty-screen' ? screen :
        path.resolve(ROOT, 'test/helpers', relative + '.ts')).href)));
  return { dir, screen: pathToFileURL(screen).href, runner: pathToFileURL(runner).href };
}

test('actual write callbacks settle once; stalled reads and disposal reject at the original absolute deadline', async () => {
  const a = adapter();
  try {
    const { createPtyScreen, controls } = await import(a.screen);
    const deadlineAt = performance.now() + 80;
    const screen = await createPtyScreen(10, 2, { deadlineAt });
    screen.write('complete');
    expect((await screen.readFrame()).inputOffset).toBe(8);
    screen.write('stall');
    const reads = [screen.read(), screen.readFrame()];
    const closing = screen.dispose();
    expect(screen.dispose()).toBe(closing);
    const settled = await Promise.allSettled([...reads, closing]);
    expect(settled.every(result => result.status === 'rejected')).toBe(true);
    expect(new Set(settled.map(result => (result as PromiseRejectedResult).reason)).size).toBe(1);
    expect(performance.now()).toBeLessThan(deadlineAt + 500);
    expect(controls.disposals).toBe(1);
    controls.callback(); controls.callback();
    await expect(screen.readFrame()).rejects.toThrow('viewport is incomplete');
    await expect(screen.dispose()).rejects.toThrow('viewport is incomplete');
  } finally { fs.rmSync(a.dir, { recursive: true, force: true }); }
});

test('cancellation and parser rejection preserve the first failure through repeated disposal', async () => {
  const a = adapter();
  try {
    const { createPtyScreen, controls } = await import(a.screen);
    const abort = new AbortController();
    const screen = await createPtyScreen(10, 2, { deadlineAt: performance.now() + 600_000, signal: abort.signal });
    screen.write('stall');
    const read = screen.readFrame();
    abort.abort(controls.original);
    const error = await read.catch((error: Error) => error);
    expect(error.cause).toBe(controls.original);
    await expect(screen.dispose()).rejects.toBe(error);
    const rejected = await createPtyScreen(10, 2);
    rejected.write('stall'); rejected.write('reject');
    controls.disposeThrows = true;
    const failure = await rejected.readFrame().catch((error: Error) => error);
    expect(failure.cause).toBe(controls.original);
    await expect(rejected.dispose()).rejects.toBe(failure);
    await expect(rejected.dispose()).rejects.toBe(failure);
    expect(controls.disposals).toBe(2);
  } finally { fs.rmSync(a.dir, { recursive: true, force: true }); }
});

test('actual PTY close bounds live, already-exited, wall, spawn-failure and unresponsive-child paths', () => {
  const a = adapter();
  const worker = path.join(a.dir, 'worker.ts');
  try {
    fs.writeFileSync(worker, `import {launchClaudePty} from ${JSON.stringify(a.runner)};
import {controls} from ${JSON.stringify(a.screen)};
const realSpawn = Bun.spawn;
for (const mode of ['live','exited','wall','unresponsive','spawn-failure']) {
  let resolveExit; const signals=[];
  Bun.spawn = (_args, opts) => {
    opts.terminal.data(null,Buffer.from('stall retained raw prefix'));
    if(mode==='spawn-failure') throw controls.original;
    return {pid:123,exited:mode==='exited'?Promise.resolve(0):new Promise(resolve=>resolveExit=resolve),
      kill:signal=>{signals.push(signal);if(mode!=='unresponsive')resolveExit(0);},terminal:{write(){}}};
  };
  const before=performance.now(), disposed=controls.disposals;
  if(mode==='spawn-failure') {
    const error=await launchClaudePty({observeScreen:true,timeoutMs:600000}).catch(error=>error);
    if(error!==controls.original)throw Error('spawn error replaced');
  } else {
    const session=await launchClaudePty({observeScreen:true,timeoutMs:mode==='wall'?60:600000});
    if(mode==='exited')await Bun.sleep(1);
    if(mode==='wall')await Bun.sleep(80);
    const first=session.close();if(session.close()!==first)throw Error('close promise changed');
    const read=session.currentScreenFrame();
    const results=await Promise.allSettled([first,read,session.close()]);
    if(results.some(result=>result.status!=='rejected'))throw Error('incomplete viewport passed');
    if(!session.rawOutput().includes('retained raw prefix'))throw Error('raw diagnostics lost');
    if(mode==='unresponsive'&&signals.join()!=='SIGINT,SIGKILL')throw Error('process cleanup skipped');
    if(mode==='exited'&&signals.length)throw Error('already-exited child signalled');
  }
  if(controls.disposals!==disposed+1)throw Error('screen disposed more than once');
  if(performance.now()-before>3500)throw Error('cleanup allowance exceeded');
  console.log(mode);
}
Bun.spawn=realSpawn;
`);
    const result = spawnSync(process.execPath, [worker], { cwd: ROOT, encoding: 'utf8', timeout: 15_000,
      env: { ...process.env, BROWSE_TERMINAL_BINARY: process.execPath, EVALS_HERMETIC: '0' } });
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout.trim().split('\n')).toEqual(['live','exited','wall','unresponsive','spawn-failure']);
  } finally { fs.rmSync(a.dir, { recursive: true, force: true }); }
}, 20_000);

test.each(['counting', 'floor'])('%s attempts retain public evidence before actual cleanup within unchanged clocks', (mode) => {
  const a = adapter();
  const worker = path.join(a.dir, 'budget-worker.ts');
  try {
    const entry = fs.readFileSync(path.join(ROOT, 'test/skill-e2e-plan-design-with-ui.test.ts'), 'utf8');
    expect(entry).toContain('timeoutMs: 600_000 - (Date.now() - startedAt)');
    fs.writeFileSync(worker, `import * as fs from 'node:fs';
import * as path from 'node:path';
import {runPlanSkillCounting,runPlanSkillFloorCheck} from ${JSON.stringify(a.runner)};
const mode=${JSON.stringify(mode)};
let clock=0, sequence=0;
const epoch=Date.now(), realSleep=Bun.sleep.bind(Bun), timers=new Map();
Object.defineProperty(performance,'now',{value:()=>clock});
Date.now=()=>epoch+clock;
globalThis.setTimeout=(callback,ms=0)=>{const id=++sequence;timers.set(id,{at:clock+Math.max(0,ms),callback});return id;};
globalThis.clearTimeout=id=>timers.delete(id);
globalThis.setInterval=()=>++sequence;globalThis.clearInterval=()=>{};
Bun.sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const attempts=[], base=${JSON.stringify(a.dir)};
for(let attempt=1;attempt<=2;attempt++) {
  const startedAt=Date.now(), timerCount=timers.size;
  clock+=1700;
  const config=path.join(base,'config-'+attempt), artifacts=path.join(base,'artifacts-'+attempt);
  fs.mkdirSync(path.join(config,'projects','public'),{recursive:true});
  process.env.GSTACK_EVAL_DIR=artifacts;
  let fixtureCwd, captured, cleaned=false, exited=false, signals=[];
  Bun.spawn=(_args,opts)=>{
    fixtureCwd=opts.cwd;
    let resolveExit;
    return {pid:100+attempt,exited:new Promise(resolve=>resolveExit=resolve),
      terminal:{write(input){if(!input.includes('/plan-design-review'))return;
        const question={header:'Layout',question:'Which layout should the UI use?',options:[{label:'One panel'},{label:'Two panels'}]};
        const event={cwd:opts.cwd,sessionId:'public',timestamp:new Date(Date.now()).toISOString(),isSidechain:false,
          message:{role:'assistant',content:[{type:'tool_use',id:'question-'+attempt,name:'AskUserQuestion',input:{questions:[question]}}]}};
        fs.appendFileSync(path.join(config,'projects/public/public.jsonl'),JSON.stringify(event)+'\\n');
        opts.terminal.data(null,Buffer.from('stall public question prefix attempt '+attempt));
      }},kill(signal){signals.push(signal);exited=true;resolveExit(0);}};
  };
  const captures=()=>fs.existsSync(artifacts)?fs.readdirSync(artifacts,{recursive:true}).filter(file=>String(file).endsWith('observation.json')).map(file=>path.join(artifacts,String(file))):[];
  globalThis.beforeFixtureCleanup=fixture=>{
    if(!fs.existsSync(fixture.cwd))throw Error('cleanup preceded capture');
    const files=captures();if(files.length!==1)throw Error('attempt capture missing');
    captured=JSON.parse(fs.readFileSync(files[0],'utf8'));
    if(captured.state!=='threw'||!captured.error.includes('incomplete'))throw Error('failure classification lost');
    if(!captured.publicTools.some(event=>event.toolUseId==='question-'+attempt))throw Error('public question lost');
    if(!fs.readFileSync(path.join(path.dirname(files[0]),'terminal.raw.log'),'utf8').includes('attempt '+attempt))throw Error('raw prefix lost');
    cleaned=true;
  };
  let done=false, error;
  const run=mode==='counting'?runPlanSkillCounting:runPlanSkillFloorCheck;
  const helper=run({skillName:'plan-design-review',slashCommand:'/plan-design-review',
    followUpPrompt:'# UI input',fixtureFiles:{'review-input.md':'# UI plan'},isLastStep0AUQ:()=>false,reviewCountCeiling:1,
    timeoutMs:mode==='counting'?600_000-(Date.now()-startedAt):600_000,env:{CLAUDE_CONFIG_DIR:config}})
    .catch(value=>error=value).finally(()=>done=true);
  for(let steps=0;!done&&steps<2000;steps++) {
    await realSleep(0);
    if(done)break;
    const next=[...timers].sort((a,b)=>a[1].at-b[1].at)[0];
    if(!next)throw Error('helper stalled without a settlement timer');
    timers.delete(next[0]);clock=Math.max(clock,next[1].at);next[1].callback();
  }
  await helper;
  if(!error?.message.includes('viewport is incomplete')||!cleaned||!exited||fs.existsSync(fixtureCwd))throw Error('actual cleanup or original failure lost');
  if(Date.now()-startedAt>(mode==='counting'?600000:660000)||timers.size!==timerCount)throw Error('budget reset or timer leak');
  if(captured.capture.cwd!==fixtureCwd||!captures().length)throw Error('durable attempt identity lost');
  attempts.push({attempt,elapsed:Date.now()-startedAt,signals,fixtureCwd,artifacts});
}
if(clock>1800000||attempts[0].fixtureCwd===attempts[1].fixtureCwd)throw Error('file wall or attempt isolation failed');
console.log(JSON.stringify({attempts,total:clock,fileWall:1800000}));
`);
    const result = spawnSync(process.execPath, [worker], { cwd: ROOT, encoding: 'utf8', timeout: 20_000,
      env: { ...process.env, BROWSE_TERMINAL_BINARY: process.execPath, EVALS_HERMETIC: '1', EVALS_RUN_ID: '',
        GSTACK_EVAL_DIR: '', TMPDIR: a.dir } });
    expect(result.status, result.stdout + result.stderr).toBe(0);
    const proof = JSON.parse(result.stdout.trim().split('\n').at(-1)!);
    expect(proof.attempts).toHaveLength(2);
    expect(proof.attempts.map((attempt: { elapsed: number }) => attempt.elapsed))
      .toEqual(mode === 'counting' ? [595000, 595000] : [609700, 609700]);
    expect(proof.total).toBe(mode === 'counting' ? 1190000 : 1219400);
    expect(proof.total).toBeLessThan(proof.fileWall);
  } finally { fs.rmSync(a.dir, { recursive: true, force: true }); }
}, 25_000);
