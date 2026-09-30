import * as fs from 'node:fs';
import * as path from 'node:path';
import { createHash } from 'node:crypto';
import { ownedPath, type QAMode } from './qa-functional-fixture';
import { qaEvidenceCommand } from './qa-evidence-producer';

export const QA_OBSERVER_LIMITS = [
  'Linux inotify only; unavailable kernel monitoring blocks acceptance.',
  'Kernel events detect write syscalls, links, renames, removals and Git writes, not memory-mapped writes or remote filesystems.',
  'A closed native-command interface rejects unobserved interpreters and shell composition; this is not a hostile-process sandbox.',
  'The observer covers the owned fixture tree, not arbitrary external paths or network destinations.',
];

export function qaWriteAllowed(relative: string, mode: QAMode): boolean {
  if (/^(?:\.qa-state|qa-reports)(?:\/|$)/.test(relative)) return true;
  return mode === 'qa' && /^(?:src|test)\//.test(relative);
}

function pathFailure(root: string, relative: string, error: unknown): Error {
  let cursor = root;
  let detail = 'missing';
  for (const part of relative.split(path.sep)) {
    cursor = path.join(cursor, part);
    try {
      const entry = fs.lstatSync(cursor, { throwIfNoEntry: false });
      detail = entry ? `dev=${entry.dev} ino=${entry.ino} nlink=${entry.nlink} mode=${(entry.mode & 0o777).toString(8)}` : 'missing';
      if (!entry || entry.isSymbolicLink() || (entry.isFile() && entry.nlink !== 1)) break;
    } catch { detail = 'stat unavailable'; break; }
  }
  return new Error(`${String(error)} [path=${relative} entry=${cursor} ${detail}]`);
}

export function qaTreeSnapshot(root: string): Record<string, string> {
  const result: Record<string, string> = {};
  const visit = (relative: string) => {
    let file: string;
    try { file = relative ? ownedPath(root, relative) : root; }
    catch (error) { throw pathFailure(root, relative, error); }
    const entry = fs.lstatSync(file);
    if (entry.isDirectory()) {
      if (relative) result[relative] = `directory:${entry.mode & 0o777}`;
      for (const name of fs.readdirSync(file).sort()) visit(path.join(relative, name));
    } else if (entry.isFile()) {
      result[relative] = `${entry.mode & 0o777}:${createHash('sha256').update(fs.readFileSync(file)).digest('hex')}`;
    } else throw new Error(`Unsupported fixture entry: ${relative}`);
  };
  visit('');
  return result;
}

export function decodeQAInotify(buffer: Buffer): Array<{ wd: number; mask: number; cookie: number; name: string }> {
  const records: Array<{ wd: number; mask: number; cookie: number; name: string }> = [];
  let offset = 0;
  while (offset < buffer.length) {
    if (buffer.length - offset < 16) throw new Error('truncated kernel event');
    const length = buffer.readUInt32LE(offset + 12);
    if (offset + 16 + length > buffer.length) throw new Error('truncated kernel event name');
    records.push({ wd: buffer.readInt32LE(offset), mask: buffer.readUInt32LE(offset + 4), cookie: buffer.readUInt32LE(offset + 8),
      name: buffer.subarray(offset + 16, offset + 16 + length).toString().replace(/\0.*$/s, '') });
    offset += 16 + length;
  }
  return records;
}

export interface QAWriteObservation {
  complete: boolean;
  failures: string[];
  events: Array<{ path: string; mask: number; cookie: number; at: number }>;
  changed: string[];
  before: Record<string, string>;
  after: Record<string, string>;
  limits: string[];
}

export async function observeQAWrites(root: string, options: { reportDirectory?: string; evidenceProducer?: boolean } = {}) {
  if (process.platform !== 'linux') throw new Error('QA write observer unavailable: Linux inotify required');
  if (fs.realpathSync(root) !== root) throw new Error('Observer root must be canonical');
  let reportDirectory: string | undefined;
  if (options.reportDirectory !== undefined) {
    const directory = ownedPath(root, options.reportDirectory);
    if (!fs.lstatSync(directory).isDirectory()) throw new Error('Observer report path must be an owned directory');
    reportDirectory = path.relative(root, directory);
  }
  const transientFile = (relative: string) => qaWriteAllowed(relative, 'qa-only')
    || (reportDirectory !== undefined && relative.startsWith(reportDirectory + path.sep));
  const before = qaTreeSnapshot(root);
  const { dlopen, FFIType, ptr } = await import('bun:ffi');
  const libc = dlopen('libc.so.6', {
    inotify_init1: { args: [FFIType.i32], returns: FFIType.i32 },
    inotify_add_watch: { args: [FFIType.i32, FFIType.ptr, FFIType.u32], returns: FFIType.i32 },
  });
  const fd = libc.symbols.inotify_init1(0x800 | 0x80000);
  if (fd < 0) { libc.close(); throw new Error('inotify initialization failed'); }
  const watches = new Map<number, { relative: string; directory: boolean }>();
  const events: QAWriteObservation['events'] = [];
  const failures: string[] = [];
  const publications = new Map<string, { temporary: string; dev: number; ino: number; bytes: string; parentDev: number; parentIno: number; mode: number }>();
  let stopped = false;
  const observedPath = (relative: string, knownPair = false): string => {
    try {
      if (knownPair) throw new Error('Fixture path traverses a link');
      return relative ? ownedPath(root, relative) : root;
    } catch (error) {
      const basename = path.basename(relative);
      const deadlineTemporary = /^\.qa-deadline-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
      const evidence = options.evidenceProducer && relative.startsWith((reportDirectory ?? 'qa-reports') + path.sep)
        ? /^(exploration-\d{3}\.json|evidence\.json|receipt\.json)(?:\.tmp\.[1-9]\d*\.[a-f0-9]{8})?$/.exec(basename) : null;
      const isDeadline = basename === 'deadline.json' || deadlineTemporary.test(basename);
      const targetName = isDeadline ? 'deadline.json' : evidence?.[1];
      const mode = isDeadline ? 0o400 : 0o600;
      const temporaryName = isDeadline ? deadlineTemporary : new RegExp(`^${targetName?.replaceAll('.', '\\.')}\\.tmp\\.[1-9]\\d*\\.[a-f0-9]{8}$`);
      if (!/^(?:reports|qa-reports|\.qa-state)\//.test(relative)
        || !targetName || (!isDeadline && targetName === 'receipt.json' && !/\/\.qa-evidence\/\d{3}\//.test(relative))) throw error;
      const parent = ownedPath(root, path.dirname(relative));
      const parentStat = fs.lstatSync(parent);
      const target = path.join(parent, targetName);
      let receipt: number | undefined;
      try {
        receipt = fs.openSync(target, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
        const entry = fs.fstatSync(receipt);
        if (!parentStat.isDirectory() || parentStat.uid !== fs.lstatSync(root).uid
          || !entry.isFile() || entry.uid !== parentStat.uid || entry.nlink !== 2
          || (entry.mode & 0o777) !== mode || entry.size > (isDeadline ? 4096 : 8 * 1024 * 1024)) throw error;
        const aliases = fs.readdirSync(parent).filter(name => {
          if (!temporaryName.test(name)) return false;
          const alias = fs.lstatSync(path.join(parent, name), { throwIfNoEntry: false });
          return alias?.isFile() && alias.dev === entry.dev && alias.ino === entry.ino && alias.nlink === 2;
        });
        if (aliases.length !== 1 || (basename !== targetName && basename !== aliases[0])) throw error;
        const bytes = fs.readFileSync(receipt, 'utf8');
        const state = JSON.parse(bytes);
        const canonicalUTC = (value: unknown) => typeof value === 'string' && Number.isFinite(Date.parse(value))
          && [new Date(value).toISOString(), new Date(value).toISOString().replace('.000Z', 'Z')].includes(value);
        if (isDeadline && (!state || Object.keys(state).sort().join(',') !== 'budgetMs,deadlineAt,startedAt,version'
          || state.version !== 1 || !Number.isSafeInteger(state.budgetMs) || state.budgetMs <= 0 || state.budgetMs > 2_147_483_647
          || !canonicalUTC(state.startedAt) || !canonicalUTC(state.deadlineAt)
          || Date.parse(state.deadlineAt) > Date.parse(state.startedAt) + state.budgetMs)) throw error;
        if (!isDeadline && (!state || typeof state !== 'object' || Array.isArray(state))) throw error;
        if (targetName.startsWith('exploration-') && Object.keys(state).sort().join(',') !== 'hypothesis,nextCommand,observationCommand,observed') throw error;
        if (targetName === 'receipt.json' && (state.version !== 1 || !/^\d{3}$/.test(state.id) || !['complete', 'incomplete', 'sensitive'].includes(state.status))) throw error;
        if (targetName === 'evidence.json' && (!Array.isArray(state.evidence) || !Array.isArray(state.limits))) throw error;
        const final = fs.lstatSync(target);
        if (!final.isFile() || final.dev !== entry.dev || final.ino !== entry.ino || ![1, 2].includes(final.nlink)
          || (final.mode & 0o777) !== mode) throw error;
        const relativeTarget = path.relative(root, target);
        const publication = { temporary: path.join(path.dirname(relative), aliases[0]), dev: entry.dev, ino: entry.ino,
          bytes, parentDev: parentStat.dev, parentIno: parentStat.ino, mode };
        const previous = publications.get(relativeTarget);
        if (previous && JSON.stringify(previous) !== JSON.stringify(publication)) throw error;
        publications.set(relativeTarget, publication);
        return target;
      } catch (publicationError) {
        try { return ownedPath(root, relative); } catch { throw publicationError; }
      } finally { if (receipt !== undefined) fs.closeSync(receipt); }
    }
  };
  const add = (relative: string, fileHint = false) => {
    if (fileHint && transientFile(relative)) {
      const parent = ownedPath(root, path.dirname(relative));
      const entry = fs.lstatSync(path.join(parent, path.basename(relative)), { throwIfNoEntry: false });
      if (entry?.isSymbolicLink() || (entry?.isFile() && entry.nlink > 2)) throw new Error('Fixture path traverses a link');
      if (entry?.isFile() && entry.nlink === 2) observedPath(relative, true);
      return;
    }
    const file = observedPath(relative);
    const entry = fs.lstatSync(file);
    if (entry.isFile() && transientFile(relative)) return;
    const name = Buffer.from(file + '\0');
    const wd = libc.symbols.inotify_add_watch(fd, ptr(name), 0x00000fce);
    if (wd < 0) throw new Error(`Could not watch ${relative}`);
    watches.set(wd, { relative: path.relative(root, file), directory: entry.isDirectory() });
    if (entry.isDirectory()) for (const child of fs.readdirSync(file, { withFileTypes: true })) {
      add(path.join(relative, child.name), child.isFile());
    }
  };
  const consume = (records: ReturnType<typeof decodeQAInotify>) => {
    for (const record of records) {
      if (record.mask & 0x4000) { failures.push('kernel queue overflow'); continue; }
      const watched = watches.get(record.wd);
      if (!watched) { failures.push('event for unknown watch'); continue; }
      const relative = path.join(watched.relative, record.name);
      if (record.mask & 0x8000) {
        if (watched.directory) failures.push(`directory watch lost: ${relative}`);
        watches.delete(record.wd);
        continue;
      }
      events.push({ path: relative === '.' ? '' : relative, mask: record.mask, cookie: record.cookie, at: Date.now() });
      if ((record.mask & 0x2000) || (watched.directory && (record.mask & 0x800))) failures.push(`watch target moved or unmounted: ${relative}`);
      if (record.mask & (0x100 | 0x80)) {
        try {
          const directory = !!(record.mask & 0x40000000);
          if (!directory && transientFile(relative)) add(relative, true);
          else {
            const target = observedPath(relative);
            if (fs.existsSync(target)) add(path.relative(root, target), !directory);
            else if (directory) failures.push(`new directory vanished before watch: ${relative}`);
          }
        } catch (error) { failures.push(String(pathFailure(root, relative, error))); }
      }
    }
  };
  const drain = () => {
    const buffer = Buffer.alloc(64 * 1024);
    try {
      for (;;) {
        let count: number;
        try { count = fs.readSync(fd, buffer, 0, buffer.length, null); }
        catch (error) { if ((error as NodeJS.ErrnoException).code === 'EAGAIN') break; throw error; }
        if (!count) throw new Error('kernel event stream closed');
        consume(decodeQAInotify(buffer.subarray(0, count)));
      }
    } catch (error) { failures.push(String(error)); }
  };
  try { add(''); } catch (error) { fs.closeSync(fd); libc.close(); throw error; }
  const timer = setInterval(drain, 10);
  const checkpoint = ownedPath(root, '.qa-state/.observer-check');
  fs.writeFileSync(checkpoint, 'start');
  drain();
  if (!events.some(event => event.path === '.qa-state/.observer-check')) failures.push('start marker was not observed');
  return {
    drain,
    injectKernelRecordsForTest: (bytes: Buffer) => {
      try { consume(decodeQAInotify(bytes)); } catch (error) { failures.push(String(error)); }
    },
    stop(): QAWriteObservation {
      if (stopped) throw new Error('Observer already stopped');
      stopped = true;
      clearInterval(timer);
      const previous = events.length;
      try { fs.writeFileSync(checkpoint, 'stop'); } catch (error) { failures.push(String(error)); }
      drain();
      if (!events.slice(previous).some(event => event.path === '.qa-state/.observer-check')) failures.push('stop marker was not observed');
      for (const [relative, publication] of publications) {
        try {
          const target = ownedPath(root, relative);
          const temporary = ownedPath(root, publication.temporary);
          const parent = fs.lstatSync(ownedPath(root, path.dirname(relative)));
          const entry = fs.lstatSync(target);
          if (fs.existsSync(temporary) || entry.dev !== publication.dev || entry.ino !== publication.ino || entry.nlink !== 1
            || (entry.mode & 0o777) !== publication.mode || parent.dev !== publication.parentDev || parent.ino !== publication.parentIno
            || fs.readFileSync(target, 'utf8') !== publication.bytes) throw new Error('Evidence publication did not settle unchanged');
        } catch (error) { failures.push(String(pathFailure(root, relative, error))); }
      }
      let after: Record<string, string> = {};
      try { after = qaTreeSnapshot(root); } catch (error) { failures.push(String(error)); }
      fs.closeSync(fd);
      libc.close();
      const changed = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter(file => before[file] !== after[file]);
      return { complete: failures.length === 0, failures, events, changed, before, after, limits: [...QA_OBSERVER_LIMITS] };
    },
  };
}

export function qaWriteVerdict(observation: QAWriteObservation, mode: QAMode): string[] {
  const failures = [...observation.failures];
  if (!observation.complete) failures.push('incomplete write observation');
  for (const file of new Set([...observation.events.map(event => event.path), ...observation.changed])) {
    if (!qaWriteAllowed(file, mode)) failures.push(`forbidden ${mode} write: ${file}`);
  }
  return failures;
}

export function qaCommandAllowed(command: string, root?: string): boolean {
  const producer = root ? qaEvidenceCommand(command, { cwd: root, reportRoot: path.join(root, 'qa-reports'), executable: path.join(root, 'bin/gstack-qa-evidence') }) : undefined;
  if (producer) {
    try { ownedPath(root!, 'bin/gstack-qa-evidence'); } catch { return false; }
    return producer.action !== 'capture' || producer.timeoutMs === 10000 && /^bun (?:run probe -- |cancel\.ts$)/.test(producer.nativeCommand!) && qaCommandAllowed(producer.nativeCommand!);
  }
  if (/[\n\r;&|<>`$\\(){}]/.test(command)) return false;
  if (command === 'date -u +%Y-%m-%dT%H:%M:%SZ') return true;
  const text = command.trim();
  return /^(?:pwd|ls(?: -la)?|git (?:status --(?:short|porcelain)|branch --show-current|diff(?: --stat)?|rev-parse HEAD)|bun (?:--version|cancel\.ts|test(?: test\/[a-zA-Z0-9_.-]+\.test\.ts)*))$/.test(text)
    || /^bun run (?:cli|probe) -- (?:balance|export|apply(?: (?:[a-zA-Z0-9_.+-]+|'[a-zA-Z0-9_.+ -]*'|"[a-zA-Z0-9_.+ -]*")){0,3})$/.test(text)
    || /^bun run probe -- (?:happy|reject|duplicate|partial|concurrent-ab|concurrent-ba|cancel|dependency)$/.test(text);
}

export function qaCommandPermission(root: string, event: any) {
  let allowed = false;
  try {
    allowed = path.isAbsolute(root) && fs.realpathSync(root) === root
      && event?.hook_event_name === 'PreToolUse' && event.cwd === root && event.tool_name === 'Bash'
      && typeof event.tool_input?.command === 'string'
      && [undefined, false].includes(event.tool_input.run_in_background)
      && qaCommandAllowed(event.tool_input.command, root);
  } catch {}
  return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: allowed ? 'allow' : 'deny',
    ...(!allowed ? { permissionDecisionReason: 'Only foreground commands from the owned functional fixture interface are authorized.' } : {}) } };
}

if (import.meta.main) {
  let event: unknown;
  try { event = JSON.parse(await Bun.stdin.text()); } catch {}
  console.log(JSON.stringify(qaCommandPermission(process.argv[2], event)));
}
