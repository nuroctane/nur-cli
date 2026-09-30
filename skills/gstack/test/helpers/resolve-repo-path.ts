/**
 * Resolve a module specifier or path literal from a repo file to a repo-relative
 * path, or null when it names no file in the checkout. Bare packages and
 * `bun:` / `node:` builtins never resolve. Shared by the touchfile closure
 * invariant and the test-of-test ratchet.
 */
import * as fs from 'fs';
import * as path from 'path';

const isFile = (candidate: string) => fs.existsSync(candidate) && fs.statSync(candidate).isFile();

/** A relative module specifier (`./x`, `../lib/y.js`) → repo-relative file, or null. */
export function resolveRepoSpecifier(root: string, fromFile: string, specifier: string): string | null {
  if (!specifier.startsWith('.')) return null;
  const base = path.resolve(path.dirname(path.join(root, fromFile)), specifier);
  for (const candidate of [base, `${base}.ts`, base.replace(/\.js$/, '.ts'), `${base}.tsx`, path.join(base, 'index.ts')]) {
    if (isFile(candidate)) return toRelative(root, candidate);
  }
  return null;
}

/** A repo-rooted path literal (`test/fixtures/x.json`, `bin/gstack-x`) → itself when it exists, else null. */
export function resolveRepoLiteral(root: string, literal: string): string | null {
  const clean = literal.replace(/\/+$/, '');
  if (!clean || path.isAbsolute(clean) || clean.startsWith('..')) return null;
  return fs.existsSync(path.join(root, clean)) ? clean : null;
}

function toRelative(root: string, absolute: string): string | null {
  const relative = path.relative(root, absolute).split(path.sep).join('/');
  return relative.startsWith('..') ? null : relative;
}

/** Direct module specifiers of a source file: static imports/re-exports, require() and literal dynamic import(); `import type` excluded. */
export function directSpecifiers(source: string): string[] {
  const out: string[] = [];
  const typeOnly = /^\s*(?:import|export)\s+type\s/;
  for (const match of source.matchAll(/^[ \t]*(?:import|export)\b[^;'"`]*?from\s*(['"])([^'"]+)\1|^[ \t]*import\s*(['"])([^'"]+)\3/gm)) {
    if (typeOnly.test(match[0])) continue;
    out.push(match[2] ?? match[4]!);
  }
  for (const match of source.matchAll(/\b(?:require|import)\s*\(\s*(['"])([^'"]+)\1\s*\)/g)) out.push(match[2]!);
  return out;
}
