import {expect,test} from 'bun:test';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import fixture from './fixtures/ceo-count-ad-v2.json';
import {readPlanCountTranscript} from './helpers/plan-count-transcript';
test('exact public native requests and successful replies reconstruct the captured calls once',()=>{
 for(const which of ['distinct','paired','pairedRetry'] as const){const c=fixture.cases[which],dir=fs.mkdtempSync(path.join(os.tmpdir(),'ceo-count-public-'));try{const project=path.join(dir,'projects','owned');fs.mkdirSync(project,{recursive:true});const records=c.nativeRecords.map(r=>JSON.stringify(r)).join('\n')+'\n';fs.writeFileSync(path.join(project,c.calls[0]!.sessionId+'.jsonl'),records+records);expect(readPlanCountTranscript(dir,c.observation.capture.cwd).calls).toEqual(c.calls);for(const a of c.timeAnchors){expect(Date.parse(a.requestAt)).toBeLessThanOrEqual(Date.parse(a.replyAt));expect(Date.parse(a.replyAt)).toBeLessThanOrEqual(Date.parse(c.observation.capture.at));}}finally{fs.rmSync(dir,{recursive:true,force:true});}}
});
