import { afterEach, beforeEach, expect, spyOn, test } from 'bun:test';
import { callJudge } from './helpers/llm-judge';
import { WORKFLOW_JUDGE_RESPONSE_SCHEMA } from './helpers/workflow-judge-input';

const scores = { clarity: 4, completeness: 4, actionability: 4, reasoning: 'Complete workflow with explicit gates.' };
let originalKey: string | undefined;
let transport: ReturnType<typeof spyOn>;
let diagnostics: ReturnType<typeof spyOn>;
beforeEach(() => {
  originalKey = process.env.ANTHROPIC_API_KEY;
  process.env.ANTHROPIC_API_KEY = 'test-only-key';
  transport = spyOn(globalThis, 'fetch');
  diagnostics = spyOn(console, 'error').mockImplementation(() => {});
});
afterEach(() => {
  transport.mockRestore(); diagnostics.mockRestore();
  if (originalKey === undefined) delete process.env.ANTHROPIC_API_KEY;
  else process.env.ANTHROPIC_API_KEY = originalKey;
});

function response(stopReason = 'end_turn', text: string | null = JSON.stringify(scores)) {
  const events = [
    { type: 'message_start', message: { id: 'msg_fixture', type: 'message', role: 'assistant', model: 'claude-fable-5-1',
      content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 99023, output_tokens: 1 } } },
    { type: 'content_block_start', index: 0, content_block: { type: 'thinking', thinking: '', signature: '' } },
    { type: 'content_block_delta', index: 0, delta: { type: 'thinking_delta', thinking: 'PRIVATE_THINKING' } },
    { type: 'content_block_delta', index: 0, delta: { type: 'signature_delta', signature: 'PRIVATE_SIGNATURE' } },
    { type: 'content_block_stop', index: 0 },
    ...(text === null ? [] : [
      { type: 'content_block_start', index: 1, content_block: { type: 'text', text: '' } },
      { type: 'content_block_delta', index: 1, delta: { type: 'text_delta', text } },
      { type: 'content_block_stop', index: 1 },
    ]),
    { type: 'message_delta', delta: { stop_reason: stopReason, stop_sequence: null }, usage: { output_tokens: 65536 } },
    { type: 'message_stop' },
  ];
  return new Response(events.map(event => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join(''),
    { headers: { 'content-type': 'text/event-stream' } });
}

test('the pinned SDK refuses a default nonstreaming 64k request before network access', async () => {
  transport.mockImplementation(() => { throw new Error('Unexpected network access'); });
  await expect(callJudge('score the whole bundle', 'claude-fable-5-1', { max_tokens: 65_536 }))
    .rejects.toThrow('Streaming is required');
  expect(transport).not.toHaveBeenCalled();
});

test('the real SDK streams 64k requests and parses only completed public text', async () => {
  transport.mockResolvedValue(response());
  expect(await callJudge('score the whole bundle', 'claude-fable-5-1', {
    max_tokens: 65_536, stream: true, jsonSchema: WORKFLOW_JUDGE_RESPONSE_SCHEMA,
  })).toEqual(scores);
  expect(transport).toHaveBeenCalledTimes(1);
  const request = transport.mock.calls[0][1];
  expect(JSON.parse(request.body)).toEqual({ model: 'claude-fable-5-1', max_tokens: 65_536,
    output_config: { format: { type: 'json_schema', schema: WORKFLOW_JUDGE_RESPONSE_SCHEMA } },
    messages: [{ role: 'user', content: 'score the whole bundle' }], stream: true });
  expect(new Headers(request.headers).has('anthropic-beta')).toBe(false);
  expect(diagnostics).not.toHaveBeenCalled();
});

test('streamed truncation and refusal retain public evidence but never partial scores or private thinking', async () => {
  for (const [stopReason, text] of [['max_tokens', '{"clarity":4'], ['max_tokens', null], ['refusal', null]] as const) {
    transport.mockResolvedValue(response(stopReason, text));
    await expect(callJudge('score the whole bundle', 'claude-fable-5-1', {
      max_tokens: 65_536, stream: true, jsonSchema: WORKFLOW_JUDGE_RESPONSE_SCHEMA,
    })).rejects.toThrow(stopReason === 'max_tokens' ? 'truncated at max_tokens=65536' : 'provider refused');
    const diagnostic = diagnostics.mock.calls.at(-1)![0];
    expect(diagnostic).not.toContain('PRIVATE_');
    expect(JSON.parse(diagnostic)).toMatchObject({ stopReason, textBlocks: text === null ? [] : [text] });
  }
});

test('streaming retains the caller abort signal instead of extending its deadline', async () => {
  let started!: () => void;
  const ready = new Promise<void>(resolve => { started = resolve; });
  transport.mockImplementation((_url: string, init: RequestInit) => new Promise((_resolve, reject) => {
    init.signal!.addEventListener('abort', () => reject(init.signal!.reason), { once: true });
    started();
  }));
  const controller = new AbortController();
  const pending = callJudge('score the whole bundle', 'claude-fable-5-1', {
    max_tokens: 65_536, stream: true, jsonSchema: WORKFLOW_JUDGE_RESPONSE_SCHEMA, signal: controller.signal,
  });
  const failure = new Error('Original workflow walltime elapsed');
  const rejected = pending.catch(error => error);
  await ready;
  controller.abort(failure);
  expect(await rejected).toBe(failure);
  expect(transport).toHaveBeenCalledTimes(1);
});
