import * as fs from 'node:fs';
import * as path from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { qaEvidenceCommand, qaEvidenceHash, qaNativeCapture, qaProducerReceipt, type QaEvidenceContext } from './qa-evidence-producer';

const checkpointName = /^exploration-\d{3}\.json$/;
const object = (value: unknown): value is Record<string, any> => value !== null && typeof value === 'object' && !Array.isArray(value);

function safeRoot(root: string): void {
  if (!path.isAbsolute(root) || path.resolve(root) !== root || root === path.parse(root).root
    || !fs.lstatSync(root).isDirectory() || fs.realpathSync(root) !== root) throw new Error('Unsafe checkpoint report root');
}

export function readQACheckpointFiles(reportRoot: string): Record<string, string> {
  safeRoot(reportRoot);
  const files: Record<string, string> = {};
  for (const name of fs.readdirSync(reportRoot).filter(name => checkpointName.test(name)).sort()) {
    const target = path.join(reportRoot, name);
    const stat = fs.lstatSync(target);
    if (!stat.isFile() || stat.nlink !== 1) throw new Error(`Unsafe checkpoint file: ${name}`);
    const fd = fs.openSync(target, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
    try {
      const opened = fs.fstatSync(fd);
      if (!opened.isFile() || opened.nlink !== 1 || opened.dev !== stat.dev || opened.ino !== stat.ino) throw new Error(`Changed checkpoint file: ${name}`);
      files[name] = fs.readFileSync(fd, 'utf8');
    } finally { fs.closeSync(fd); }
  }
  return files;
}

type Probe = { command: string; observed: unknown };
type Call = { id: string; parent: string | null; name: string; input: Record<string, any>; start: number; end: number; output: string; failed: boolean; file?: { path: string; content: string } };

export function nativeCalls(transcript: unknown[], failures: string[]): Call[] {
  const calls = new Map<string, Call>();
  for (const [index, raw] of transcript.entries()) {
    if (!object(raw)) { failures.push('Malformed native event'); continue; }
    if (!['assistant', 'user'].includes(raw.type)) continue;
    const parent = raw.parent_tool_use_id ?? null;
    if (parent !== null && typeof parent !== 'string') { failures.push('Malformed native parent scope'); continue; }
    if (!Array.isArray(raw.message?.content)) {
      if (raw.message?.content != null && typeof raw.message.content !== 'string') failures.push('Malformed native message content');
      continue;
    }
    for (const block of raw.message.content) {
      if (!object(block)) { failures.push('Malformed native content block'); continue; }
      if (raw.type === 'assistant' && block.type === 'tool_use') {
        const key = JSON.stringify([parent, block.id]);
        if (typeof block.id !== 'string' || !block.id || calls.has(key) || typeof block.name !== 'string' || !object(block.input)) {
          failures.push('Missing or duplicate native tool identity');
          continue;
        }
        calls.set(key, { id: block.id, parent, name: block.name, input: block.input, start: index, end: -1, output: '', failed: false });
      } else if (raw.type === 'user' && block.type === 'tool_result') {
        const call = calls.get(JSON.stringify([parent, block.tool_use_id]));
        if (!call || call.end !== -1) { failures.push('Orphaned or duplicate native result'); continue; }
        call.end = index;
        call.failed = block.is_error === true || call.name === 'Bash' && raw.tool_use_result?.interrupted === true;
        if (typeof block.content === 'string') call.output = block.content;
        else if (Array.isArray(block.content) && block.content.every(part => object(part) && part.type === 'text' && typeof part.text === 'string')) {
          call.output = block.content.map(part => part.text).join('\n');
        } else { call.failed = true; failures.push('Unsupported native result content'); }
        const file = raw.tool_use_result?.type === 'text' ? raw.tool_use_result.file : undefined;
        if (call.name === 'Read' && !call.failed && object(file) && typeof call.input.file_path === 'string' && file.filePath === call.input.file_path && typeof file.content === 'string'
          && file.startLine === 1 && file.numLines === file.content.split('\n').length && file.totalLines === file.numLines
          && call.output === file.content.split('\n').map((line: string, index: number) => `${index + 1}\t${line}`).join('\n')) {
          call.file = { path: file.filePath, content: file.content };
        }
      }
    }
  }
  for (const call of calls.values()) {
    if (call.end < 0) failures.push('Missing native tool completion');
  }
  return [...calls.values()];
}

function nativeJSON(output: string): unknown[] {
  try { return [JSON.parse(output)]; } catch {}
  return output.split('\n').flatMap(line => {
    try { const value = JSON.parse(line); return object(value) || Array.isArray(value) ? [value] : []; } catch { return []; }
  });
}

export function validateQACheckpoints(input: {
  transcript: unknown[];
  reportRoot: string;
  probes: Probe[];
  requiredProbes: Probe[];
  additionalTargets?: Array<{ command: string; output: string }>;
  files: Record<string, string>;
  reportMarkdown: string;
  producer?: QaEvidenceContext;
}): string[] {
  const failures: string[] = [];
  let disk: Record<string, string>;
  try { disk = readQACheckpointFiles(input.reportRoot); }
  catch { return ['Unsafe or missing checkpoint report root/artifact']; }
  if (!isDeepStrictEqual(disk, input.files)) failures.push('Checkpoint files differ from actual disk artifacts');
  const calls = nativeCalls(input.transcript, failures);
  const bound: Array<{ probe: Probe; call: Call }> = [];
  for (const probe of input.probes) {
    const matches = calls.filter(call => {
      if (call.name !== 'Bash' || call.input.command !== probe.command || call.end <= call.start) return false;
      const capture = qaNativeCapture(call, input.producer);
      const produced = qaEvidenceCommand(call.input.command, input.producer) || /^bun \S*\/gstack-qa-evidence(?:\s|['"]\s)/.test(call.input.command)
        || call.output.split('\n').some(line => line.startsWith('QA_EVIDENCE '));
      return produced ? !!capture && isDeepStrictEqual(capture.captured.observed, probe.observed) : isDeepStrictEqual(nativeJSON(call.output), [probe.observed]);
    });
    if (matches.length !== 1 || bound.some(row => row.call === matches[0])) failures.push(`Unbound or ambiguous native probe: ${probe.command}`);
    else bound.push({ probe, call: matches[0] });
  }
  bound.sort((a, b) => a.call.start - b.call.start);
  const notes: Array<{ name: string; call: Call; value: Record<string, any>; intent?: Call }> = [];
  const written = new Set<string>();
  for (const call of calls) {
    const producerCommand = call.name === 'Bash' ? qaEvidenceCommand(call.input.command, input.producer) : undefined;
    let attempted = call.input.file_path;
    let content = call.input.content;
    let intent: Call | undefined;
    if (producerCommand?.action === 'checkpoint') {
      const producer = qaProducerReceipt(call, input.producer);
      const name = `exploration-${producerCommand.id}.json`;
      const writes = producerCommand.intent ? [call] : calls.filter(write => write.name === 'Write' && !write.failed && write.parent === call.parent
        && write.end > write.start && write.end < call.start && write.input.file_path === producerCommand.source
        && typeof write.input.content === 'string' && qaEvidenceHash(write.input.content) === producer?.receipt.intentSha256);
      if (!producer || writes.length !== 1 || typeof disk[name] !== 'string' || qaEvidenceHash(disk[name]) !== producer.receipt.sha256) {
        failures.push(`Checkpoint lacks completed native producer and intent: ${name}`);
        continue;
      }
      intent = writes[0];
      let decision: any;
      let published: any;
      const intentText = producerCommand.intent ? JSON.stringify(producerCommand.intent) : intent.input.content;
      try { decision = JSON.parse(intentText); } catch {}
      try { published = JSON.parse(disk[name]); } catch {}
      const captures = bound.filter(row => row.call.parent === call.parent && row.call.end < intent!.start
        && row.probe.command === decision?.observationCommand).map(row => ({ row, producer: qaNativeCapture(row.call, input.producer) }))
        .filter(row => row.producer?.command.id === producer.receipt.capture && row.producer.receipt.sha256 === producer.receipt.captureSha256);
      const capture = captures.length === 1 ? captures[0] : undefined;
      const read = capture && (capture.producer!.command.publicOutput || calls.some(read => read.name === 'Read' && !read.failed && read.parent === call.parent
        && read.start > capture.row.call.end && read.end > read.start && read.end < intent!.start
        && read.file?.path === path.join(input.reportRoot, `.qa-evidence/${producer.receipt.capture}/observation.json`)
        && read.file.content === capture.producer!.captured.observationText));
      if (!capture || !read || !object(decision) || decision.capture !== producer.receipt.capture
        || qaEvidenceHash(intentText) !== producer.receipt.intentSha256
        || !isDeepStrictEqual(Object.keys(decision).sort(), ['capture', 'hypothesis', 'nextCommand', 'observationCommand'])
        || !isDeepStrictEqual(published, { observationCommand: decision.observationCommand, observed: capture.row.probe.observed, hypothesis: decision.hypothesis, nextCommand: decision.nextCommand })
        || producerCommand.source && calls.some(change => ['Write', 'Edit'].includes(change.name) && change.input.file_path === producerCommand.source
          && change.start > intent!.end && change.start < call.end)) {
        failures.push(`Checkpoint intent lacks its completed observation read: ${name}`);
        continue;
      }
      attempted = path.join(input.reportRoot, name);
      content = disk[name];
    }
    if (call.name === 'Bash' && producerCommand?.action !== 'checkpoint' && typeof call.input.command === 'string' && /exploration-\d+\.json/.test(call.input.command)) {
      failures.push('Unsupported checkpoint Bash interaction');
    }
    if (typeof attempted !== 'string' || !path.basename(attempted).startsWith('exploration-')) continue;
    if (call.name === 'Read') continue;
    const name = path.basename(attempted);
    if ((call.name !== 'Write' && !intent) || !checkpointName.test(name) || attempted !== path.join(input.reportRoot, name)) {
      failures.push(`Unsupported checkpoint write/path: ${attempted}`);
      continue;
    }
    if (written.has(name)) failures.push(`Reused or overwritten checkpoint: ${name}`);
    written.add(name);
    if (call.failed || call.end <= call.start) { failures.push(`Checkpoint Write did not complete successfully: ${name}`); continue; }
    if (typeof content !== 'string' || !Object.hasOwn(disk, name) || disk[name] !== content) {
      failures.push(`Checkpoint artifact differs from captured Write: ${name}`);
      continue;
    }
    const destinations = [...input.reportMarkdown.matchAll(/\[[^\]\n]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)/g)].map(match => match[1]);
    if (!destinations.some(destination => destination === name || destination === `./${name}` || destination === path.join(input.reportRoot, name))) {
      failures.push(`Report does not link checkpoint: ${name}`);
    }
    let value: unknown;
    try { value = JSON.parse(content); } catch {}
    if (!object(value) || !isDeepStrictEqual(Object.keys(value).sort(), ['hypothesis', 'nextCommand', 'observationCommand', 'observed'])
      || typeof value.hypothesis !== 'string' || value.hypothesis.trim().length <= 20
      || !/[a-z]{3}/i.test(value.hypothesis) || typeof value.observationCommand !== 'string' || typeof value.nextCommand !== 'string') {
      failures.push(`Invalid checkpoint schema: ${name}`);
      continue;
    }
    notes.push({ name, call, value, ...(intent ? { intent } : {}) });
  }
  for (const name of Object.keys(disk)) if (!written.has(name)) failures.push(`Checkpoint has no public Write: ${name}`);
  const additional: Array<{ command: string; call: Call }> = [];
  for (const target of input.additionalTargets ?? []) {
    if (!notes.some(note => note.value.nextCommand === target.command)) continue;
    const matches = calls.filter(call => call.name === 'Bash' && call.input.command === target.command
      && call.end > call.start && call.output === target.output);
    if (matches.length !== 1 || bound.some(row => row.call === matches[0]) || additional.some(row => row.call === matches[0])) {
      failures.push(`Unbound or ambiguous additional checkpoint target: ${target.command}`);
    } else additional.push({ command: target.command, call: matches[0] });
  }
  const owners = new Map<Call, typeof notes>();
  for (const target of [...bound.map(row => ({ command: row.probe.command, call: row.call })), ...additional]) {
    const previous = bound.filter(row => row.call.parent === target.call.parent && row.call.start < target.call.start).at(-1);
    owners.set(target.call, notes.filter(note => previous && note.call.parent === target.call.parent
      && note.call.start > previous.call.end && note.call.end < target.call.start
      && (!note.intent || note.intent.start > previous.call.end)
      && note.value.observationCommand === previous.probe.command && isDeepStrictEqual(note.value.observed, previous.probe.observed)
      && note.value.nextCommand === target.command
      && !calls.some(call => call.name === 'Bash' && call.parent === target.call.parent && call.input.command === target.command
        && call.start >= note.call.start && call.start < target.call.start)));
  }
  const targets = new Set<Call>();
  for (const required of input.requiredProbes) {
    const matches = bound.filter(row => row.probe.command === required.command && isDeepStrictEqual(row.probe.observed, required.observed));
    if (matches.length !== 1 || targets.has(matches[0].call)) { failures.push(`Unbound or reused checkpoint target: ${required.command}`); continue; }
    const target = matches[0];
    targets.add(target.call);
    if (owners.get(target.call)?.length !== 1) failures.push(`Missing unique completed checkpoint before probe: ${required.command}`);
  }
  for (const note of notes) {
    const matches = [...owners.values()].filter(candidates => candidates.includes(note));
    if (matches.length !== 1 || matches[0].length !== 1) failures.push(`Unrelated, reused or retrospective checkpoint: ${note.name}`);
  }
  return failures.map(failure => `QA checkpoint: ${failure}`);
}
