import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { DOC_PATH, fixtureDocs } from './helpers/docsync-fixture';
import { docsWriteFailures, observeDocsWrites } from './helpers/docsync-observer';
import { decodeQAInotify, qaWriteVerdict, type QAWriteObservation } from './helpers/qa-functional-observer';
import type { SkillTestResult } from './helpers/session-runner';

function nativeResult(fixture: ReturnType<typeof fixtureDocs>) {
  const result = { exitReason: 'success', transcript: [], toolCalls: [] } as unknown as SkillTestResult;
  return {
    result,
    replace(source: string, content: string, name: 'Edit' | 'Write' = 'Edit', parent: string | null = null) {
      const file = path.join(fixture.repo, DOC_PATH);
      const original = fs.readFileSync(file, 'utf8');
      const id = `native-${result.toolCalls.length}`;
      const input = name === 'Edit' ? { file_path: file, old_string: original, new_string: content, replace_all: false }
        : { file_path: file, content };
      result.transcript.push({ type: 'assistant', parent_tool_use_id: parent, message: { content: [{ type: 'tool_use', id, name, input }] } });
      const fd = fs.openSync(path.join(fixture.repo, source), 'wx', 0o600);
      fs.writeFileSync(fd, content);
      fs.fchmodSync(fd, fs.statSync(file).mode & 0o777);
      fs.closeSync(fd);
      fs.renameSync(path.join(fixture.repo, source), file);
      const output = 'Native callback result text is not mutation authority.';
      result.toolCalls.push({ tool: name, input, output });
      result.transcript.push({ type: 'user', parent_tool_use_id: parent,
        message: { content: [{ type: 'tool_result', tool_use_id: id, content: output }] },
        tool_use_result: name === 'Edit' ? { filePath: file, oldString: original, newString: content, replaceAll: false, originalFile: original, userModified: false }
          : { type: 'update', filePath: file, content, originalFile: original, userModified: false },
      });
    },
  };
}

const sibling = (name: string) => path.join(path.dirname(DOC_PATH), name);
type Capture = { fixture: ReturnType<typeof fixtureDocs>; result: SkillTestResult; observation: QAWriteObservation };
let captured: Capture;

(process.platform === 'linux' ? describe : describe.skip)('native docs atomic replacement attribution', () => {
  beforeAll(async () => {
    const fixture = fixtureDocs('updated');
    const observer = await observeDocsWrites(fixture);
    const native = nativeResult(fixture);
    native.replace(sibling('arbitrary-sibling'), 'First native content.\n');
    observer.drain();
    captured = { fixture, result: native.result, observation: observer.stop() };
  });
  afterAll(() => captured?.fixture.clean());

  const verdict = (capture: Capture) => docsWriteFailures(capture.observation, [DOC_PATH], capture);
  const copy = (): Capture => ({ fixture: captured.fixture, result: structuredClone(captured.result), observation: structuredClone(captured.observation) });

  test('decodes unsigned kernel cookies without changing event fields', () => {
    const bytes = Buffer.alloc(32);
    bytes.writeInt32LE(7, 0); bytes.writeUInt32LE(0x40, 4); bytes.writeUInt32LE(0xfedcba98, 8); bytes.writeUInt32LE(16, 12);
    bytes.write('sibling', 16);
    expect(decodeQAInotify(bytes)).toEqual([{ wd: 7, mask: 0x40, cookie: 0xfedcba98, name: 'sibling' }]);
  });

  test('attributes a real kernel lifecycle without authorizing its filename', () => {
    expect(captured.observation.complete).toBe(true);
    expect(verdict(captured)).toEqual([]);
    expect(docsWriteFailures(captured.observation, [DOC_PATH])).toEqual([`forbidden docs write: ${sibling('arbitrary-sibling')}`]);
    expect(captured.observation.before[sibling('arbitrary-sibling')]).toBeUndefined();
    expect(captured.observation.after[sibling('arbitrary-sibling')]).toBeUndefined();
    const moves = captured.observation.events.filter(event => event.mask === 0x40 || event.mask === 0x80);
    expect(moves).toHaveLength(2);
    expect(moves[0].cookie).toBeGreaterThan(0);
    expect(moves[0].cookie).toBe(moves[1].cookie);
    expect(captured.observation.changed.sort()).toEqual(['.qa-state/.observer-check', DOC_PATH].sort());
  });

  test('does not mutate raw evidence or broaden either QA verdict', () => {
    const original = structuredClone(captured.observation);
    const legacy = { ...original, events: original.events.map(({ cookie, ...event }) => event) } as QAWriteObservation;
    verdict(captured);
    expect(captured.observation).toEqual(original);
    for (const mode of ['qa', 'qa-only'] as const) {
      expect(qaWriteVerdict(captured.observation, mode)).toEqual(qaWriteVerdict(legacy, mode));
      expect(qaWriteVerdict(captured.observation, mode)).toContain(`forbidden ${mode} write: ${sibling('arbitrary-sibling')}`);
    }
  });

  test('read-only and actor source permissions never authorize an atomic document write', () => {
    expect(docsWriteFailures(captured.observation, [], captured).length).toBeGreaterThan(0);
    expect(docsWriteFailures(captured.observation, ['app.ts'], captured).length).toBeGreaterThan(0);
    expect(docsWriteFailures(captured.observation, [DOC_PATH], { ...captured, readOnly: true }).length).toBeGreaterThan(0);
  });

  const corruptions: Record<string, (capture: Capture) => void> = {
    'missing cookies': c => { c.observation.events.forEach(e => { delete (e as any).cookie; }); },
    'zero cookies': c => { c.observation.events.forEach(e => { e.cookie = 0; }); },
    'mismatched cookie': c => { c.observation.events.find(e => e.mask === 0x80)!.cookie++; },
    'reused cookie': c => { c.observation.events.push({ ...c.observation.events.find(e => e.mask === 0x40)! }); },
    'missing rename source': c => { c.observation.events = c.observation.events.filter(e => e.mask !== 0x40); },
    'missing rename destination': c => { c.observation.events = c.observation.events.filter(e => e.mask !== 0x80); },
    'rename to other path': c => { c.observation.events.find(e => e.mask === 0x80)!.path = 'app.ts'; },
    'protected destination': c => { c.observation.events.find(e => e.mask === 0x80)!.path = '.git/config'; },
    'cross-directory source': c => { c.observation.events.filter(e => e.path === sibling('arbitrary-sibling')).forEach(e => { e.path = 'elsewhere'; }); },
    'preexisting sibling': c => { c.observation.before[sibling('arbitrary-sibling')] = c.observation.before[DOC_PATH]; },
    'surviving sibling': c => { c.observation.after[sibling('arbitrary-sibling')] = c.observation.after[DOC_PATH]; },
    'missing create': c => { c.observation.events = c.observation.events.filter(e => e.path !== sibling('arbitrary-sibling') || e.mask !== 0x100); },
    'missing modify': c => { c.observation.events = c.observation.events.filter(e => e.path !== sibling('arbitrary-sibling') || e.mask !== 0x2); },
    'missing close': c => { c.observation.events = c.observation.events.filter(e => e.path !== sibling('arbitrary-sibling') || e.mask !== 0x8); },
    'post-close sibling write': c => { const at = c.observation.events.findIndex(e => e.mask === 0x40); c.observation.events.splice(at, 0, { ...c.observation.events[at], mask: 2, cookie: 0 }); },
    'forged directory source': c => { c.observation.events.find(e => e.path === sibling('arbitrary-sibling'))!.mask |= 0x40000000; },
    'unrelated syscall': c => { c.observation.events.push({ path: '.git/config', mask: 2, cookie: 0, at: 0 }); },
    'target write-restore': c => { c.observation.events.push({ path: DOC_PATH, mask: 2, cookie: 0, at: 0 }); },
    'target chmod-restore': c => { c.observation.events.push({ path: DOC_PATH, mask: 4, cookie: 0, at: 0 }); },
    'mode changed': c => { c.observation.after[DOC_PATH] = c.observation.after[DOC_PATH].replace(/^\d+:/, '384:'); },
    'incomplete observation': c => { c.observation.complete = false; },
    'kernel failure': c => { c.observation.failures.push('kernel queue overflow'); },
    'missing transcript': c => { c.result.transcript = []; },
    'missing completion': c => { c.result.transcript.pop(); },
    'failed completion': c => { c.result.transcript[1].message.content[0].is_error = true; },
    'malformed error flag': c => { c.result.transcript[1].message.content[0].is_error = 'false'; },
    'different parent scope': c => { c.result.transcript[1].parent_tool_use_id = 'other-parent'; },
    'duplicate tool identity': c => { c.result.transcript.unshift(structuredClone(c.result.transcript[0])); },
    'duplicate completion': c => { c.result.transcript.push(structuredClone(c.result.transcript[1])); },
    'unrelated successful edit': c => { c.result.transcript[0].message.content[0].input.file_path = path.join(c.fixture.repo, 'README.md'); },
    'forged success prose': c => { c.result.transcript[1].message.content[0].content = 'updated successfully'; delete c.result.transcript[1].tool_use_result; },
    'forged result path': c => { c.result.transcript[1].tool_use_result.filePath = path.join(c.fixture.repo, 'app.ts'); },
    'forged original content': c => { c.result.transcript[1].tool_use_result.originalFile += 'forged'; },
    'forged replacement payload': c => { c.result.transcript[1].tool_use_result.newString += 'forged'; },
    'unbound final content': c => { c.observation.after[DOC_PATH] = c.observation.before[DOC_PATH]; },
    'user-modified payload': c => { c.result.transcript[1].tool_use_result.userModified = true; },
    'unsettled capture': c => { c.result.exitReason = 'timeout'; },
    'shell outside authority': c => { c.result.toolCalls.push({ tool: 'Bash', input: { command: 'echo forged > hidden' }, output: '' }); },
  };
  for (const [name, corrupt] of Object.entries(corruptions)) {
    test(`rejects ${name}`, () => {
      const control = copy();
      expect(verdict(control)).toEqual([]);
      corrupt(control);
      expect(verdict(control).length).toBeGreaterThan(0);
    });
  }

  test.each(['ordered', 'overlap', 'restore', 'reuse', 'extra-rename', 'chain-mismatch', 'write-payload', 'write-type'])('binds multiple real replacements: %s', async fault => {
    const fixture = fixtureDocs('updated');
    const observer = await observeDocsWrites(fixture);
    const original = fs.readFileSync(path.join(fixture.repo, DOC_PATH), 'utf8');
    const native = nativeResult(fixture);
    try {
      native.replace(sibling('first'), 'first\n', 'Edit', 'native-child');
      observer.drain();
      native.replace(sibling(fault === 'reuse' ? 'first' : 'second'), fault === 'restore' ? original : 'second\n', 'Write', 'native-child');
      observer.drain();
      const observation = observer.stop();
      if (fault === 'overlap') [native.result.transcript[1], native.result.transcript[2]] = [native.result.transcript[2], native.result.transcript[1]];
      if (fault === 'extra-rename') native.result.transcript.splice(2);
      if (fault === 'chain-mismatch') native.result.transcript[3].tool_use_result.originalFile = original;
      if (fault === 'write-payload') native.result.transcript[3].tool_use_result.content = 'forged';
      if (fault === 'write-type') native.result.transcript[3].tool_use_result.type = 'create';
      const failures = docsWriteFailures(observation, [DOC_PATH], { fixture, result: native.result });
      if (fault === 'ordered') expect(failures).toEqual([]);
      else expect(failures.length).toBeGreaterThan(0);
    } finally { fixture.clean(); }
  });

  test.each(['write-restore', 'rename-other', 'survive', 'mode', 'hardlink', 'symlink', 'protected', 'preexisting', 'read-only'])('rejects actual filesystem %s', async fault => {
    const fixture = fixtureDocs('updated');
    const source = sibling('actual-sibling');
    const target = path.join(fixture.repo, DOC_PATH);
    if (fault === 'preexisting') fs.writeFileSync(path.join(fixture.repo, source), 'already here');
    const observer = await observeDocsWrites(fixture);
    const native = nativeResult(fixture);
    try {
      if (fault === 'preexisting') fs.unlinkSync(path.join(fixture.repo, source));
      native.replace(source, 'changed\n');
      observer.drain();
      if (fault === 'write-restore') { fs.writeFileSync(target, 'unauthorized'); fs.writeFileSync(target, 'changed\n'); }
      if (fault === 'rename-other') { fs.renameSync(target, path.join(fixture.repo, 'other')); fs.renameSync(path.join(fixture.repo, 'other'), target); }
      if (fault === 'survive') fs.writeFileSync(path.join(fixture.repo, source), 'survived');
      if (fault === 'mode') fs.chmodSync(target, 0o600);
      if (fault === 'hardlink') fs.linkSync(target, path.join(fixture.repo, source));
      if (fault === 'symlink') fs.symlinkSync(target, path.join(fixture.repo, source));
      if (fault === 'protected') fs.appendFileSync(path.join(fixture.repo, '.git/config'), '\n');
      const observation = observer.stop();
      expect(docsWriteFailures(observation, [DOC_PATH], { fixture, result: native.result, readOnly: fault === 'read-only' }).length).toBeGreaterThan(0);
    } finally { fixture.clean(); }
  });
});
