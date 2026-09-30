import { expect, test } from 'bun:test';
import { runPlanSkillCounting } from './helpers/claude-pty-runner';

test('opt-in requires a picker before creating any native fixture',async()=>{
 await expect(runPlanSkillCounting({requireNativePicker:true} as any)).rejects.toThrow('requires a declared picker');
});
