import {test,expect,afterEach} from 'bun:test';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {spawnSync} from 'node:child_process';
import fixture from './fixtures/autoplan-edit-digests-al.json';
import {createAutoplanArtifactRecorder,recordAutoplanArtifact,readPendingAutoplanArtifact,autoplanArtifactRecorderStatus} from './helpers/autoplan-artifact-recorder';
import {createAutoplanEditDigest,validAutoplanEditDigest} from './helpers/autoplan-artifact-digest';
import type {NativePublicToolEvent} from './helpers/plan-count-transcript';
const cleanups:Array<()=>void>=[];afterEach(()=>{for(const cleanup of cleanups.splice(0))cleanup()});
function replay(record=true) {
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'ap-digest-')),cwd=path.join(root,path.basename(fixture.cwd)),ownedStateRoot=path.join(root,'home','.gstack'),config=path.join(root,'config');
 const file=fixture.pending.file.replace(fixture.ownedStateRoot,ownedStateRoot),transcript=path.join(config,'projects','owned',fixture.pending.sessionId+'.jsonl');
 fs.mkdirSync(cwd,{recursive:true});fs.mkdirSync(path.dirname(file),{recursive:true});fs.mkdirSync(path.dirname(transcript),{recursive:true});fs.writeFileSync(file,fixture.before);fs.writeFileSync(transcript,'');
 const old=new Date(Date.parse(fixture.pending.timestamp)-1000);fs.utimesSync(file,old,old);
 const recorder=createAutoplanArtifactRecorder(cwd,config,ownedStateRoot);cleanups.push(()=>{recorder.dispose();fs.rmSync(root,{recursive:true,force:true})});
 const event={hook_event_name:'PreToolUse',tool_name:'Edit',session_id:fixture.pending.sessionId,tool_use_id:fixture.pending.toolUseId,cwd,transcript_path:transcript,tool_input:{file_path:file,...fixture.reconstructedRequest}};
 const history=structuredClone(fixture.events) as NativePublicToolEvent[];for(const e of history)if(e.input?.file_path===fixture.pending.file)e.input.file_path=file;
 const startedAt=Date.parse(history[0]!.timestamp)-1;
 if(record)recordAutoplanArtifact(JSON.stringify(event),recorder.file,cwd,config,ownedStateRoot);
 const pending=readPendingAutoplanArtifact(recorder.file,cwd,config,ownedStateRoot,startedAt,history);
 const context={cwd,ownedStateRoot,commandStartedAt:startedAt,transcriptStatus:'ready',publicTools:history,pending,viewportCapturedAt:Date.now(),now:Date.now()+1000};
 return {root,file,config,recorder,event,context,viewport:fixture.viewport};
}

test('hook persists bounded digests from its input, never request or result text',()=>{
 const r=replay(false),hook=r.recorder.hooks.PreToolUse[0]!.hooks[0]!;
 const child=spawnSync('bash',['-c',hook.command],{input:JSON.stringify({...r.event,tool_response:'PRIVATE_RESULT_SENTINEL'}),encoding:'utf8',timeout:6000});
 expect(child.status).toBe(0);expect(child.stdout).toBe('');expect(child.stderr).toBe('');
 const raw=fs.readFileSync(r.recorder.file,'utf8'),state=JSON.parse(raw);expect(validAutoplanEditDigest(state.pending.editDigest)).toBe(true);
 for(const secret of ['old_string','new_string','PRIVATE_RESULT_SENTINEL','Owner: the user.','Toast stacking ahead'])expect(raw).not.toContain(secret);
 expect(state.pending.editDigest).toEqual(createAutoplanEditDigest(r.file,r.event.tool_input.old_string,r.event.tool_input.new_string));
 expect(fs.statSync(r.recorder.file).size).toBeLessThan(1024*1024);expect(fs.statSync(r.recorder.file).mode&0o777).toBe(0o600);
 const legacy=structuredClone(state);delete legacy.pending.editDigest.clippedAdditions;
 expect(Buffer.byteLength(JSON.stringify(legacy))).toBeLessThan(64*1024);
});
test('unavailable or oversized before/request data yields no new digest authority',()=>{
 const r=replay();expect(createAutoplanEditDigest(r.file,'missing original','new')).toBeUndefined();expect(createAutoplanEditDigest(r.file,'Owner: the user.\n','x\n'.repeat(513))).toBeUndefined();
 const link=path.join(r.root,'linked');fs.symlinkSync(r.file,link);expect(createAutoplanEditDigest(link,r.event.tool_input.old_string,r.event.tool_input.new_string)).toBeUndefined();
 fs.writeFileSync(r.file,'x'.repeat(1024*1024+1));expect(createAutoplanEditDigest(r.file,'x','new')).toBeUndefined();fs.unlinkSync(r.file);expect(createAutoplanEditDigest(r.file,'old','new')).toBeUndefined();
});
test('identical pending hook replay cannot refresh digest or timestamp',()=>{
 const r=replay(),before=fs.readFileSync(r.recorder.file,'utf8');recordAutoplanArtifact(JSON.stringify(r.event),r.recorder.file,r.context.cwd,r.config,r.context.ownedStateRoot);expect(fs.readFileSync(r.recorder.file,'utf8')).toBe(before);
});
test.each(['changed-new','changed-old','whitespace-only','missing-input','over-limit','replace-all','before-file'])('same pending identity with %s invalidates prior digest authority',kind=>{
 const r=replay(),e=structuredClone(r.event) as any;
 if(kind==='changed-new')e.tool_input.new_string+='A different final action.\n';
 if(kind==='changed-old')e.tool_input.old_string='Owner: the user.';
 if(kind==='whitespace-only')e.tool_input.new_string=e.tool_input.new_string.replace('Owner: the user.','Owner:  the user.');
 if(kind==='missing-input')delete e.tool_input.new_string;
 if(kind==='over-limit')e.tool_input.new_string='new\n'.repeat(513);
 if(kind==='replace-all')e.tool_input.replace_all=true;
 if(kind==='before-file')fs.writeFileSync(r.file,fixture.before+'Unobserved change.');
 recordAutoplanArtifact(JSON.stringify(e),r.recorder.file,r.context.cwd,r.config,r.context.ownedStateRoot);
 expect(autoplanArtifactRecorderStatus(r.recorder.file,r.context.cwd,r.config,r.context.ownedStateRoot)).toEqual({status:'invalid',reason:'conflicting_replay'});
 expect(readPendingAutoplanArtifact(r.recorder.file,r.context.cwd,r.config,r.context.ownedStateRoot,r.context.commandStartedAt,r.context.publicTools,r.context.now)).toBeUndefined();
});

// Synthetic legacy crops use the actual generated PreToolUse subprocess. They
// preserve the frozen deletion/context policy, not new insertion-only authority.
