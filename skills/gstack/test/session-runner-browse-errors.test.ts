import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawnSync } from 'node:child_process';
import { computePaidCaseSelection } from '../scripts/test-paid-shards';

let fixture: string;
let observations: any;
beforeAll(() => {
  fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'browse-signals-'));
  for (const name of ['home', 'tmp', 'state', 'codex']) fs.mkdirSync(path.join(fixture, name));
  const childScript = path.join(fixture, 'signals.test.ts');
  fs.writeFileSync(childScript, SCRIPT);
  const child = spawnSync(process.execPath, ['test', childScript, '--retry', '1'], {
    cwd: fixture, encoding: 'utf8', timeout: 15_000,
    env: {
      PATH: process.env.PATH ?? '', LANG: 'C.UTF-8', NO_COLOR: '1',
      HOME: path.join(fixture, 'home'), TMPDIR: path.join(fixture, 'tmp'),
      GSTACK_HOME: path.join(fixture, 'state'), CODEX_HOME: path.join(fixture, 'codex'),
      EVALS_HERMETIC: '0', SIGNAL_FIXTURE_ROOT: fixture,
      SIGNAL_REPO_ROOT: path.resolve(import.meta.dir, '..'),
    },
  });
  expect(child.status, child.stderr || child.stdout).toBe(0);
  observations = JSON.parse(fs.readFileSync(path.join(fixture, 'observations.json'), 'utf8'));
}, 20_000);
afterAll(() => { if (fixture) fs.rmSync(fixture, { recursive: true, force: true }); });

test.each(['full', 'pr'] as const)('%s selection binds the captured failure to SQL review', profile => {
  expect(computePaidCaseSelection({ profile, env: {}, changedFiles: ['test/fixtures/review-browse-error-ci-36516246523.json'] }).selection)
    .toEqual({ e2e: ['review-sql-injection'], judges: [] });
});

describe('native browser-error evidence', () => {
  test('all JavaScript line terminators bound missing-file diagnostics in execution output and stderr', () => {
    const rows = observations.sessions.filter(row => row.id.startsWith('line-boundary-'));
    expect(rows).toHaveLength(8);
    for (const row of rows) expect(row.result.browseErrors, row.id).toEqual([]);
  });
  test.each(['captured-ci', 'separate-lines', 'browser-document', 'browse-document', 'read-document', 'assistant-text', 'transport-metadata'])('%s is not a browser failure', id => {
    const row = observations.sessions.find(row => row.id === id);
    expect(row.spawns).toBe(1);
    expect(row.result.exitReason).toBe('success');
    expect(row.result.browseErrors).toEqual([]);
  });

  test.each(['unknown-command', 'snapshot-flag', 'missing-binary', 'start-failure', 'missing-file', 'missing-windows-file'])('%s remains a browser failure in execution output and stderr', id => {
    for (const suffix of ['', '-stderr']) {
      const row = observations.sessions.find(row => row.id === id + suffix);
      expect(row.spawns).toBe(1);
      expect(row.result.exitReason).toBe('success');
      expect(row.result.browseErrors).toHaveLength(1);
      expect(row.result.browseErrors[0]).toContain(row.signal);
    }
  });

  test('captured public event reproduces the original serialized-envelope false positive', () => {
    expect(observations.legacyBrowseError).toBe(observations.capturedBrowseError);
    expect(observations.legacyBrowseError).toContain('No such file or directory');
    expect(observations.legacyBrowseError).toContain('tool_use_id');
  });
});

describe('registered SQL review callback verdict', () => {
  test('a genuine browser failure triggers Bun’s configured retry and preserves both collector attempts', () => {
    expect(observations.retry.map(row => row.passed)).toEqual([false, true]);
    expect(observations.retry.map(row => row.browse_errors)).toEqual([['Server failed to start'], []]);
  });
  test.each(['captured-ci', 'browser-error', 'session-error', 'missing-report', 'missing-finding'])('%s agrees with its collector and rejects failures for the native retry', id => {
    const row = observations.sql.find(row => row.id === id);
    const passed = id === 'captured-ci';
    expect(row.registeredName).toBe('review-sql-injection');
    expect(row.recordings).toHaveLength(1);
    expect(row.recordings[0].passed).toBe(passed);
    expect(row.rejected).toBe(!passed);
    expect(row.recordings[0].browse_errors).toEqual(row.browseErrors);
  });
});

const SCRIPT = String.raw`import { expect, mock, test } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
const dir = process.env.SIGNAL_FIXTURE_ROOT!;
const root = process.env.SIGNAL_REPO_ROOT!;
const capture = JSON.parse(fs.readFileSync(path.join(root, 'test/fixtures/review-browse-error-ci-36516246523.json'), 'utf8'));
const diagnostics = [
 ['unknown-command', 'Unknown command: unavailable'],
 ['snapshot-flag', 'Unknown snapshot flag: --unavailable'],
 ['missing-binary', 'ERROR: browse binary not found'],
 ['start-failure', 'Server failed to start'],
 ['missing-file', 'No such file or directory: /fixture/browse'],
 ['missing-windows-file', 'No such file or directory: C:\\fixture\\browse.exe'],
];
function events(output: string, tool = 'Bash') {
 return [
  {type:'assistant',message:{content:[{type:'tool_use',id:'fixture-tool',name:tool,input:tool==='Bash'?{command:'browse unavailable'}:{file_path:'/fixture/README.md'}}]}},
  {type:'user',message:{content:[{type:'tool_result',tool_use_id:'fixture-tool',content:output}]}},
 ];
}
const cases: any[] = [
 {id:'captured-ci',events:capture.events},
 {id:'separate-lines',events:events('No such file or directory: Gemfile\nbrowse')},
 {id:'browser-document',events:events('No such file or directory: BROWSER.md')},
 {id:'browse-document',events:events('No such file or directory: browse.md')},
 {id:'read-document',events:events(diagnostics.map(x=>x[1]).join('\n'),'Read')},
 {id:'assistant-text',events:[{type:'assistant',message:{content:[{type:'text',text:diagnostics.map(x=>x[1]).join('\n')}]}}]},
 {id:'transport-metadata',events:[...events('no browser error'),{type:'system',message:diagnostics.map(x=>x[1]).join('\n')}]},
 ...[['lf','\n'],['cr','\r'],['ls','\u2028'],['ps','\u2029']].flatMap(([id,separator])=>[
  {id:'line-boundary-'+id,events:events('No such file or directory: Gemfile'+separator+'browse')},
  {id:'line-boundary-'+id+'-stderr',events:events('ok'),stderr:'No such file or directory: Gemfile'+separator+'browse'},
 ]),
 ...diagnostics.flatMap(([id,signal])=>[{id,signal,events:events(signal)},{id:id+'-stderr',signal,events:events('ok'),stderr:signal}]),
];
let active: any;
mock.module('child_process',()=>({
 spawn(command: string,args: string[],options: any) {
  if(command!=='claude')throw Error('Unexpected executable');
  const x=active;x.spawns++;
  queueMicrotask(()=>{
   for(const event of x.scenario.events)x.child.stdout.write(JSON.stringify(event)+'\n');
   x.child.stdout.end(JSON.stringify({type:'result',subtype:'success',is_error:false,result:'fixture result',num_turns:1,total_cost_usd:0})+'\n');
   x.child.stderr.end(x.scenario.stderr??'');
   x.child.exitCode=0;x.child.emit('exit',0,null);
  });
  return x.child;
 },
 spawnSync(){return {status:1,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};},
 execFileSync(){throw Error('Unexpected synchronous executable');},
}));
mock.module(path.join(root,'scripts/test-strict-output.ts'),()=>({killProcessGroup(child: any){if(child!==active.child)throw Error('Unknown child');}}));
const {runSkillTest}=await import(path.join(root,'test/helpers/session-runner.ts'));
const {recordE2E}=await import(path.join(root,'test/helpers/e2e-helpers.ts'));
test('exercise the actual runner and registered SQL callback without a provider',async()=>{
 const sessions: any[]=[];
 for(const scenario of cases) {
  const child=Object.assign(new EventEmitter(),{pid:undefined,stdin:new PassThrough(),stdout:new PassThrough(),stderr:new PassThrough(),exitCode:null});
  const x=active={scenario,child,spawns:0};
  const cwd=path.join(dir,scenario.id);fs.mkdirSync(cwd);
  const result=await runSkillTest({prompt:'local native-event replay',workingDirectory:cwd,timeout:1000,testName:scenario.id,model:'fixture-no-provider'});
  sessions.push({id:scenario.id,signal:scenario.signal,spawns:x.spawns,result});
 }
 const source=fs.readFileSync(path.join(root,'test/skill-e2e-review.test.ts'),'utf8');
 const start=source.indexOf("testConcurrentIfSelected('review-sql-injection',");
 const ending='}, CAPTURE_MS + REVIEW_FINALIZE_MS);';
 const end=source.indexOf(ending,start);
 expect(start).toBeGreaterThanOrEqual(0);expect(end).toBeGreaterThan(start);
 const registration=source.slice(start,end+ending.length);
 const sql: any[]=[];
 for(const id of ['captured-ci','browser-error','session-error','missing-report','missing-finding']) {
  const reviewDir=path.join(dir,'sql-'+id);fs.mkdirSync(reviewDir);
  if(id!=='missing-report')fs.writeFileSync(path.join(reviewDir,'review-output.md'),id==='missing-finding'?'Nothing found':'SQL injection in user input');
  const result=structuredClone(sessions.find(row=>row.id==='captured-ci').result);
  if(id!=='captured-ci')result.browseErrors=[];
  if(id==='browser-error')result.browseErrors=['Server failed to start'];
  if(id==='session-error')result.exitReason='error_api';
  const recordings: any[]=[];let callback: any;let registeredName: string='';
  const register=(name: string,fn: any)=>{registeredName=name;callback=fn;};
  const transpiled=new Bun.Transpiler({loader:'ts'}).transformSync(registration);
  new Function('testConcurrentIfSelected','runSkillTest','reviewDir','CAPTURE_MS','REVIEW_FINALIZE_MS','runId','logCost','recordE2E','evalCollector','expect','fs','path',transpiled)(register,async()=>result,reviewDir,1234,5000,'fixture-run',()=>{},recordE2E,{addTest(row: any){recordings.push(row);}},expect,fs,path);
  expect(typeof callback).toBe('function');
  let rejected=false;try{await callback();}catch{rejected=true;}
  sql.push({id,registeredName,recordings,rejected,browseErrors:result.browseErrors});
 }
 const legacyBrowseError=capture.events.map(event=>JSON.stringify(event)).join('\n').match(/no such file or directory.*browse/i)?.[0].slice(0,200);
 fs.writeFileSync(path.join(dir,'observations.json'),JSON.stringify({sessions,sql,legacyBrowseError,capturedBrowseError:capture.source.browseErrors[0]},null,2)+'\n',{mode:0o600});
},10000);
const retryDir=path.join(dir,'native-sql-retry');fs.mkdirSync(retryDir);
fs.writeFileSync(path.join(retryDir,'review-output.md'),'SQL injection in user input');
const retrySource=fs.readFileSync(path.join(root,'test/skill-e2e-review.test.ts'),'utf8');
const retryStart=retrySource.indexOf("testConcurrentIfSelected('review-sql-injection',");
const retryEnding='}, CAPTURE_MS + REVIEW_FINALIZE_MS);';
const retryEnd=retrySource.indexOf(retryEnding,retryStart);
expect(retryStart).toBeGreaterThanOrEqual(0);expect(retryEnd).toBeGreaterThan(retryStart);
let retryAttempts=0;
const retryRecordings: any[]=[];
const registerRetry=(name: string,fn: any)=>test(name,async()=>{
 try{await fn();}finally{
  const file=path.join(dir,'observations.json');
  const data=JSON.parse(fs.readFileSync(file,'utf8'));
  data.retry=retryRecordings;
  fs.writeFileSync(file,JSON.stringify(data,null,2)+'\n',{mode:0o600});
 }
});
const retryResult=async()=>{
 retryAttempts++;
 const data=JSON.parse(fs.readFileSync(path.join(dir,'observations.json'),'utf8'));
 const result=data.sessions.find(row=>row.id==='captured-ci').result;
 result.browseErrors=retryAttempts===1?['Server failed to start']:[];
 return result;
};
const retryJs=new Bun.Transpiler({loader:'ts'}).transformSync(retrySource.slice(retryStart,retryEnd+retryEnding.length));
new Function('testConcurrentIfSelected','runSkillTest','reviewDir','CAPTURE_MS','REVIEW_FINALIZE_MS','runId','logCost','recordE2E','evalCollector','expect','fs','path',retryJs)(registerRetry,retryResult,retryDir,1234,5000,'fixture-retry',()=>{},recordE2E,{addTest(row: any){retryRecordings.push(row);}},expect,fs,path);
`;
