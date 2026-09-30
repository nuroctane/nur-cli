import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';

const [browse, url, screenshot] = process.argv.slice(2);
if (!browse || !url || !screenshot) throw new Error('Expected browse binary, target URL and screenshot path');
if (existsSync(screenshot)) throw new Error('Refusing to overwrite a previous screenshot');

const results: Record<string, { stdout: string; stderr: string; exitCode: number }> = {};
for (const [name, args] of [
  ['navigation', ['goto', url]],
  ['console', ['console', '--errors']],
  ['screenshot', ['screenshot', screenshot]],
] as const) {
  const result = spawnSync(browse, [...args], { encoding: 'utf8', timeout: 10_000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${name} failed: ${result.status}: ${result.stderr}`);
  results[name] = { stdout: result.stdout, stderr: result.stderr, exitCode: result.status };
}
process.stdout.write(JSON.stringify(results) + '\n');
