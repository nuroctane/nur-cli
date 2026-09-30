import {expect,test} from 'bun:test';
import batching from './fixtures/eng-batching-saved-ledger-b176.json';
import {createEngBatchingIssueCounter} from './helpers/eng-seeded-coverage';
import {engSetupAUQ} from './helpers/claude-pty-runner';

// Change placement only. Indented History states remain historical, and invalid
// duplicate current states are preserved rather than silently reconciled.
function resolutionBlock(plan:string):string {
  return plan.split(/\n(?=### R[1-9]\d*:)/).map(record=>{
    const states=record.match(/^State:.*\n/gm)??[];
    if(!states.length)return record;
    expect(record).toMatch(/^Actual answer:/m);
    return record.replace(/^State:.*\n/gm,'').replace(/^Actual answer:/m,states.join('')+'Actual answer:');
  }).join('\n');
}
test('saved native Header and Options remain scoped before the resolution fields',()=>{
  for(const index of [3,4,5,6,7,8,9]) {
    const f=batching.frames[index]!;
    const check=(plan:string)=>createEngBatchingIssueCounter(()=>plan,engSetupAUQ).isReviewAUQ(f.fingerprint,[]);
    expect(check(f.preAskPlan)).toBe(true);
    expect(check(resolutionBlock(f.preAskPlan))).toBe(true);
    for(const state of ['', 'State: pending\nState: pending\n', 'State: pending\nState: approved\n']) {
      const question=f.fingerprint.nativeCall.questions[0]!.question.split('\n')[0]!;
      const at=f.preAskPlan.indexOf(question),start=f.preAskPlan.lastIndexOf('\n### ',at),next=f.preAskPlan.indexOf('\n### ',at);
      expect(at).toBeGreaterThan(0);expect(start).toBeGreaterThanOrEqual(0);
      const end=next<0?f.preAskPlan.length:next,record=f.preAskPlan.slice(start,end);
      expect(record).toContain('State: pending\n');
      const bad=f.preAskPlan.slice(0,start)+record.replace(/^State: pending\n/m,state)+f.preAskPlan.slice(end);
      expect(check(bad)).toBe(false);expect(check(resolutionBlock(bad))).toBe(false);
    }
  }
});
