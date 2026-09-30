import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { readWorkflowJudgeInput } from './helpers/workflow-judge-input';

const root = resolve(import.meta.dir, '..');
const compact = (text: string) => text.replace(/\s+/g, ' ').trim();
const sources = [
  {
    label: 'templates',
    main: readFileSync(resolve(root, 'plan-ceo-review/SKILL.md.tmpl'), 'utf8'),
    section: readFileSync(resolve(root, 'plan-ceo-review/sections/review-sections.md.tmpl'), 'utf8'),
  },
  {
    label: 'actual judge bundle',
    ...(() => {
      const input = readWorkflowJudgeInput({
        root,
        skillPath: 'plan-ceo-review/SKILL.md',
        startMarker: '## Step 0: Nuclear Scope Challenge',
        endMarker: '## Review Sections',
      });
      return {
        main: input.files.find(file => file.kind === 'entrypoint')!.content,
        section: input.files.find(file => file.path === 'plan-ceo-review/sections/review-sections.md')!.content,
      };
    })(),
  },
];

for (const source of sources) describe(`CEO clarity routing — ${source.label}`, () => {
  const main = compact(source.main);
  const section = compact(source.section);
  const decisions = main.split('### 0D.')[1]!.split('### 0E.')[0]!;
  const mode = main.split('### 0E.')[1]!.split('### 0F.')[0]!;

  test('pending proposals stay out of approved work and settled means exact authority', () => {
    expect(main).toContain('**Required choice:** unanswered. Resolve a choice only when continuing would change scope, hide a blocker or produce the wrong output');
    expect(main).toContain('**Pending:** unapproved; keep in Proposed, not tasks or accepted work');
    expect(main).toContain('Status is `unresolved` or `reopened`');
    expect(main).toContain('**Settled:** an answer, direct instruction or authorized auto-decision resolves this exact choice and scope; a recommendation does not');
  });

  test('admin menus return locally while prescribed scope menus still require the full approval cycle', () => {
    expect(decisions).toContain('Skip steps 1–4; this approves no plan changes. Resume that menu\'s next step');
    expect(decisions).toContain('0E owns mode selection; 0H owns document approval');
    expect(decisions).toContain('If an admin answer requests a plan change, use the Plan decision route for that change before resuming');
    expect(decisions).toContain('0G proposals and section findings use this route even with prescribed menus');
    expect(decisions).toContain('0D returns to its caller, not to mode selection');
    expect(decisions).toContain('For mode changes, follow 0E\'s **Mode change** instruction');
    expect(decisions).not.toContain('0D never restarts mode selection');
    expect(decisions).toContain('**Pre-question checkpoint:**');
    expect(decisions).toContain('**STOP for the actual answer, even for a lone option.**');
    expect(decisions).toContain('**Post-answer checkpoint:**');
  });

  test('a mode change waits for authority and resumes without discarding earlier answers', () => {
    const change = mode.split('**Mode change:**')[1]!.split('Selecting a mode')[0]!;
    expect(change).toContain('Pause and ask with the four-mode menu; keep the mode until answered');
    expect(change).toContain('repeat the handoff/provenance record');
    expect(change).toContain('complete newly applicable Step 0 work in route order, reusing completed work and scope answers');
    expect(change).toContain('Then resume the paused step. If unchanged, resume directly');
    expect(mode).toContain('Selecting a mode does not approve changes');
    expect(mode).toContain('| SCOPE EXPANSION / SELECTIVE EXPANSION | 0F → 0G → 0H (including its spec review loop) → 0I |');
    expect(mode).toContain('| HOLD SCOPE | 0G → 0I |');
    expect(mode).toContain('| SCOPE REDUCTION | 0G |');
  });

  test('scope limits count reused deliverables but mode recommendations count only changed files', () => {
    expect(main).toContain('Count all deliverables, including reused code, against scope limits');
    expect(main).toContain('0E counts changed files, excluding unchanged reuse, to recommend a mode');
    expect(main).toContain('Neither count approves changes');
    expect(mode).toContain('For >15 planned changed files, recommend SCOPE REDUCTION');
    const routes = ['For >15 planned changed files', 'If categories overlap or are unclear', 'Otherwise: a new product/system'];
    const positions = routes.map(route => mode.indexOf(route));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a, b) => a - b));
    expect(main).toContain('more than 8 files or more than 2 new classes/services');
  });

  test('coverage and kind-only scoring retain approvals and define the completion fraction', () => {
    expect(decisions).toContain('**Same work, different coverage:**');
    expect(decisions).toContain('10 = all edge cases, 7 = happy path, 3 = shortcut');
    expect(decisions).toContain('mode selection and Add/Defer/Skip or Defer/Keep');
    expect(decisions).toContain('No score does not waive approval checkpoints');
    expect(section).toContain('Select answered questions scored for coverage under 0D that offered a 10/10 option');
    expect(section).toContain('Exclude unscored mode/scope choices and unanswered questions');
    expect(section).toContain('Count a reopened choice only once, using its latest answered option');
    expect(section).toContain('Y is the number of eligible questions; X is how many selected the 10/10 option');
    expect(section).toContain('Report X/Y, or `N/A` when Y is zero');
  });

  test('section findings check reopening evidence before reusing prior answers', () => {
    const gate = section.split('**Resolve.**')[1]!.split('**Apply.**')[0]!;
    const paths = ['1. This section needs a new choice', '2. An exact prior answer covers it', '3. A non-blocking choice belongs to a later section'];
    const positions = paths.map(path => gate.indexOf(path));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a, b) => a - b));
    expect(gate).toContain('0D\'s Plan decision route through its post-answer save');
    expect(gate).toContain('Resolve critical risks now');
    expect(gate).toContain('No path selects the mode again');
    expect(source.section.match(/\*\*Decision gate\.\*\*/g)).toHaveLength(11);
  });

  test('Section 1 publishes current dispositions, not a second initial mode handoff', () => {
    const opening = section.split('### Section 1: Architecture Review')[1]!.split('Evaluate and diagram:')[0]!;
    expect(opening).toContain('Publish **Current scope** in chat before the architecture analysis');
    expect(opening).toContain('Retain 0E\'s selected mode, rationale and preference attribution');
    expect(opening).toContain('each governing row\'s ID, disposition and answer reference');
    expect(opening).toContain('including scope decisions after 0E');
    expect(opening).toContain('Distinguish accepted, deferred, rejected and pending work');
    expect(opening).toContain('This is a scope update, not another mode handoff; do not ask or log the mode again');
    expect(opening).not.toContain('using the Step 0E mode-handoff format');
  });
});
