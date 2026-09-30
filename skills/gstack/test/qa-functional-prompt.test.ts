import { expect, test } from 'bun:test';
import { qaFunctionalPrompt, QA_FUNCTIONAL_CASES } from './helpers/qa-functional-eval';
import { qaCommandAllowed } from './helpers/qa-functional-observer';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { parseNDJSON } from './helpers/session-runner';
import { qaFunctionalVerdict, qaNativeProbes } from './helpers/qa-functional-evidence';
import { createQAFunctionalFixture, ownedPath } from './helpers/qa-functional-fixture';
import { validateQACheckpoints } from './helpers/qa-checkpoint-evidence';
import { computePaidCaseSelection } from '../scripts/test-paid-shards';

test.each(['full', 'pr'] as const)('%s selection assigns the captured webhook regression to its native owner', profile => {
  for (const file of ['qa-webhook-r85-checkpoints.json', 'qa-functional-ci-36505065023.json']) {
    const result = computePaidCaseSelection({ profile, env: {},
      changedFiles: [`test/fixtures/${file}`] });
    expect(result.selection).toEqual({ e2e: ['qa-functional-webhook-report'], judges: [] });
    if (profile === 'pr') {
      expect(result.coverage?.mode).toBe('pr');
      expect(result.coverage?.unknownFiles).toEqual([]);
    }
  }
});

test.each(['full', 'pr'] as const)('%s selection assigns the captured CLI learning regression to its native owner', profile => {
  const result = computePaidCaseSelection({ profile, env: {},
    changedFiles: ['test/fixtures/qa-functional-cli-learning-ci-36516246523.json'] });
  expect(result.selection).toEqual({ e2e: ['qa-functional-cli-report'], judges: [] });
});

test('CI CLI replay summaries fail only the distinct-probe metric despite valid native exploration', () => {
  const captures = JSON.parse(readFileSync(join(import.meta.dir, 'fixtures/qa-functional-cli-learning-ci-36516246523.json'), 'utf8'));
  expect(captures.attempts).toHaveLength(2);
  for (const original of captures.attempts) {
    const fixture = createQAFunctionalFixture('cli');
    try {
      const captured = JSON.parse(JSON.stringify(original).replaceAll(original.fixtureRoot, fixture.root));
      fixture.revision = captured.report.revision;
      captured.report.runtime = `bun ${Bun.version}`;
      for (const [name, content] of Object.entries(captured.reports)) writeFileSync(ownedPath(fixture.root, `qa-reports/${name}`), content as string);
      const parsed = parseNDJSON(captured.publicEvents.map(event => JSON.stringify(event)));
      const result = { ...parsed, output: '', exitReason: captured.exitReason, browseErrors: [], duration: 0,
        firstResponseMs: 0, maxInterTurnMs: 0, model: 'native-event-replay',
        costEstimate: { inputChars: 0, outputChars: 0, estimatedTokens: 0, estimatedCost: 0, turnsUsed: 0 } };
      const read = parsed.toolCalls.find(call => call.tool === 'Read' && call.input.file_path.endsWith('/qa/sections/system-functional.md'))!;
      const section = { path: 'qa/sections/system-functional.md', content: read.output.replace(/^\s*\d+(?:→|\t)/gm, '').trim() };
      const verdict = (report = captured.report, transcript = captured.publicEvents, observation = captured.observation) =>
        qaFunctionalVerdict(fixture, 'qa-only', { ...result, transcript }, observation, report, section, captured.reports['report.md']);
      expect(verdict()).toEqual(['missing observation-to-next-hypothesis evidence']);
      expect(captured.report.learning[0].observationCommand).toBe(captured.report.learning[0].nextCommand);
      const noteCall = parsed.toolCalls.find(call => call.tool === 'Write' && basename(call.input.file_path).startsWith('exploration-')
        && JSON.parse(call.input.content).observationCommand !== JSON.parse(call.input.content).nextCommand)!;
      const { observationCommand, nextCommand, hypothesis } = JSON.parse(noteCall.input.content);
      const learning = { observationCommand, nextCommand, hypothesis };
      const report = { ...captured.report, learning: [learning] };
      expect(verdict(report)).toEqual([]);
      for (const invalid of [[], [{ ...learning, nextCommand: observationCommand }],
        [{ ...learning, nextCommand: 'bun run probe -- apply uncaptured 11' }],
        [{ ...learning, nextCommand: 'bun test' }],
        [{ ...learning, nextCommand: `${observationCommand}; ${nextCommand}` }],
        [{ ...learning, observationCommand: nextCommand, nextCommand: observationCommand }],
        [{ ...learning, hypothesis: 'Try another probe.' }]]) {
        expect(verdict({ ...report, learning: invalid })).toContain('missing observation-to-next-hypothesis evidence');
      }
      const noteId = captured.publicEvents.flatMap(event => event.message.content)
        .find(block => block.type === 'tool_use' && block.name === 'Write' && block.input.file_path === noteCall.input.file_path).id;
      const pending = captured.publicEvents.filter(event => !event.message.content.some(block => block.type === 'tool_result' && block.tool_use_id === noteId));
      expect(pending.length).toBeLessThan(captured.publicEvents.length);
      expect(verdict(report, pending).some(failure => failure.includes('checkpoint'))).toBe(true);
      expect(verdict(report, captured.publicEvents, { ...captured.observation, complete: false })).toContain('incomplete write observation');
      const noteFile = join(fixture.root, 'qa-reports', basename(noteCall.input.file_path));
      const note = JSON.parse(readFileSync(noteFile, 'utf8'));
      note.observed.stdout = 'invented output';
      writeFileSync(noteFile, JSON.stringify(note));
      expect(verdict(report).some(failure => failure.includes('checkpoint'))).toBe(true);
    } finally { fixture.cleanup(); }
  }
});

test('functional driver discloses its learning, CLI coverage and repair acceptance requirements', () => {
  for (const entry of QA_FUNCTIONAL_CASES) {
    const prompt = qaFunctionalPrompt(entry);
    expect(prompt).toContain(`Read ${entry.mode}/SKILL.md, qa/sections/scope.md, ${entry.mode}/sections/exploratory.md and qa/sections/system-functional.md in full`);
    expect(prompt).toContain('All four reads are required before probing in this fixture, even when its surfaces and isolation are already established');
    expect(prompt).toContain('command is the exact full outer capture invocation, including that ID and all wrapper options, not just the native child command after --');
    expect(prompt).toContain('"command":"<exact full outer capture invocation>"');
    expect(prompt).not.toContain('<exact executed native probe command>');
    expect(prompt).toContain('different later command');
    expect(prompt).toContain('not the required same-command replay');
    expect(prompt).toContain('Select that checkpoint ID in annotations.learning');
    expect(prompt).toContain('the production helper copies its observationCommand, hypothesis and nextCommand');
    expect(prompt).toContain('different native child commands');
    expect(prompt).toContain('Only the helper writes observed fields');
    expect(prompt).toContain('English, more than 20 characters');
    if (entry.family === 'cli') expect(prompt).toContain('a successful apply; balance alone is not enough');
    if (entry.mode === 'qa') {
      expect(prompt).toContain(`repair only src/${entry.family === 'cli' ? 'cli' : 'worker'}.ts`);
      expect(prompt).toContain('existing tests remain read-only');
      expect(prompt).toContain('Freeze all test files after red');
    }
  }
});

test('CI native checkpoint keeps public fixture paths exact and requires the completed Write', () => {
  const captured = JSON.parse(readFileSync(join(import.meta.dir, 'fixtures/qa-functional-ci-36505065023.json'), 'utf8'));
  const reportRoot = realpathSync(mkdtempSync(join(tmpdir(), 'qa-ci-note-')));
  try {
    for (const variant of ['captured redaction', 'exact public JSON', 'omitted path', 'renamed identity', 'pending Write', 'late Write']) {
      const originalRoot = dirname(captured.checkpointEvents[2].message.content[0].input.file_path);
      const events = JSON.parse(JSON.stringify(captured.checkpointEvents).replaceAll(originalRoot, reportRoot));
      const parsed = parseNDJSON(events.map(event => JSON.stringify(event)));
      const probes = qaNativeProbes(parsed);
      expect(probes.map(probe => probe.command)).toEqual(['bun run probe -- duplicate', 'bun run probe -- partial']);
      const write = events[2].message.content[0].input;
      const note = JSON.parse(write.content);
      expect(note.observed.stateRoot).toContain('/qa-state-redacted/');
      expect(probes[0].observed.stateRoot).toContain('/qaf-QXv0tB/.qa-state/');
      if (variant !== 'captured redaction') note.observed = structuredClone(probes[0].observed);
      if (variant === 'omitted path') delete note.observed.stateRoot;
      if (variant === 'renamed identity') {
        note.observed.fixture = note.observed.stateRoot;
        delete note.observed.stateRoot;
      }
      write.content = JSON.stringify(note);
      writeFileSync(write.file_path, write.content);
      if (variant === 'pending Write') events.splice(3, 1);
      if (variant === 'late Write') events.push(...events.splice(3, 1));
      const errors = validateQACheckpoints({ transcript: events, reportRoot, probes,
        requiredProbes: probes.slice(1), files: { 'exploration-002.json': write.content },
        reportMarkdown: '[Checkpoint](exploration-002.json)' });
      if (variant === 'exact public JSON') expect(errors).toEqual([]);
      else expect(errors).toContain('QA checkpoint: Missing unique completed checkpoint before probe: bun run probe -- partial');
    }
  } finally { rmSync(reportRoot, { recursive: true, force: true }); }
});

test('CI native exploration Read does not stand in for completed method Reads', () => {
  const captured = JSON.parse(readFileSync(join(import.meta.dir, 'fixtures/qa-functional-ci-36505065023.json'), 'utf8'));
  const fixture = createQAFunctionalFixture('webhook');
  try {
    for (const variant of ['omitted methods', 'pending methods', 'completed methods']) {
      const events = structuredClone(captured.omittedReadEvents);
      if (variant !== 'omitted methods') events.push(...captured.completedMethodEvents.filter(event => variant === 'completed methods' || event.type === 'assistant'));
      const parsed = parseNDJSON(events.map(event => JSON.stringify(event)));
      expect(parsed.toolCalls.some(call => call.tool === 'Read' && call.input.file_path.endsWith('/qa-only/sections/exploratory.md') && call.output.includes('# Shared exploratory QA'))).toBe(true);
      const errors = qaFunctionalVerdict(fixture, 'qa-only', {
        ...parsed, output: '', exitReason: 'success', browseErrors: [], duration: 0,
        firstResponseMs: 0, maxInterTurnMs: 0, model: 'native-event-replay',
        costEstimate: { inputChars: 0, outputChars: 0, estimatedTokens: 0, estimatedCost: 0, turnsUsed: 0 },
      }, { complete: true, failures: [], events: [], changed: [], before: {}, after: {}, limits: [] }, {},
      { path: 'qa/sections/system-functional.md', content: readFileSync(join(import.meta.dir, '../qa/sections/system-functional.md'), 'utf8') });
      expect(errors.includes('no completed functional instruction read')).toBe(variant !== 'completed methods');
    }
  } finally { fixture.cleanup(); }
});

test('the native launcher consumes the family-specific actor boundary', () => {
  const source = readFileSync(join(import.meta.dir, 'helpers/qa-functional-eval.ts'), 'utf8');
  expect(source).toContain('prompt: qaFunctionalPrompt(entry)');
  for (const entry of QA_FUNCTIONAL_CASES) {
    const prompt = qaFunctionalPrompt(entry);
    expect(prompt).toContain(`Read ${entry.mode}/SKILL.md`);
    expect(prompt).toContain(entry.mode === 'qa' ? 'Full exploration and the Standard fix tier' : 'Full report-only exploration');
    expect(prompt).not.toContain('at Standard depth');
    expect(prompt).toContain('successful checkpoint publication before the next probe');
    expect(prompt).toContain('no shell composition, scripts or added path operands');
    expect(prompt).toContain('ONLY complete JSON actually emitted');
    expect(prompt).toContain('never a combined command list');
    expect(prompt).toContain('Put tests, raw CLI diagnostics, launch failures and timeouts in Markdown');
    expect(prompt).toContain(entry.family === 'cli'
      ? 'The generic wrapper does NOT support wait'
      : 'bun cancel.ts is a CLI-only entrypoint, not part of this fixture');
    expect(prompt).not.toContain('parseInt');
    if (entry.family === 'webhook') {
      expect(prompt).toContain('All eight scenarios are required coverage; a replay does not replace another scenario');
      expect(prompt).toContain('Choose their order from observations after the happy path');
    } else {
      expect(prompt).not.toContain('All eight scenarios');
    }
  }
});

test('declared examples respect the existing closed native grammar', () => {
  for (const command of ['pwd', 'ls', 'ls -la', 'git status --short', 'git status --porcelain',
    'git branch --show-current', 'git diff', 'git diff --stat', 'git rev-parse HEAD', 'bun --version',
    'date -u +%Y-%m-%dT%H:%M:%SZ', 'bun test', 'bun test test/contract.test.ts',
    'bun run probe -- balance', 'bun run probe -- export', 'bun run probe -- apply id 7',
    'bun run probe -- apply', 'bun cancel.ts', ...['happy', 'reject', 'duplicate', 'partial',
      'concurrent-ab', 'concurrent-ba', 'cancel', 'dependency'].map(name => `bun run probe -- ${name}`)]) {
    expect(qaCommandAllowed(command)).toBe(true);
  }
  for (const command of ['ls -la .qa-state qa-reports', 'ls -la .qa-state', 'ls -la qa-reports', 'bun run probe -- wait bad 7',
    'git rev-parse HEAD; bun --version', 'bun run probe -- happy && bun run probe -- partial']) {
    expect(qaCommandAllowed(command)).toBe(false);
  }
});

test('artifact completion preserves exact evidence before concise linked reporting', () => {
  for (const entry of QA_FUNCTIONAL_CASES) {
    const prompt = qaFunctionalPrompt(entry);
    expect(prompt).toContain('Materialize qa-reports/evidence.json first, then write a concise qa-reports/report.md');
    expect(prompt).toContain('using the functional report structure');
    expect(prompt).toContain('Link the evidence and checkpoint files rather than repeating full probe payloads in Markdown');
    expect(prompt).toContain('Both artifacts are required before completion');
    expect(prompt).toContain('The learning array is a summary: choose one completed checkpoint');
    expect(prompt).toContain('not another probe or a duplicate of the complete checkpoint ledger');
    expect(prompt).toContain('Both commands must name exact captured probes with different native child commands');
    expect(prompt).toContain('Preserve every checkpoint and link every checkpoint in Markdown');
    expect(prompt).toContain('keep every executed probe and its complete JSON in evidence');
    expect(prompt).toContain('Evidence rows contain ONLY complete JSON actually emitted by native probes, including failures and repeats');
    expect(prompt).toContain('retain pre-repair results alongside green results');
    expect(prompt).toContain('Never synthesize JSON');
    expect(prompt).toContain('one causal sentence per checkpoint hypothesis (English, more than 20 characters) and compact JSON formatting, preserving every field and value');
    expect(prompt).toContain('retain its headings and required fields');
    expect(prompt).toContain('link to evidence.json and checkpoints for details already recorded there');
    expect(prompt).toContain('After saving both artifacts, return only their paths and the actual completion status');
    expect(prompt).toContain('Never shorten native JSON or omit a required probe, check or field');
    expect(prompt).toContain('aim under 400 words');
  }
  const source = readFileSync(join(import.meta.dir, 'helpers/qa-functional-eval.ts'), 'utf8');
  expect(source).toContain('maxTurns: 40');
  expect(source).toContain('completionReserveMs: timeout / 4');
});

test('fix completion budgets for required repair and avoids duplicating preserved evidence', () => {
  for (const entry of QA_FUNCTIONAL_CASES) {
    const prompt = qaFunctionalPrompt(entry);
    if (entry.mode === 'qa') {
      expect(prompt).toContain('a reproduced in-tier defect requires the authorized native regression, repair and verification');
      expect(prompt).toContain('retain its headings and required fields');
      expect(prompt).toContain('link to evidence.json and checkpoints for details already recorded there');
      expect(prompt).toContain('Include the diagnosis, red/green test results and coverage limits');
      expect(prompt).toContain('After saving both artifacts, return only their paths and the actual completion status');
      expect(prompt).toContain('Never shorten native JSON or omit a required probe, check or field');
      const stages = ['1. Prove the regression red', '2. On the repaired source', '3. Save the evidence and Markdown artifacts'];
      const positions = stages.map(stage => prompt.indexOf(stage));
      expect(positions.every(position => position >= 0)).toBe(true);
      expect(positions).toEqual([...positions].sort((a, b) => a - b));
      expect(prompt).toContain('A green test suite does not substitute for these native probes');
      expect(prompt).toContain('not a signal to stop stage 2');
      expect(prompt).toContain('report incomplete; do not call it complete with a caveat');
      expect(prompt).toContain('one causal sentence per checkpoint hypothesis (English, more than 20 characters) and compact JSON formatting, preserving every field and value');
    } else {
      expect(prompt).not.toContain('This is a fix run');
      expect(prompt).toContain('Include the diagnosis, proposed test stubs and coverage limits');
      expect(prompt).not.toContain('Include the diagnosis, red/green test results');
    }
  }
});

test('webhook fix stage retains the required scenarios omitted by both R29 captures', () => {
  const captured = [
    { id: 'ecd6da06-abd0-4299-8c33-e1b99a672325', scenarios: ['happy', 'partial', 'partial', 'concurrent-ab', 'partial', 'cancel', 'dependency', 'happy'], missing: ['reject', 'duplicate', 'concurrent-ba'] },
    { id: '45722f13-a72c-4c01-87cc-8e17285ef8c4', scenarios: ['happy', 'concurrent-ab', 'concurrent-ab', 'concurrent-ab', 'happy', 'cancel', 'dependency'], missing: ['reject', 'duplicate', 'partial', 'concurrent-ba'] },
  ];
  const prompt = qaFunctionalPrompt({ family: 'webhook', mode: 'qa' });
  const verification = prompt.slice(prompt.indexOf('2. On the repaired source'), prompt.indexOf('3. Save the evidence'));
  const required = ['happy', 'reject', 'duplicate', 'partial', 'concurrent-ab', 'concurrent-ba', 'cancel', 'dependency'];
  for (const attempt of captured) {
    expect(required.filter(scenario => !attempt.scenarios.includes(scenario))).toEqual(attempt.missing);
    for (const scenario of required) expect(verification).toContain(`\`${scenario}\``);
  }
  expect(verification).toContain('every still-unobserved scenario');
  expect(verification).toContain('recheck earlier scenarios affected by the repair');
  expect(verification).toContain('None of these scenarios is optional exploration');
  expect(prompt).toContain('the completion reserve does not end required coverage');
  expect(qaFunctionalPrompt({ family: 'cli', mode: 'qa' })).not.toContain('`concurrent-ba`');
});

test('fix-stage checkpoint provenance survives intervening native regression tests', () => {
  const prompt = qaFunctionalPrompt({ family: 'webhook', mode: 'qa' });
  expect(prompt).toContain('most recent completed native probe');
  expect(prompt).toContain('Tests, source edits and clock reads do not replace that observation');
  expect(prompt).toContain('put red/green test output in the report, not in observed');
  expect(prompt).toContain('write no checkpoint when there is no next probe');
});

test('R29 captured webhook bytes bind across a green test; test summaries and altered JSON do not', () => {
  const captured = {
    before: '{"scenario":"concurrent-ab","requests":[{"method":"POST","path":"/events","auth":"$QA_SYNTHETIC_AUTH","body":{"id":"delivery","cents":7},"status":202,"response":"{\\"accepted\\":\\"delivery\\"}"}],"order":["a","b"],"interrupted":"","state":{"jobs":{"delivery":{"cents":7,"status":"complete","attempts":2}},"effects":[{"id":"delivery","cents":7},{"id":"delivery","cents":7}]},"stateRoot":"/q/gstack-paid-shard-hyrPGY/tmp/qaf-JaNRb2/.qa-state/concurrent-ab-nVrq2v"}',
    after: '{"scenario":"concurrent-ab","requests":[{"method":"POST","path":"/events","auth":"$QA_SYNTHETIC_AUTH","body":{"id":"delivery","cents":7},"status":202,"response":"{\\"accepted\\":\\"delivery\\"}"}],"order":["a","b"],"interrupted":"","state":{"jobs":{"delivery":{"cents":7,"status":"complete","attempts":1}},"effects":[{"id":"delivery","cents":7}]},"stateRoot":"/q/gstack-paid-shard-hyrPGY/tmp/qaf-JaNRb2/.qa-state/concurrent-ab-ySExJy"}',
    green: 'bun test v1.4.0 (34cbb9a40)\n\n 2 pass\n 0 fail\n 4 expect() calls\nRan 2 tests across 2 files. [50.00ms]',
    checkpoint: '{"observationCommand":"bun test test/worker.regression-1.test.ts","observed":"red before repair: expect(received).toEqual(expected) — effects had two {id:delivery,cents:7} entries; 0 pass 1 fail. After the src/worker.ts post-gate recheck, bun test reported 2 pass 0 fail (native test output, not probe JSON).","hypothesis":"The regression turned green after the post-gate ledger recheck, so the original failing native probe should now show exactly one effect with both workers still released in a then b order.","nextCommand":"bun run probe -- concurrent-ab"}\n',
  };
  const reportRoot = realpathSync(mkdtempSync(join(tmpdir(), 'qa-r29-')));
  const name = 'exploration-004.json';
  const file = join(reportRoot, name);
  const command = 'bun run probe -- concurrent-ab';
  try {
    for (const variant of ['original summary', 'native JSON', 'raw test output', 'missing stateRoot', 'green result', 'missing Write receipt', 'terminal note']) {
      const note = JSON.parse(captured.checkpoint);
      if (variant !== 'original summary') {
        note.observationCommand = command;
        note.observed = JSON.parse(captured.before);
      }
      if (variant === 'raw test output') { note.observationCommand = 'bun test'; note.observed = captured.green; }
      if (variant === 'missing stateRoot') delete note.observed.stateRoot;
      if (variant === 'green result') note.observed = JSON.parse(captured.after);
      if (variant === 'terminal note') note.nextCommand = 'none';
      const content = variant === 'original summary' ? captured.checkpoint : JSON.stringify(note);
      writeFileSync(file, content, { mode: 0o600 });
      const calls = [
        { tool: 'Bash', input: { command }, output: `$ bun probe.ts concurrent-ab\n${captured.before}` },
        { tool: 'Bash', input: { command: 'bun test' }, output: captured.green },
        { tool: 'Write', input: { file_path: file, content }, output: `File created successfully at: ${file}` },
        { tool: 'Bash', input: { command }, output: `$ bun probe.ts concurrent-ab\n${captured.after}` },
      ];
      const packets = calls.flatMap((call, index) => [
        { type: 'assistant', message: { content: [{ type: 'tool_use', id: `r29-${index}`, name: call.tool, input: call.input }] } },
        ...variant === 'missing Write receipt' && call.tool === 'Write' ? [] : [
          { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: `r29-${index}`, content: call.output }] } },
        ],
      ]);
      const result = parseNDJSON(packets.map(packet => JSON.stringify(packet)));
      const probes = qaNativeProbes(result);
      expect(probes).toHaveLength(2);
      const failures = validateQACheckpoints({ transcript: result.transcript, reportRoot, probes,
        requiredProbes: probes.slice(1), files: { [name]: content }, reportMarkdown: `[Checkpoint](${name})` });
      if (variant === 'native JSON') expect(failures).toEqual([]);
      else expect(failures).toContain(`QA checkpoint: Missing unique completed checkpoint before probe: ${command}`);
      if (variant === 'original summary') expect(failures).toContain(`QA checkpoint: Unrelated, reused or retrospective checkpoint: ${name}`);
    }
  } finally { rmSync(reportRoot, { recursive: true, force: true }); }
});

test.each(JSON.parse(readFileSync(join(import.meta.dir, 'fixtures/qa-webhook-r85-checkpoints.json'), 'utf8')))(
  'R85 $attempt rejects a published draft even after a corrected successor', capture => {
    for (const variant of ['captured pair', 'complete note only', 'draft only', 'missing Write receipt', 'missing report link', 'partial observation']) {
      const reportRoot = realpathSync(mkdtempSync(join(tmpdir(), 'qa-r85-')));
      try {
        const packets = structuredClone(capture.transcript);
        if (variant === 'draft only') packets.splice(4, 2);
        else if (variant !== 'captured pair') packets.splice(2, 2);
        if (variant === 'missing Write receipt') packets.splice(3, 1);
        const files: Record<string, string> = {};
        for (const packet of packets) {
          for (const block of packet.message.content) {
            if (block.type !== 'tool_use' || block.name !== 'Write') continue;
            const name = basename(block.input.file_path);
            block.input.file_path = join(reportRoot, name);
            if (variant === 'partial observation') {
              const note = JSON.parse(block.input.content);
              delete note.observed.state;
              block.input.content = JSON.stringify(note);
            }
            files[name] = block.input.content;
            writeFileSync(block.input.file_path, block.input.content);
          }
        }
        const result = parseNDJSON(packets.map((packet: unknown) => JSON.stringify(packet)));
        const probes = qaNativeProbes(result);
        expect(probes).toHaveLength(2);
        const failures = validateQACheckpoints({ transcript: result.transcript, reportRoot, probes,
          requiredProbes: probes.slice(1), files,
          reportMarkdown: variant === 'missing report link' ? '' : Object.keys(files).map(name => `[Checkpoint](${name})`).join('\n') });
        if (variant === 'complete note only') expect(failures).toEqual([]);
        else if (variant === 'captured pair' || variant === 'draft only') {
          expect(failures).toContain(`QA checkpoint: ${capture.bad === 'exploration-003.json' ? 'Invalid checkpoint schema' : 'Unrelated, reused or retrospective checkpoint'}: ${capture.bad}`);
        } else if (variant === 'missing report link') {
          expect(failures).toContain(`QA checkpoint: Report does not link checkpoint: ${capture.good}`);
        } else {
          expect(failures).toContain(`QA checkpoint: Missing unique completed checkpoint before probe: ${probes[1].command}`);
        }
      } finally { rmSync(reportRoot, { recursive: true, force: true }); }
    }
  },
);

test('report-only exploration requires a completed written checkpoint before the next probe', () => {
  const section = readFileSync(join(import.meta.dir, '../qa-only/sections/exploratory.md'), 'utf8');
  const positions = ['1. First demonstrate success', '2. **Decide whether another probe is needed.**', '**Publish before probing.**', '3. Run that exact probe; G enforces the deadline when bounded']
    .map(marker => section.indexOf(marker));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  expect(section).toContain('exploration-NNN.json');
  expect(section).toContain("Reuse resolved REPORT_DIR");
  expect(section).toContain('own a fresh');
  expect(section).toContain('owned probe directory');
  for (const field of ['observationCommand', 'observed', 'hypothesis', 'nextCommand']) expect(section).toContain(`${field}:`);
  expect(section).toContain('Wait for successful checkpoint publication');
  expect(section).toContain('Never backfill or overwrite notes');
  expect(section).toContain('link each checkpoint');
  expect(section).not.toContain('a separate assistant text message');
});

test('surface evidence checks defer to one exploratory execution sequence', () => {
  const source = readFileSync(join(import.meta.dir, '../scripts/resolvers/qa.ts'), 'utf8');
  expect(source).toContain('Each probe is one native command/interaction plus checks, excluding bookkeeping');
  const positions = ['2. **Decide whether another probe is needed.**', '**Publish before probing.**', '3. Run that exact probe; G enforces the deadline when bounded']
    .map(marker => source.indexOf(marker));
  expect(positions.every(position => position >= 0)).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  expect(source).toContain('Never batch probes');
  expect(source).toContain('Follow the shared exploratory loop\'s order and written checkpoints');
  expect(source).toContain('Replay the exact failing command/request from the same initial fixture state');
  expect(source).toContain('Another input or a regression test is not that replay');
});

test('all public callers directly require the functional method before exploration', () => {
  for (const skill of ['qa', 'qa-only', 'review', 'ship']) {
    const file = skill === 'ship' ? 'ship/sections/review-army.md' : `${skill}/SKILL.md`;
    const source = readFileSync(join(import.meta.dir, '..', file), 'utf8');
    expect(source).toContain(['review', 'ship'].includes(skill) ? '../qa/sections/exploratory.md' : 'sections/exploratory.md');
    expect(source).not.toMatch(/Functional surfaces[^\n]*\n[^\n]*Read[^\n]*system-functional\.md/);
    const explorer = readFileSync(join(import.meta.dir, '..', skill === 'qa-only' ? 'qa-only' : 'qa', 'sections/exploratory.md'), 'utf8');
    expect(explorer).toMatch(/Functional surfaces[^\n]*\n[^\n]*Read[^\n]*system-functional\.md/);
    expect(explorer).toContain('Browser surfaces only');
    const stages = ['Read `sections/scope.md`', 'in full and select the surfaces',
      'Read `sections/system-functional.md`', 'Write a **charter**', '1. First demonstrate success'];
    const positions = stages.map(stage => explorer.indexOf(stage));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a, b) => a - b));
    const functional = readFileSync(join(import.meta.dir, '../qa/sections/system-functional.md'), 'utf8');
    expect(functional).toContain('## Contract map');
    expect(functional).toContain("Follow the shared exploratory loop's order and written checkpoints");
  }
});
