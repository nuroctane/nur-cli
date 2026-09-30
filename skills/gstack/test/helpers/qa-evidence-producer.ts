import * as fs from 'node:fs';
import * as path from 'node:path';
import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { readQaCapture } from '../../lib/qa-evidence';

export const QA_EVIDENCE_RUNTIME = [
  'bin/gstack-qa-evidence', 'bin/gstack-qa-deadline', 'lib/qa-evidence.ts', 'lib/qa-deadline.ts',
  'lib/claude-code-windows-job.ts', 'lib/fs-atomic.ts', 'lib/redact-engine.ts', 'lib/redact-patterns.ts',
];

export function qaEvidenceRuntimeFiles(): Record<string, string> {
  return Object.fromEntries(QA_EVIDENCE_RUNTIME.map(file => [file, fs.readFileSync(path.resolve(import.meta.dir, '../..', file), 'utf8')]));
}

export interface QaEvidenceContext {
  cwd: string;
  reportRoot: string;
  executable: string;
}

export interface QaEvidenceCommand {
  action: 'capture' | 'checkpoint' | 'materialize';
  id?: string;
  source?: string;
  nativeCommand?: string;
  argv?: string[];
  deadline?: string;
  timeoutMs?: number;
  publicOutput?: boolean;
  intent?: { capture: string; observationCommand: string; hypothesis: string; nextCommand: string };
}

export function qaEvidenceCommand(command: string, context?: QaEvidenceContext): QaEvidenceCommand | undefined {
  if (!context || typeof command !== 'string' || /[\r\n]/.test(command)) return;
  const matches = [...command.matchAll(/'[^'\r\n]*'|"[^"\\$`\r\n]*"|[^\s'"\\;&|<>`$(){}*?\[\]~#]+/g)];
  if (!matches.length || command.slice(0, matches[0].index).trim() || command.slice(matches.at(-1)!.index! + matches.at(-1)![0].length).trim()) return;
  for (let index = 1; index < matches.length; index++) {
    if (!/^ +$/.test(command.slice(matches[index - 1].index! + matches[index - 1][0].length, matches[index].index))) return;
  }
  const tokens = matches.map(match => /^['"]/.test(match[0]) ? match[0].slice(1, -1) : match[0]);
  if ([tokens[1], tokens[3]].some(value => value?.split(/[\\/]/).includes('..'))) return;
  if (tokens[0] !== 'bun' || path.resolve(context.cwd, tokens[1] ?? '') !== context.executable
    || path.resolve(context.cwd, tokens[3] ?? '') !== context.reportRoot) return;
  const source = (value: string | undefined) => value && !value.split(/[\\/]/).includes('..')
    && path.resolve(context.reportRoot, value).startsWith(context.reportRoot + path.sep) ? path.resolve(context.reportRoot, value) : undefined;
  if (tokens[2] === 'materialize' && tokens.length === 5 && source(tokens[4])) return { action: 'materialize', source: source(tokens[4]) };
  if (!/^\d{3}$/.test(tokens[4] ?? '')) return;
  if (tokens[2] === 'checkpoint' && tokens.length === 6 && source(tokens[5])) return { action: 'checkpoint', id: tokens[4], source: source(tokens[5]) };
  if (tokens[2] === 'checkpoint' && tokens.length === 9 && /^\d{3}$/.test(tokens[5])) return { action: 'checkpoint', id: tokens[4],
    intent: { capture: tokens[5], observationCommand: tokens[6], hypothesis: tokens[7], nextCommand: tokens[8] } };
  const publicOutput = tokens[5] === '--public';
  const option = publicOutput ? 6 : 5;
  if (tokens[2] !== 'capture' || tokens[option + 2] !== '--' || tokens.length < option + 4) return;
  const deadline = tokens[option] === '--deadline' ? source(path.resolve(context.cwd, tokens[option + 1])) : undefined;
  if (tokens[option] === '--deadline' && !deadline) return;
  if (tokens[option] === '--timeout-ms' && (!/^[1-9]\d*$/.test(tokens[option + 1]) || Number(tokens[option + 1]) > 2_147_483_647)) return;
  if (!['--deadline', '--timeout-ms'].includes(tokens[option])) return;
  return { action: 'capture', id: tokens[4], publicOutput, argv: tokens.slice(option + 3), nativeCommand: command.slice(matches[option + 3].index).trim(),
    ...(tokens[option] === '--deadline' ? { deadline } : { timeoutMs: Number(tokens[option + 1]) }) };
}

export type QaProducerCall = { name: string; input: Record<string, any>; output: string; failed: boolean; start: number; end: number };

export function qaProducerReceipt(call: QaProducerCall, context?: QaEvidenceContext, status: 'complete' | 'incomplete' = 'complete') {
  if (call.name !== 'Bash' || call.end <= call.start) return;
  const command = qaEvidenceCommand(call.input.command, context);
  if (!command) return;
  try {
    const lines = call.output.split('\n').filter(line => line.startsWith('QA_EVIDENCE '));
    if (lines.length !== 1) return;
    const receipt = JSON.parse(lines[0].slice('QA_EVIDENCE '.length));
    const exitCode = call.failed ? Number(/^Exit code (\d+)\n/.exec(call.output)?.[1]) : 0;
    if (receipt.producer !== 'gstack-qa-evidence' || receipt.version !== 1 || receipt.action !== command.action
      || receipt.status !== status || !/^[a-f0-9]{64}$/.test(receipt.sha256)
      || receipt.exitCode !== exitCode
      || (command.id !== undefined && receipt.id !== command.id)
      || (call.failed && (command.action !== 'capture' || receipt.exitCode === 0))) return;
    return { command, receipt };
  } catch { return; }
}

export function qaNativeCapture(call: QaProducerCall, context?: QaEvidenceContext) {
  const producer = qaProducerReceipt(call, context);
  if (!context || producer?.command.action !== 'capture') return;
  try {
    const captured = readQaCapture(context.reportRoot, producer.command.id!, producer.receipt.sha256);
    if (captured.receipt.cwd !== context.cwd || !isDeepStrictEqual(captured.receipt.argv, producer.command.argv)
      || captured.receipt.exitCode !== producer.receipt.exitCode || captured.receipt.signal !== producer.receipt.signal
      || captured.receipt.publicOutput !== producer.command.publicOutput || captured.receipt.publicOutput !== producer.receipt.publicOutput
      || (producer.command.deadline !== undefined && captured.receipt.deadline !== producer.command.deadline)) return;
    if (producer.command.publicOutput && call.output.split('\n').filter(line => line === JSON.stringify(captured.observed)).length !== 1) return;
    if (producer.command.timeoutMs !== undefined) {
      const deadline = JSON.parse(fs.readFileSync(path.join(context.reportRoot, `.qa-evidence/${producer.command.id}/deadline.json`), 'utf8'));
      if (captured.receipt.deadline !== path.join(context.reportRoot, `.qa-evidence/${producer.command.id}/deadline.json`)
        || deadline.budgetMs !== producer.command.timeoutMs) return;
    }
    return { ...producer, captured };
  } catch { return; }
}

export function qaEvidenceHash(bytes: string): string {
  return createHash('sha256').update(bytes).digest('hex');
}
