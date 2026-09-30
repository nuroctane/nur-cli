import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import type { createAutoplanArtifactRecorder } from './helpers/autoplan-artifact-recorder';

// Runs in the ordinary Windows subset as well as the full free suite. Keep
// transport coverage separate from the recorder suite's POSIX mode assertions.
  test.each(['C:\\owned', '\\\\server\\share', '\\\\\\\\server\\share'])('Windows hook transport preserves opaque native identity from %s', nativeRoot=>{
    const root=fs.mkdtempSync(path.join(os.tmpdir(),'artifact-argv-'));
    let recorder:ReturnType<typeof createAutoplanArtifactRecorder>|undefined;
    try{
      const helper=path.join(import.meta.dir,'helpers/autoplan-artifact-recorder.ts');
      const receiver=path.join(root,'receive-argv.ts');
      fs.writeFileSync(receiver,`
import {autoplanArtifactRecorderStatus} from ${JSON.stringify(pathToFileURL(helper).href)};
const args=process.argv.slice(2),[,file,cwd,config,stateRoot,...flags]=args;
const qa=flags[flags.indexOf('--eng-test-plan-root')+1];
console.log(JSON.stringify({args,status:autoplanArtifactRecorderStatus(file,cwd,config,stateRoot,qa),
  changedIdentity:autoplanArtifactRecorderStatus(file,cwd.replaceAll('\\\\','/'),config,stateRoot,qa)}));
`);
      // Execute the actual constructor under Windows path validation. Only the
      // two directory probes are synthetic; state and Bash-to-Bun argv are real.
      const source=fs.readFileSync(helper,'utf8');
      const quote=source.match(/^const quote = .*$/m)?.[0];expect(quote).toBeDefined();
      const start=source.indexOf('export function createAutoplanArtifactRecorder(');
      const end=source.indexOf('/** Reuses the pending UI path',start);
      expect(start).toBeGreaterThan(0);expect(end).toBeGreaterThan(start);
      const constructor=source.slice(start,end).replace('export function','function')
        .replaceAll('import.meta.path',JSON.stringify(receiver.replaceAll('/','\\')));
      const js=new Bun.Transpiler({loader:'ts'}).transformSync(`
function bind(fs,os,path,process){${quote}\n${constructor}\nreturn createAutoplanArtifactRecorder;}`);
      // Keep consecutive separators literal; path.join would erase the very
      // argv bytes being tested. Include shell syntax as data, never commands.
      const cwd=nativeRoot+"\\repo with ' quote $(echo NEVER_EXECUTE) "+String.fromCharCode(96)+'echo NEVER_EXECUTE'+String.fromCharCode(96);
      const config=nativeRoot+'\\interior\\\\config', stateRoot=nativeRoot+'\\state', qaRoot=nativeRoot+'\\qa\\\\';
      const create=new Function(js+';return bind;')()(
        {...fs,lstatSync:(file:string)=>[stateRoot,qaRoot].includes(file)?{isDirectory:()=>true}:fs.lstatSync(file)},
        os,{...path,isAbsolute:path.win32.isAbsolute},{platform:'win32',execPath:process.execPath.replaceAll('/','\\')});
      recorder=create(cwd,config,stateRoot,true,true,qaRoot);
      const hook=recorder!.hooks.PreToolUse[0]!.hooks[0]!;
      const child=spawnSync('bash',['-c',hook.command],{encoding:'utf8',timeout:6000});
      expect(child.error).toBeUndefined();expect(child.status,child.stderr).toBe(0);expect(child.stderr).toBe('');
      const received=JSON.parse(child.stdout);
      expect(received.args).toEqual(['--record',recorder!.file,cwd,config,stateRoot,'--approve-edits','--eng-test-plan-only','--eng-test-plan-root',qaRoot]);
      expect(received.status).toEqual({status:'idle'});
      expect(received.changedIdentity).toEqual({status:'invalid',reason:'record_error'});
    }finally{recorder?.dispose();fs.rmSync(root,{recursive:true,force:true});}
  });
