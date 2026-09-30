import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawnSync } from 'node:child_process';
import { startTestServer } from '../browse/test/test-server';

const ROOT = path.resolve(import.meta.dir, '..');
const PROBE = path.join(ROOT, 'test/fixtures/qa-only-browser-probe.ts');

test('the report-only fixture serves exactly one bounded, observable defect', async () => {
  const { server, url } = startTestServer();
  try {
    const response = await fetch(url + '/qa-only.html');
    const html = await response.text();
    expect(response.status).toBe(200);
    expect(html).toBe(fs.readFileSync(path.join(ROOT, 'browse/test/fixtures/qa-only.html'), 'utf8'));
    expect(html.match(/console\.error\(/g)).toHaveLength(1);
    expect(html).not.toMatch(/<(?:a|form|img)\b/);
    expect(html).toContain('Cannot read properties of undefined');
  } finally {
    server.stop(true);
  }
});

test.each(['success', 'failure', 'existing-screenshot'])('the browser probe retains structured command evidence: %s', scenario => {
  const directory = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'qa-probe-'));
  const browse = path.join(directory, process.platform === 'win32' ? 'browse.exe' : 'browse');
  const source = path.join(directory, 'browse.ts');
  const calls = path.join(directory, 'calls.jsonl');
  const screenshot = path.join(directory, 'initial.png');
  if (scenario === 'existing-screenshot') fs.writeFileSync(screenshot, 'previous screenshot');
  fs.writeFileSync(source, `
import {appendFileSync, writeFileSync} from 'node:fs';
const args=process.argv.slice(2);
appendFileSync(${JSON.stringify(calls)},JSON.stringify(args)+'\\n');
if(args[0]==='console' && ${JSON.stringify(scenario)}==='failure') {
  process.stderr.write('console failed\\n');process.exit(7);
}
if(args[0]==='screenshot')writeFileSync(args[1],'fixture image');
process.stdout.write(args[0]+' result\\n\\n');
process.stderr.write(args[0]+' diagnostic\\n');
`, { mode: 0o700 });
  try {
    const compiled = spawnSync(process.execPath, ['build', '--compile', source, '--outfile', browse], {
      encoding: 'utf8', timeout: 5000,
    });
    expect(compiled.error).toBeUndefined();
    expect(compiled.status, compiled.stderr).toBe(0);
    const result = spawnSync(process.execPath, [PROBE, browse, 'http://fixture.invalid/', screenshot], {
      encoding: 'utf8', timeout: 5000,
    });
    expect(result.error).toBeUndefined();
    if (scenario === 'existing-screenshot') {
      expect(result.status).not.toBe(0);
      expect(result.stdout).toBe('');
      expect(result.stderr).toContain('Refusing to overwrite a previous screenshot');
      expect(fs.existsSync(calls)).toBe(false);
      expect(fs.readFileSync(screenshot, 'utf8')).toBe('previous screenshot');
      return;
    }
    const commands = fs.readFileSync(calls, 'utf8').trim().split('\n').map(line => JSON.parse(line));
    if (scenario === 'failure') {
      expect(result.status).not.toBe(0);
      expect(result.stdout).toBe('');
      expect(result.stderr).toContain('console failed: 7: console failed');
      expect(commands).toEqual([['goto', 'http://fixture.invalid/'], ['console', '--errors']]);
      expect(fs.existsSync(screenshot)).toBe(false);
      return;
    }
    expect(result.status).toBe(0);
    expect(result.stderr).toBe('');
    expect(commands).toEqual([['goto', 'http://fixture.invalid/'], ['console', '--errors'], ['screenshot', screenshot]]);
    const observed = JSON.parse(result.stdout);
    expect(Object.keys(observed)).toEqual(['navigation', 'console', 'screenshot']);
    for (const [name, command] of [['navigation', 'goto'], ['console', 'console'], ['screenshot', 'screenshot']]) {
      expect(observed[name]).toEqual({ stdout: command + ' result\n\n', stderr: command + ' diagnostic\n', exitCode: 0 });
    }
    expect(fs.readFileSync(screenshot, 'utf8')).toBe('fixture image');
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
