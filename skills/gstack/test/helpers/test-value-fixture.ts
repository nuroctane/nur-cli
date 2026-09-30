/**
 * Fixture for the test value bar evals (test/skill-e2e-test-value.test.ts).
 *
 * A tiny Bun project. The feature branch adds three tests:
 *   - test/pricing-source.test.ts greps src/pricing.ts for a function name
 *     (low value: exact source grep, no declared contract);
 *   - test/pricing-reset.test.ts exists only to call _resetPriceCacheForTests,
 *     a test-only export with no production caller (low value);
 *   - test/skill-golden.test.ts compares generated SKILL.md bytes with a golden
 *     file (a generated-output contract the retention bar keeps).
 */
import * as fs from 'node:fs';
import * as path from 'node:path';
import { spawnSync } from 'node:child_process';

export const LOW_VALUE_TESTS = {
  sourceGrep: 'test/pricing-source.test.ts',
  testOnlyExport: 'test/pricing-reset.test.ts',
  golden: 'test/skill-golden.test.ts',
  testOnlySymbol: '_resetPriceCacheForTests',
} as const;

function write(dir: string, relative: string, content: string): void {
  fs.mkdirSync(path.dirname(path.join(dir, relative)), { recursive: true });
  fs.writeFileSync(path.join(dir, relative), content);
}

function git(dir: string, args: string[]): void {
  const result = spawnSync('git', args, { cwd: dir, stdio: 'pipe', timeout: 10_000 });
  if (result.status !== 0) throw new Error(`git ${args.join(' ')} failed: ${result.stderr}`);
}

export function createTestValueFixture(dir: string): void {
  write(dir, 'package.json', JSON.stringify({ name: 'pricing-app', version: '1.0.0', type: 'module', scripts: { test: 'bun test' } }, null, 2) + '\n');
  write(dir, 'src/pricing.ts', `const cache = new Map<string, number>();

export function applyDiscount(price: number, percent: number): number {
  if (percent < 0 || percent > 100) throw new Error('Invalid discount');
  const key = \`\${price}:\${percent}\`;
  if (!cache.has(key)) cache.set(key, Math.round(price * (100 - percent)) / 100);
  return cache.get(key)!;
}
`);
  write(dir, 'src/checkout.ts', `import { applyDiscount } from './pricing';

export function total(prices: number[], percent: number): number {
  return prices.reduce((sum, price) => sum + applyDiscount(price, percent), 0);
}
`);
  write(dir, 'scripts/gen-skill.ts', `export function renderSkill(name: string): string {
  return \`# \${name}\\n\\nRun \\\`/\${name}\\\` to start.\\n\`;
}
`);
  write(dir, 'test/fixtures/golden/SKILL.md', '# pricing\n\nRun `/pricing` to start.\n');
  write(dir, 'test/pricing.test.ts', `import { test, expect } from 'bun:test';
import { applyDiscount } from '../src/pricing';
import { total } from '../src/checkout';

test('applies a discount and rejects an invalid percent', () => {
  expect(applyDiscount(100, 25)).toBe(75);
  expect(() => applyDiscount(100, 101)).toThrow('Invalid discount');
  expect(total([100, 50], 10)).toBe(135);
});
`);
  write(dir, 'CLAUDE.md', '# pricing-app\n\n## Testing\n\nRun `bun test`.\n');
  git(dir, ['init', '-q', '-b', 'main']);
  git(dir, ['config', 'user.email', 'test@test.com']);
  git(dir, ['config', 'user.name', 'Test']);
  git(dir, ['add', '.']);
  git(dir, ['commit', '-q', '-m', 'initial pricing app']);
  git(dir, ['checkout', '-q', '-b', 'feature/pricing-cache']);

  fs.appendFileSync(path.join(dir, 'src/pricing.ts'), `
export function ${LOW_VALUE_TESTS.testOnlySymbol}(): void {
  cache.clear();
}
`);
  write(dir, LOW_VALUE_TESTS.sourceGrep, `import { test, expect } from 'bun:test';
import * as fs from 'node:fs';

test('pricing module defines applyDiscount', () => {
  const source = fs.readFileSync(new URL('../src/pricing.ts', import.meta.url), 'utf8');
  expect(source).toContain('export function applyDiscount');
});
`);
  write(dir, LOW_VALUE_TESTS.testOnlyExport, `import { test, expect } from 'bun:test';
import { ${LOW_VALUE_TESTS.testOnlySymbol} } from '../src/pricing';

test('cache reset helper is callable', () => {
  expect(() => ${LOW_VALUE_TESTS.testOnlySymbol}()).not.toThrow();
});
`);
  write(dir, LOW_VALUE_TESTS.golden, `import { test, expect } from 'bun:test';
import * as fs from 'node:fs';
import { renderSkill } from '../scripts/gen-skill';

test('generated SKILL.md matches the golden bytes', () => {
  const golden = fs.readFileSync(new URL('./fixtures/golden/SKILL.md', import.meta.url), 'utf8');
  expect(renderSkill('pricing')).toBe(golden);
});
`);
  git(dir, ['add', '.']);
  git(dir, ['commit', '-q', '-m', 'add pricing cache and tests']);
}

export function lastJsonLine(output: string): any {
  const lines = output.trim().split('\n').map(line => line.trim()).filter(Boolean);
  for (let index = lines.length - 1; index >= 0; index--) {
    const line = lines[index]!.replace(/^`+|`+$/g, '');
    if (!line.startsWith('{')) continue;
    try { return JSON.parse(line); } catch {}
  }
  return undefined;
}

export function jsonFindings(output: string): any[] {
  return output.split('\n').map(line => line.trim()).filter(line => line.startsWith('{') && line.includes('"severity"')).flatMap(line => {
    try { return [JSON.parse(line)]; } catch { return []; }
  });
}
