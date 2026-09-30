import { describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawnSync } from 'node:child_process';

describe.skipIf(process.platform !== 'linux')('bootstrap native session lifecycle hooks', () => {
  test('actual runner arms cleanup before registration and bounds rejected or stalled hooks', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'bs-life-'));
    try {
      const script = path.join(root, 'observe.test.ts');
      fs.writeFileSync(script, SCRIPT);
      const result = spawnSync(process.execPath, ['test', script, '--timeout', '15000'], {
        cwd: root, encoding: 'utf8', timeout: 20000,
        env: {
          PATH: process.env.PATH, HOME: root, GSTACK_HOME: path.join(root, 'state'), EVALS_HERMETIC: '0',
          BOOTSTRAP_SESSION_SOURCE: path.join(import.meta.dir, 'helpers/session-runner.ts'),
        },
      });
      expect(result.status, result.stderr + result.stdout).toBe(0);
      const observations = JSON.parse(fs.readFileSync(path.join(root, 'observations.json'), 'utf8'));
      expect(observations.length).toBe(5);
      for (const row of observations) {
        expect(row.handlersArmed).toBe(true);
        expect(row.exited).toBe(true);
        expect(row.settledCalls).toBe(1);
        expect(row.elapsed).toBeLessThan(6500);
      }
      expect(observations[0].reason).toBe('success');
      expect(observations[1].error).toContain('registration fault');
      expect(observations[1].receivedPrompt).toBe(false);
      expect(observations[2].error).toContain('settlement fault');
      expect(observations[3].error).toContain('deadline exceeded');
      expect(observations[4].error).toContain('native lifecycle failed');
      expect(observations[4].causes).toEqual(['registration fault', 'settlement fault']);
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  }, 25000);
});

const SCRIPT = String.raw`
import { mock, test, expect } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as cp from 'node:child_process';
const spawn = cp.spawn;
const spawnSync = cp.spawnSync;
let active: any;
mock.module('child_process', () => ({
  spawnSync,
  spawn(command: string, args: string[], options: any) {
    if (command !== 'claude') throw new Error('unexpected executable');
    const child = spawn(process.execPath, ['-e',
      "await Bun.stdin.text(); require('fs').writeFileSync('received-prompt','yes'); console.log(JSON.stringify({type:'result',subtype:'success',is_error:false,result:'synthetic bootstrap',num_turns:1}));"], options);
    active.child = child;
    return child;
  },
}));
const { runSkillTest } = await import(process.env.BOOTSTRAP_SESSION_SOURCE!);
test('exercise native lifecycle', async () => {
  const observations: any[] = [];
  for (const scenario of ['success','start-fail','settle-fail','settle-stall','both-fail']) {
    const cwd = path.join(process.cwd(), scenario); fs.mkdirSync(cwd);
    active = {settledCalls:0};
    const began = Date.now();
    let result: any, error: any;
    try {
      result = await runSkillTest({prompt:'no model',workingDirectory:cwd,timeout:1000,startupGraceMs:1000,model:'fixture-no-provider',
        nativeLifecycle:{
          onSpawn(pid: number) {
            active.handlersArmed = active.child.listenerCount('exit') > 0 && active.child.listenerCount('error') > 0;
            expect(pid).toBe(active.child.pid);
            if (scenario === 'start-fail' || scenario === 'both-fail') throw new Error('registration fault');
          },
          async onSettled(input: any) {
            active.settledCalls++;
            active.exited = input.exited && (active.child.exitCode !== null || active.child.signalCode !== null);
            expect(input.deadline - Date.now()).toBeLessThanOrEqual(5000);
            if (scenario === 'settle-fail' || scenario === 'both-fail') throw new Error('settlement fault');
            if (scenario === 'settle-stall') await new Promise(()=>{});
          },
        },
      });
    } catch (e) { error=e; }
    observations.push({scenario,handlersArmed:active.handlersArmed,settledCalls:active.settledCalls,exited:active.exited,reason:result?.exitReason,error:error?.message,causes:error?.errors?.map((e:any)=>e.message),elapsed:Date.now()-began,receivedPrompt:fs.existsSync(path.join(cwd,'received-prompt'))});
  }
  fs.writeFileSync('observations.json',JSON.stringify(observations));
},15000);
`;
