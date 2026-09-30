import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { generatePlanCompletionGateShip } from '../scripts/resolvers/review';
import { generateTestBootstrap } from '../scripts/resolvers/testing';
import { HOST_PATHS } from '../scripts/resolvers/types';

const read = (file: string) => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');
const compact = (text: string) => text.replace(/\s+/g, ' ');
const ctx = { host: 'claude' as const, skillName: 'ship', tmplPath: '', paths: HOST_PATHS.claude };

test('ship STOP blocks advancement while retaining the stated repair route', () => {
  const entry = compact(read('ship/SKILL.md.tmpl'));
  expect(entry).toContain('STOP blocks advancement until the stated repair/resume route clears; without one, end this attempt');
  expect(entry).toContain('Answer each AskUserQuestion before continuing');
  const review = compact(read('ship/sections/review-army.md.tmpl'));
  expect(review).toContain('**Dispatched reviewer output missing:** STOP');
  expect(review).toContain('Retain queued fixes and restore coverage');
  expect(review).toContain('**Third fixing cycle reached (`CYCLES >= 3`):** STOP');
  expect(entry).toContain('Routine authorization never waives those gates or their required user decisions');
});

test('a new ship bootstrap choice overrides only the saved decline, not framework selection', () => {
  const ship = generateTestBootstrap(ctx);
  const decline = ship.slice(ship.indexOf('**If BOOTSTRAP_DECLINED**'), ship.indexOf('**If NO ecosystem marker matched:**'));
  expect(compact(decline)).toContain("Step 5's explicit Add tests choice overrides that marker for this invocation only");
  expect(compact(decline)).toContain('continue to runtime detection and B2–B3, including framework approval');
  expect(decline).not.toContain('rm ');
  expect(ship.indexOf('**If ANY existing-test evidence appears**')).toBeLessThan(ship.indexOf('**If BOOTSTRAP_DECLINED**'));
  const qa = generateTestBootstrap({ ...ctx, skillName: 'qa' });
  expect(qa).toContain('**If BOOTSTRAP_DECLINED** appears: Print "Test bootstrap previously declined — skipping." **Skip the rest of bootstrap.**');
  expect(qa).not.toContain("Step 5's explicit Add tests choice");
});

test('an unverified item answered not done enters the existing decision before continuing', () => {
  const gate = generatePlanCompletionGateShip(ctx);
  const exits = compact(gate.slice(gate.indexOf('**Exit conditions:**'), gate.indexOf('**Cap.**')));
  expect(exits).toContain('Any N: STOP and report that item as NOT DONE');
  expect(exits).toContain('Resume only after its required work is verified');
  expect(exits).toContain('no second deferral choice');
  expect(exits).not.toContain('re-running /ship');
  expect(gate).toContain('Per-item confirmation is mandatory');
  expect(gate).toContain('with the user\'s free-text evidence');
});

test('docs reentry distinguishes the initial audit from same-invocation accepted evidence', () => {
  const docs = compact(read('ship/sections/documentation.md.tmpl'));
  const entry = docs.slice(0, docs.indexOf('## Prepare the candidate'));
  expect(entry).toContain('First entry always launches the initial audit');
  expect(entry).toContain('On reentry, reuse only this invocation\'s validated audit or named-risk decision');
  expect(entry).toContain('Reentry never resets the count or authorizes a launch');
  expect(entry).toContain("this invocation's validated audit or named-risk decision");
  expect(entry).toContain('base/input hashes still match');
  expect(entry).toContain('retain its actual status and scope');
  expect(entry).toContain('Otherwise use Blocked recovery, not an unconditional launch');
  expect(docs).toContain('never a third attempt, even after Step 16 changes');
  expect(docs).toContain('never reuse an audit across invocations');
  expect(docs).toContain('Unconfirmed writers, ownership violations, unauthorized Git mutation and redaction/security gates cannot be waived');
});

test('late behavioral repairs rebuild before the docs decision and commit only remaining changes', () => {
  const entry = compact(read('ship/SKILL.md.tmpl'));
  expect(entry).toContain('A range ending at Step 14 does not enter Step 14.5');
  expect(entry).toContain('rebuild and compare again before stage 3 decides documentation freshness');
  const commit = entry.slice(entry.indexOf('### 5. Report, then push'), entry.indexOf('## Step 17:'));
  expect(commit).toContain('left uncommitted after Step 15');
  expect(commit).toContain('never create an empty commit');
  expect(commit).toContain('Preserve unrelated user files');
});

test('the title is prefixed once before the exact scanned value is published', () => {
  const body = compact(read('ship/sections/pr-body.md.tmpl'));
  expect(body).toContain("Use Step 18's `NEW_TITLE` unchanged; its version prefix is already present");
  expect(body).not.toContain('`NEW_TITLE`, prefixed with');
  expect(body).toContain('--title "$NEW_TITLE"');
  expect(body).toContain('the same scanned `NEW_TITLE`');
});
