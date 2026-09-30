import * as fs from 'node:fs';
import * as path from 'node:path';
import { createHash } from 'node:crypto';
import { atomicWriteSync } from './fs-atomic';
import { runQaDeadlineCommand, runQaWindowsWorker, startQaDeadline, withQaReceiptOutput } from './qa-deadline';
import { scan } from './redact-engine';

const object = (value: unknown): value is Record<string, any> => value !== null && typeof value === 'object' && !Array.isArray(value);
const hash = (value: string | Buffer) => createHash('sha256').update(value).digest('hex');
const exact = (value: unknown, keys: string[]) => object(value) && Object.keys(value).sort().join(',') === keys.sort().join(',');
class QaEvidenceError extends Error {}

function id(value: string): string {
  if (!/^\d{3}$/.test(value)) throw new QaEvidenceError('Capture and checkpoint IDs must be three digits');
  return value;
}

export function qaEvidenceRoot(value: string): string {
  const root = path.resolve(value);
  if (!value || value.includes('\0') || value.split(/[\\/]/).includes('..') || root === path.parse(root).root
    || fs.realpathSync(root) !== root || !fs.lstatSync(root).isDirectory()
    || (process.getuid && fs.lstatSync(root).uid !== process.getuid())) throw new QaEvidenceError('Invalid report root');
  let current = root;
  while (current !== path.parse(current).root) {
    if (fs.lstatSync(current).isSymbolicLink()) throw new QaEvidenceError('Linked report root');
    current = path.dirname(current);
  }
  return root;
}

function owned(root: string, value: string): string {
  const target = path.resolve(root, value);
  if (!value || value.includes('\0') || value.split(/[\\/]/).includes('..') || !target.startsWith(root + path.sep)) throw new QaEvidenceError('Source must be inside the report root');
  let current = root;
  for (const part of path.relative(root, target).split(path.sep)) {
    current = path.join(current, part);
    const stat = fs.lstatSync(current, { throwIfNoEntry: false });
    if (stat && (stat.isSymbolicLink() || (!stat.isDirectory() && (!stat.isFile() || stat.nlink !== 1)))) throw new QaEvidenceError('Linked or nonregular evidence path');
    if (stat && process.getuid && stat.uid !== process.getuid()) throw new QaEvidenceError('Evidence path has a different owner');
  }
  return target;
}

function read(root: string, name: string): Buffer {
  const target = owned(root, name);
  const fd = fs.openSync(target, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0) | (fs.constants.O_NONBLOCK ?? 0));
  try {
    const stat = fs.fstatSync(fd);
    const current = fs.lstatSync(target);
    if (!stat.isFile() || stat.nlink !== 1 || stat.ino !== current.ino || stat.dev !== current.dev) throw new QaEvidenceError('Changed evidence source');
    return fs.readFileSync(fd);
  } finally { fs.closeSync(fd); }
}

function decode(bytes: Buffer): string {
  return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
}

function privateDirectory(root: string, name: string, exclusive = false): string {
  const target = owned(root, name);
  try { fs.mkdirSync(target, { mode: 0o700 }); }
  catch (error) { if (exclusive || (error as NodeJS.ErrnoException).code !== 'EEXIST') throw error; }
  const stat = fs.lstatSync(target);
  if (!stat.isDirectory() || (process.platform !== 'win32' && (stat.mode & 0o777) !== 0o700)) throw new QaEvidenceError('Evidence directory must be private');
  return target;
}

function publish(root: string, name: string, value: unknown): string {
  const bytes = JSON.stringify(value, null, 2) + '\n';
  atomicWriteSync(owned(root, name), bytes, { mode: 0o600, noReplace: true });
  return hash(bytes);
}

export function readQaCaptureRecord(reportRoot: string, captureId: string, expectedHash?: string) {
  const root = qaEvidenceRoot(reportRoot);
  const directory = `.qa-evidence/${id(captureId)}`;
  const receiptBytes = read(root, `${directory}/receipt.json`);
  if (expectedHash !== undefined && hash(receiptBytes) !== expectedHash) throw new QaEvidenceError('Capture differs from completed producer receipt');
  const receipt = JSON.parse(decode(receiptBytes));
  if (!exact(receipt, ['version', 'id', 'cwd', 'argv', 'deadline', 'timing', 'observation', 'publicOutput', 'startedAt', 'completedAt', 'exitCode', 'signal', 'status', 'stdout', 'stderr'])
    || receipt.version !== 1 || receipt.id !== captureId || !['complete', 'incomplete', 'sensitive'].includes(receipt.status)
    || receipt.signal !== null && typeof receipt.signal !== 'string' || typeof receipt.publicOutput !== 'boolean'
    || !Number.isInteger(receipt.exitCode) || receipt.exitCode < 0 || receipt.exitCode > 255
    || !path.isAbsolute(receipt.cwd) || !path.isAbsolute(receipt.deadline) || !Array.isArray(receipt.timing)
    || !Array.isArray(receipt.argv) || !receipt.argv.length || !receipt.argv.every((arg: unknown) => typeof arg === 'string')
    || !Number.isFinite(Date.parse(receipt.startedAt)) || !Number.isFinite(Date.parse(receipt.completedAt))
    || Date.parse(receipt.completedAt) < Date.parse(receipt.startedAt)) throw new QaEvidenceError('Incomplete or invalid capture');
  const stdout = read(root, `${directory}/stdout`);
  const stderr = read(root, `${directory}/stderr`);
  for (const [stream, bytes] of [['stdout', stdout], ['stderr', stderr]] as const) {
    if (!exact(receipt[stream], ['sha256', 'bytes']) || receipt[stream].sha256 !== hash(bytes) || receipt[stream].bytes !== bytes.length) throw new QaEvidenceError('Captured output changed');
  }
  return { receipt, sha256: hash(receiptBytes), stdout, stderr };
}

export function readQaCapture(reportRoot: string, captureId: string, expectedHash?: string) {
  const root = qaEvidenceRoot(reportRoot);
  const { receipt, sha256, stdout, stderr } = readQaCaptureRecord(root, captureId, expectedHash);
  if (receipt.status !== 'complete' || receipt.signal !== null) throw new QaEvidenceError('Incomplete capture cannot be published');
  const out = decode(stdout), err = decode(stderr);
  if (scan(out + '\n' + err + '\n' + JSON.stringify(receipt.argv)).findings.some(finding => finding.tier === 'HIGH')) throw new QaEvidenceError('Sensitive capture cannot be published');
  let observed: unknown = out;
  try { observed = JSON.parse(out); } catch {}
  const observationText = JSON.stringify(observed, null, 2) + '\n';
  if (!exact(receipt.observation, ['sha256', 'bytes']) || receipt.observation.sha256 !== hash(observationText)
    || receipt.observation.bytes !== Buffer.byteLength(observationText)
    || !read(root, `.qa-evidence/${id(captureId)}/observation.json`).equals(Buffer.from(observationText))) throw new QaEvidenceError('Observation view differs from captured output');
  return { receipt, sha256, stdout: out, stderr: err, observed, observationText };
}

async function capture(root: string, captureId: string, publicOutput: boolean, option: string, budget: string, command: string, args: string[]) {
  id(captureId);
  if (!command || !['--deadline', '--timeout-ms'].includes(option)) throw new QaEvidenceError('Capture requires a deadline or finite command timeout');
  if (option === '--timeout-ms' && (!/^[1-9]\d*$/.test(budget) || !Number.isSafeInteger(Number(budget)) || Number(budget) > 2_147_483_647)) throw new QaEvidenceError('Invalid command timeout');
  privateDirectory(root, '.qa-evidence');
  const directory = privateDirectory(root, `.qa-evidence/${captureId}`, true);
  const deadline = option === '--deadline' ? owned(root, path.resolve(budget)) : path.join(directory, 'deadline.json');
  if (option === '--timeout-ms') startQaDeadline(deadline, (Number(budget) / 1000).toFixed(3));
  const startedAt = new Date().toISOString();
  const fds = { stdout: fs.openSync(path.join(directory, 'stdout'), 'wx', 0o600), stderr: fs.openSync(path.join(directory, 'stderr'), 'wx', 0o600) };
  const digests = { stdout: createHash('sha256'), stderr: createHash('sha256') };
  const lengths = { stdout: 0, stderr: 0 };
  const timing: Record<string, unknown>[] = [];
  let result = { exitCode: 2, signal: null as NodeJS.Signals | null, completed: false };
  let exitCode: number;
  try {
    const emit = (_stream: 'stdout' | 'stderr', value: Record<string, unknown>, completion?: typeof result) => {
      timing.push({ guard: 'qa-deadline', ...value });
      if (completion) result = completion;
    };
    exitCode = process.platform === 'win32'
      ? await runQaWindowsWorker(['run', deadline, '--', command, ...args], emit, path.resolve(import.meta.dir, '../bin/gstack-qa-deadline'), 'qa-deadline-receipt', fds)
      : await runQaDeadlineCommand(deadline, command, args, emit, {
      write: (stream, chunk) => {
        fs.writeFileSync(fds[stream], chunk);
        digests[stream].update(chunk);
        lengths[stream] += chunk.length;
      },
      complete: value => { result = value; },
    });
    for (const stream of ['stdout', 'stderr'] as const) {
      const stat = fs.fstatSync(fds[stream]);
      const current = fs.lstatSync(owned(root, `.qa-evidence/${captureId}/${stream}`));
      if (stat.nlink !== 1 || stat.dev !== current.dev || stat.ino !== current.ino) throw new QaEvidenceError('Capture output was replaced');
      if (process.platform === 'win32') {
        const bytes = read(root, `.qa-evidence/${captureId}/${stream}`);
        digests[stream].update(bytes);
        lengths[stream] = bytes.length;
      }
    }
    fs.fsyncSync(fds.stdout);
    fs.fsyncSync(fds.stderr);
  } finally {
    fs.closeSync(fds.stdout);
    fs.closeSync(fds.stderr);
  }
  const stdout = read(root, `.qa-evidence/${captureId}/stdout`), stderr = read(root, `.qa-evidence/${captureId}/stderr`);
  const streams = {
    stdout: { sha256: digests.stdout.digest('hex'), bytes: lengths.stdout },
    stderr: { sha256: digests.stderr.digest('hex'), bytes: lengths.stderr },
  };
  let status = result.completed && result.exitCode === exitCode ? 'complete' : 'incomplete';
  if (streams.stdout.sha256 !== hash(stdout) || streams.stderr.sha256 !== hash(stderr)) status = 'incomplete';
  if (status === 'complete') {
    try {
      if (scan(decode(stdout) + '\n' + decode(stderr) + '\n' + JSON.stringify([command, ...args])).findings.some(finding => finding.tier === 'HIGH')) status = 'sensitive';
    } catch { status = 'incomplete'; }
  }
  let observation: { sha256: string; bytes: number } | null = null;
  if (status === 'complete') {
    let value: unknown = decode(stdout);
    try { value = JSON.parse(value as string); } catch {}
    const bytes = JSON.stringify(value, null, 2) + '\n';
    fs.writeFileSync(owned(root, `.qa-evidence/${captureId}/observation.json`), bytes, { flag: 'wx', mode: 0o600 });
    observation = { sha256: hash(bytes), bytes: Buffer.byteLength(bytes) };
  }
  const receipt = { version: 1, id: captureId, cwd: process.cwd(), argv: [command, ...args], deadline, timing, startedAt,
    completedAt: new Date().toISOString(), exitCode, signal: result.signal, status, observation, publicOutput,
    ...streams };
  const sha256 = publish(root, `.qa-evidence/${captureId}/receipt.json`, receipt);
  return { action: 'capture', id: captureId, status, sha256, exitCode, signal: result.signal, publicOutput };
}

function checkpoint(root: string, checkpointId: string, source: string | Record<string, string>) {
  id(checkpointId);
  const bytes = typeof source === 'string' ? read(root, source) : Buffer.from(JSON.stringify(source));
  const intent = JSON.parse(decode(bytes));
  if (!exact(intent, ['capture', 'observationCommand', 'hypothesis', 'nextCommand'])
    || typeof intent.capture !== 'string' || typeof intent.observationCommand !== 'string' || !intent.observationCommand.trim()
    || typeof intent.hypothesis !== 'string' || intent.hypothesis.trim().length <= 20 || !/[a-z]{3}/i.test(intent.hypothesis)
    || typeof intent.nextCommand !== 'string' || !intent.nextCommand.trim()) throw new QaEvidenceError('Invalid causal intent');
  if (scan(decode(bytes)).findings.some(finding => finding.tier === 'HIGH')) throw new QaEvidenceError('Sensitive intent cannot be published');
  const captured = readQaCapture(root, intent.capture);
  const value = { observationCommand: intent.observationCommand, observed: captured.observed, hypothesis: intent.hypothesis, nextCommand: intent.nextCommand };
  const sha256 = publish(root, `exploration-${checkpointId}.json`, value);
  return { action: 'checkpoint', id: checkpointId, status: 'complete', sha256, capture: intent.capture, captureSha256: captured.sha256, intentSha256: hash(bytes), exitCode: 0 };
}

function materialize(root: string, source: string) {
  const bytes = read(root, source);
  if (scan(decode(bytes)).findings.some(finding => finding.tier === 'HIGH')) throw new QaEvidenceError('Sensitive annotations cannot be published');
  const annotations = JSON.parse(decode(bytes));
  if (!exact(annotations, ['revision', 'runtime', 'cwd', 'limits', 'evidence', 'learning'])
    || !['revision', 'runtime', 'cwd'].every(key => typeof annotations[key] === 'string' && annotations[key].trim())
    || !Array.isArray(annotations.limits) || !annotations.limits.length || !annotations.limits.every((limit: unknown) => typeof limit === 'string' && limit.trim())
    || !Array.isArray(annotations.evidence) || !Array.isArray(annotations.learning)) throw new QaEvidenceError('Invalid report annotations');
  const captures = new Set<string>();
  const evidence = annotations.evidence.map((row: any) => {
    if (!exact(row, ['capture', 'command', 'contract', 'expected', 'classification'])
      || !Object.values(row).every(value => typeof value === 'string' && value.trim()) || captures.has(row.capture)) throw new QaEvidenceError('Invalid evidence annotation');
    captures.add(row.capture);
    const captured = readQaCapture(root, row.capture);
    return { command: row.command, contract: row.contract, expected: row.expected, classification: row.classification, observed: captured.observed };
  });
  const learning = annotations.learning.map((name: unknown) => {
    if (typeof name !== 'string') throw new QaEvidenceError('Invalid checkpoint reference');
    const note = JSON.parse(decode(read(root, `exploration-${id(name)}.json`)));
    if (!exact(note, ['observationCommand', 'observed', 'hypothesis', 'nextCommand'])) throw new QaEvidenceError('Invalid referenced checkpoint');
    return { observationCommand: note.observationCommand, hypothesis: note.hypothesis, nextCommand: note.nextCommand };
  });
  const sha256 = publish(root, 'evidence.json', { ...annotations, evidence, learning });
  return { action: 'materialize', status: 'complete', sha256, annotationsSha256: hash(bytes), exitCode: 0 };
}

export async function qaEvidenceMain(args: string[]): Promise<number> {
  return withQaReceiptOutput(false, 'qa-evidence-receipt', value => value.event === 'observation'
    ? JSON.stringify(value.observed) + '\n' : value.event === 'diagnostic' ? String(value.stderr)
      : '\nQA_EVIDENCE ' + JSON.stringify({ producer: 'gstack-qa-evidence', version: 1, ...value }) + '\n', async emit => {
    try {
      const [action, reportRoot, ...rest] = args;
      const root = qaEvidenceRoot(reportRoot);
      let receipt: Record<string, any>;
      const publicOutput = action === 'capture' && rest[1] === '--public';
      if (publicOutput) rest.splice(1, 1);
      if (action === 'capture' && rest.length >= 5 && rest[3] === '--') {
        receipt = await capture(root, rest[0], publicOutput, rest[1], rest[2], rest[4], rest.slice(5));
        if (publicOutput && receipt.status === 'complete') {
          const captured = readQaCapture(root, rest[0], receipt.sha256);
          emit('stdout', { event: 'observation', observed: captured.observed });
          if (captured.stderr) emit('stderr', { event: 'diagnostic', stderr: captured.stderr });
        }
      } else if (action === 'checkpoint' && rest.length === 2) receipt = checkpoint(root, rest[0], rest[1]);
      else if (action === 'checkpoint' && rest.length === 5) receipt = checkpoint(root, rest[0], { capture: rest[1], observationCommand: rest[2], hypothesis: rest[3], nextCommand: rest[4] });
      else if (action === 'materialize' && rest.length === 1) receipt = materialize(root, rest[0]);
      else throw new QaEvidenceError('Usage: capture ROOT ID [--public] --deadline FILE|--timeout-ms MS -- COMMAND ARGS | checkpoint ROOT ID CAPTURE OBSERVATION_COMMAND HYPOTHESIS NEXT_COMMAND | checkpoint ROOT ID INTENT_FILE | materialize ROOT ANNOTATIONS');
      emit('stdout', receipt);
      return receipt.status === 'complete' ? receipt.exitCode : receipt.status === 'incomplete' ? receipt.exitCode || 2 : 2;
    } catch (error) {
      emit('stderr', { action: 'error', message: error instanceof QaEvidenceError ? error.message : 'Evidence operation failed' });
      return 2;
    }
  });
}
