/**
 * Derived touchfile rule: a paid test depends on every `test/helpers` and
 * `test/fixtures` file it reaches through static imports, plus every such path
 * named in a string literal inside that closure (fixtures read through `fs`).
 * The result is a lower bound: a computed fixture path is declared by adding it
 * to the key by hand.
 */
import * as fs from 'fs';
import * as path from 'path';
import { matchGlob } from './test-selection';
import { resolveRepoLiteral, resolveRepoSpecifier } from './resolve-repo-path';

export interface ClosureEntry {
  /** Repo-relative dependency path. */
  file: string;
  /** How the paid test reached it: the import chain, ending in `"literal"` for string references. */
  chain: string[];
}

const LITERAL = /test\/(?:helpers|fixtures)\/[A-Za-z0-9_.\-/]+[A-Za-z0-9_]/g;
const scanner = new Bun.Transpiler({ loader: 'tsx' });

/** Selection itself is diffed by map (diffTouchfileMaps), and test files are never dependencies. */
const SELECTION_MODULES = new Set(['test/helpers/touchfiles-data.ts', 'test/helpers/touchfiles.ts', 'test/helpers/test-selection.ts']);
const inScope = (file: string) => (file.startsWith('test/helpers/') || file.startsWith('test/fixtures/')) &&
  !file.endsWith('.test.ts') && !SELECTION_MODULES.has(file);

/**
 * Static test/helpers + test/fixtures closure of one paid test file, with literal fixture paths.
 * Traversal stops at `boundary` files (the global touchfiles): an edit there already selects every test.
 */
export function paidTestClosure(root: string, testFile: string, boundary: ReadonlySet<string> = new Set()): ClosureEntry[] {
  const seen = new Map<string, string[]>();
  const queue: Array<{ file: string; chain: string[] }> = [{ file: testFile, chain: [testFile] }];
  while (queue.length) {
    const { file, chain } = queue.shift()!;
    if (!/\.(ts|tsx|js|mjs)$/.test(file) || boundary.has(file)) continue;
    const source = fs.readFileSync(path.join(root, file), 'utf8');
    let imports: Array<{ path: string }> = [];
    try { imports = scanner.scanImports(source); } catch { imports = []; }
    for (const { path: specifier } of imports) {
      const target = resolveRepoSpecifier(root, file, specifier);
      if (!target || !inScope(target) || seen.has(target)) continue;
      seen.set(target, [...chain, target]);
      queue.push({ file: target, chain: [...chain, target] });
    }
    for (const match of source.match(LITERAL) ?? []) {
      const found = inScope(match) ? resolveRepoLiteral(root, match) : null;
      if (!found) continue;
      const literal = fs.statSync(path.join(root, found)).isDirectory() ? `${found}/**` : found;
      if (seen.has(literal)) continue;
      seen.set(literal, [...chain, `"${match}"`]);
      if (!literal.endsWith('/**')) queue.push({ file: literal, chain: [...chain, `"${match}"`] });
    }
  }
  return [...seen].map(([file, chain]) => ({ file, chain }));
}

/** True when a dependency is selected by the key's own list or the global list. */
export function isCovered(file: string, patterns: readonly string[], globals: readonly string[]): boolean {
  const probe = file.endsWith('/**') ? `${file.slice(0, -3)}/probe` : file;
  return [...patterns, ...globals].some(pattern => pattern === file || matchGlob(probe, pattern));
}
