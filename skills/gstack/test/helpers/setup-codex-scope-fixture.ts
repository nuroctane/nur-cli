import { afterEach, expect } from 'bun:test';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, cpSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, readlinkSync, realpathSync, rmSync, symlinkSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

export const ROOT = resolve(import.meta.dir, '../..');
export const files = spawnSync('git', ['ls-files', '-z'], { cwd: ROOT, encoding: 'utf8', timeout: 10_000 });
if (files.status !== 0) throw new Error(files.stderr);
export const owned: string[] = [];
export const quote = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`;


export function assertFixtureWrite(file: string) {
  const root = owned.find(dir => file === dir || file.startsWith(dir + '/'));
  if (!root) throw new Error(`Fixture write outside owned roots: ${file}`);
  let existing = file;
  while (!existsSync(existing)) {
    if (lstatSync(existing, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error(`Unresolvable fixture link: ${existing}`);
    existing = dirname(existing);
  }
  const target = realpathSync(existing);
  const physicalRoot = realpathSync(root);
  if (target !== physicalRoot && !target.startsWith(physicalRoot + '/')) throw new Error(`Fixture write escapes physical root: ${file} -> ${target}`);
}
export function fixtureWriteFileSync(...args: Parameters<typeof writeFileSync>) {
  assertFixtureWrite(String(args[0]));
  return writeFileSync(...args);
}
export function fixtureCopyFileSync(...args: Parameters<typeof copyFileSync>) {
  assertFixtureWrite(String(args[1]));
  return copyFileSync(...args);
}
export function fixtureMkdirSync(...args: Parameters<typeof mkdirSync>) {
  assertFixtureWrite(String(args[0]));
  return mkdirSync(...args);
}
export function fixtureUtimesSync(...args: Parameters<typeof utimesSync>) {
  assertFixtureWrite(String(args[0]));
  return utimesSync(...args);
}

export function tree(dir: string): unknown {
  const stat = lstatSync(dir);
  if (stat.isSymbolicLink()) return { link: readlinkSync(dir) };
  if (stat.isDirectory()) return Object.fromEntries(readdirSync(dir).sort().map(name => [name, tree(join(dir, name))]));
  return createHash('sha256').update(readFileSync(dir)).digest('hex');
}

export function fixture(layout: string) {
  const dir = mkdtempSync(join(tmpdir(), 'gstack-codex-scope-'));
  owned.push(dir);
  const home = join(dir, 'home');
  const project = join(dir, 'project-a');
  const other = join(dir, 'project-b');
  const source = layout === 'machine' ? join(home, '.claude/skills/gstack')
    : layout === 'ordinary' ? join(project, 'custom-checkout') : join(project, layout, 'skills/gstack');
  const commands = join(dir, 'commands');
  for (const d of [home, other, commands, source]) fixtureMkdirSync(d, { recursive: true });
  for (const rel of [...files.stdout.split('\0').filter(Boolean), 'scripts/external-skill-names.ts', 'scripts/preflight-codex-overlap.ts']) {
    if (/^(?:test|docs|browse\/test|\.github)\//.test(rel)) continue;
    const dest = join(source, rel);
    fixtureMkdirSync(dirname(dest), { recursive: true });
    if (lstatSync(join(ROOT, rel)).isSymbolicLink()) {
      const target = readlinkSync(join(ROOT, rel));
      expect(resolve(dirname(dest), target).startsWith(source + '/')).toBe(true);
      symlinkSync(target, dest);
    } else fixtureCopyFileSync(join(ROOT, rel), dest);
  }
  const write = (file: string, content: string) => {
    fixtureMkdirSync(dirname(file), { recursive: true });
    fixtureWriteFileSync(file, content, { mode: 0o755 });
  };
  for (const rel of ['browse/dist/browse', 'design/dist/design', 'make-pdf/dist/pdf', 'browse/dist/.build-complete']) {
    const file = join(source, rel);
    write(file, '#!/bin/sh\nexit 0\n');
    fixtureUtimesSync(file, new Date('2040-01-01'), new Date('2040-01-01'));
  }
  write(join(commands, 'bun'), `#!/usr/bin/env bash
case "$*" in
  'install --frozen-lockfile') exit 0 ;;
  'build --help') echo 'Fixture Bun has no CSO compile flags'; exit 0 ;;
  'run build') echo 'Unexpected build in registration fixture' >&2; exit 90 ;;
  *) exec ${quote(process.execPath)} "$@" ;;
esac
`);
  const realRm = Bun.which('rm');
  if (!realRm) throw new Error('rm is required');
  write(join(commands, 'rm'), `#!/usr/bin/env bash
if [ "$#" -eq 2 ] && [ "$1" = -f ] && [ "$2" = /tmp/gstack-latest-version ]; then exit 0; fi
exec ${quote(realRm)} "$@"
`);
  for (const name of ['codex', 'claude']) write(join(commands, name), '#!/bin/sh\nexit 0\n');
  const global = join(home, '.codex/skills');
  const previous = join(dir, 'previous/gstack');
  write(join(previous, 'bin/gstack-autoplan-snapshot.ts'), readFileSync(join(ROOT, 'bin/gstack-autoplan-snapshot.ts'), 'utf8'));
  write(join(previous, 'lib/claude-bin.ts'), 'export {};\n');
  for (const name of ['gstack-review', 'gstack-claude', 'gstack-retired']) {
    write(join(previous, name, 'SKILL.md'), `---\nname: ${name}\n---\n<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->\n<!-- Regenerate: bun run gen:skill-docs -->\nPrior global skill.\n`);
    fixtureMkdirSync(global, { recursive: true });
    if (name === 'gstack-claude') {
      write(join(global, name, 'SKILL.md'), readFileSync(join(previous, name, 'SKILL.md'), 'utf8'));
    } else symlinkSync(join(previous, name), join(global, name), 'dir');
  }
  fixtureMkdirSync(join(global, 'gstack'), { recursive: true });
  symlinkSync(join(previous, 'bin'), join(global, 'gstack/bin'), 'dir');
  symlinkSync(join(previous, 'lib'), join(global, 'gstack/lib'), 'dir');
  write(join(global, 'gstack/SKILL.md'), '<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->\n<!-- Regenerate: bun run gen:skill-docs -->\nGlobal router.\n');
  write(join(global, 'custom/SKILL.md'), 'User-owned skill.\n');
  const env = {
    PATH: `${commands}:${process.env.PATH}`, HOME: home, USERPROFILE: home,
    CODEX_HOME: join(home, '.codex'), CLAUDE_CONFIG_DIR: join(home, '.claude'),
    GSTACK_HOME: join(home, '.gstack'), GSTACK_STATE_ROOT: join(home, '.gstack'),
    TMPDIR: dir, TMP: dir, TEMP: dir,
    GSTACK_SKIP_PLAYWRIGHT: '1', GSTACK_SKIP_FONTS: '1', GSTACK_SKIP_COREUTILS: '1', GSTACK_SKIP_ASIDE: '1', GSTACK_SKIP_GBRAIN_REGEN: '1',
    BUN_RUNTIME_TRANSPILER_CACHE_PATH: join(tmpdir(), 'gstack-preflight-bun-cache'),
  };
  write(join(home, '.gstack/config.yaml'), 'telemetry: off\nartifacts_sync: off\n');
  return { dir, source, home, project, other, global, previous, env };
}

export function install(f: ReturnType<typeof fixture>, args = '--host codex') {
  const result = spawnSync('bash', [join(f.source, 'setup'), ...args.split(' '), '--no-plan-tune-hooks', '--no-timeline-stop-hook', '--no-team'], {
    cwd: f.other, env: f.env, encoding: 'utf8', timeout: 60_000,
  });
  expect(result.status, result.stdout + result.stderr).toBe(0);
  return result;
}

export function cleanupOwnedFixtures() { for (const dir of owned.splice(0)) rmSync(dir, { recursive: true, force: true }); }
afterEach(cleanupOwnedFixtures);
