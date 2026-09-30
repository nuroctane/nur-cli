import { afterEach, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { pathToFileURL } from 'node:url';
const roots:string[]=[];
afterEach(()=>{for(const root of roots.splice(0))fs.rmSync(root,{recursive:true,force:true});});
test.skipIf(process.platform==='win32')('real launcher installs only opt-in owned hooks and removes records on close or early exit',async()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gstack-artifact-launch-'));roots.push(root);
  const fake=path.join(root,'fake-claude');fs.writeFileSync(fake,`#!${process.execPath}\n`+String.raw`
import * as fs from 'node:fs';
fs.writeFileSync(process.env.ARTIFACT_RECORD,JSON.stringify({pid:process.pid,args:process.argv.slice(2)}));
if(process.env.ARTIFACT_FAIL==='1')process.exit(19);
process.stdout.write('ARTIFACT_READY\n');process.stdin.resume();
`,{mode:0o755});
  const runner=pathToFileURL(path.join(import.meta.dir,'helpers/claude-pty-runner.ts')).href;
  for(const variant of ['enabled','approval','disabled','explicit-home','explicit-config','early-exit']){
    const cwd=path.join(root,variant);fs.mkdirSync(cwd);const result=path.join(cwd,'result.json');
    const extra=variant==='explicit-home'?{HOME:cwd}:variant==='explicit-config'?{CLAUDE_CONFIG_DIR:cwd}:{};
    const worker=path.join(cwd,'worker.ts');fs.writeFileSync(worker,`
import * as fs from 'node:fs';
import {launchClaudePty,resolveClaudeBinary} from ${JSON.stringify(runner)};
if(resolveClaudeBinary()!==${JSON.stringify(fake)})throw Error('fake binding');
const session=await launchClaudePty({cwd:${JSON.stringify(cwd)},seedSkills:true,observeAutoplanArtifacts:${variant!=='disabled'},approveAutoplanArtifactEdits:${variant==='approval'||variant.startsWith('explicit-')},timeoutMs:8000,
 env:${JSON.stringify({...extra,ARTIFACT_RECORD:result,ARTIFACT_FAIL:variant==='early-exit'?'1':'0'})}});
try {try{await session.waitFor('ARTIFACT_READY',{timeoutMs:2000,pollMs:20});}catch(e){if(${variant!=='early-exit'})throw e;}
const r=JSON.parse(fs.readFileSync(${JSON.stringify(result)},'utf8'));
r.file=session.pendingAutoplanArtifactFile??null;r.stateRoot=session.hermeticSkillStateRoot??null;
r.canStart=typeof session.startAutoplanArtifactEditApproval==='function';
if(${variant==='approval'}){const start=Date.now();session.startAutoplanArtifactEditApproval(start);r.started=JSON.parse(fs.readFileSync(r.file,'utf8')).approvalStartedAt===start;}
r.exists=r.file?fs.existsSync(r.file):false;fs.writeFileSync(${JSON.stringify(result)},JSON.stringify(r));
} finally {await session.close();}
`);
    const child=Bun.spawn([process.execPath,worker],{env:{...process.env,BROWSE_TERMINAL_BINARY:fake,EVALS_HERMETIC:'1'},stdout:'pipe',stderr:'pipe'});
    const timer=setTimeout(()=>child.kill('SIGKILL'),15000);
    try{const [code,out,err]=await Promise.all([child.exited,new Response(child.stdout).text(),new Response(child.stderr).text()]);expect(code,out+err).toBe(0);}finally{clearTimeout(timer);}
    const resultData=JSON.parse(fs.readFileSync(result,'utf8')),enabled=['enabled','approval','early-exit'].includes(variant);
    expect(resultData.canStart).toBe(variant==='approval');
    if(variant==='approval')expect(resultData.started).toBe(true);
    expect(Boolean(resultData.file)).toBe(enabled);expect(resultData.exists).toBe(enabled);
    if(enabled){const settings=JSON.parse(resultData.args[resultData.args.indexOf('--settings')+1]);
      expect(Object.keys(settings.hooks).sort()).toEqual(['PostToolUse','PostToolUseFailure','PreToolUse']);
      for(const entries of Object.values(settings.hooks) as any[]){expect(entries).toHaveLength(1);expect(entries[0].matcher).toBe('^(Write|Edit)$');expect(entries[0].hooks[0].timeout).toBe(5);expect(entries[0].hooks[0].command).toContain(resultData.stateRoot);expect(entries[0].hooks[0].command.includes('--approve-edits')).toBe(variant==='approval');}
      expect(fs.existsSync(resultData.file)).toBe(false);
    }else expect(resultData.args).not.toContain('--settings');
    expect(()=>process.kill(resultData.pid,0)).toThrow();
  }
},90000);
