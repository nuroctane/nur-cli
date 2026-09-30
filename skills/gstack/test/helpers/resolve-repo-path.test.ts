import { describe, expect, test } from 'bun:test';
import * as path from 'path';
import { directSpecifiers, resolveRepoLiteral, resolveRepoSpecifier } from './resolve-repo-path';

const ROOT = path.resolve(import.meta.dir, '../..');

describe('resolve-repo-path', () => {
  test('relative specifiers resolve to repo files, with and without extensions', () => {
    expect(resolveRepoSpecifier(ROOT, 'test/touchfiles.test.ts', './helpers/touchfiles')).toBe('test/helpers/touchfiles.ts');
    expect(resolveRepoSpecifier(ROOT, 'test/touchfiles.test.ts', '../lib/eval-model')).toBe('lib/eval-model.ts');
    expect(resolveRepoSpecifier(ROOT, 'lib/code-intelligence/gbrain-adapter.ts', '../egress-receipt.js')).toBe('lib/egress-receipt.ts');
    expect(resolveRepoSpecifier(ROOT, 'test/touchfiles.test.ts', './helpers/no-such-module')).toBeNull();
  });

  test('bare packages and bun:/node: builtins never resolve', () => {
    for (const specifier of ['bun:test', 'node:fs', 'fs', '@anthropic-ai/sdk', 'ts-morph']) {
      expect(resolveRepoSpecifier(ROOT, 'test/touchfiles.test.ts', specifier)).toBeNull();
    }
  });

  test('direct specifiers include imports, re-exports, require and literal dynamic import, not type-only imports', () => {
    const source = [
      "import { a } from './a';",
      "import type { T } from './types';",
      "export type { U } from './more-types';",
      "export { b } from '../lib/b';",
      "import {\n  c,\n  d,\n} from './multi';",
      "import './side-effect';",
      "const e = require('./e');",
      "const f = await import('./f');",
      "const g = await import(name);",
    ].join('\n');
    expect(directSpecifiers(source)).toEqual(['./a', '../lib/b', './multi', './side-effect', './e', './f']);
  });

  test('path literals resolve only when the repo path exists', () => {
    expect(resolveRepoLiteral(ROOT, 'bin/gstack-config')).toBe('bin/gstack-config');
    expect(resolveRepoLiteral(ROOT, 'test/fixtures/')).toBe('test/fixtures');
    expect(resolveRepoLiteral(ROOT, path.join('test', 'helpers', 'touchfiles.ts'))).toBe('test/helpers/touchfiles.ts');
    expect(resolveRepoLiteral(ROOT, 'test/fixtures/no-such-fixture.json')).toBeNull();
    expect(resolveRepoLiteral(ROOT, '/etc/passwd')).toBeNull();
    expect(resolveRepoLiteral(ROOT, '../outside')).toBeNull();
  });
});
