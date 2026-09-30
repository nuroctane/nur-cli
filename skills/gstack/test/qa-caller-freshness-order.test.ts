import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ALL_HOST_CONFIGS } from '../hosts';
import { RESOLVERS } from '../scripts/resolvers';
import { HOST_PATHS, type TemplateContext } from '../scripts/resolvers/types';

const root = join(import.meta.dir, '..');
const compact = (text: string) => text.replace(/\s+/g, ' ');

function render(file: string, ctx: TemplateContext): string {
  let text = readFileSync(join(root, file), 'utf8');
  for (let pass = 0; pass < 10; pass++) {
    const next = text.replace(/\{\{([A-Z_]+)(?::([^}]+))?\}\}/g, (_match, name, args) => {
      if (!RESOLVERS[name]) throw new Error(`Unknown resolver ${name}`);
      return RESOLVERS[name](ctx, args?.split(':'));
    });
    if (next === text) return compact(text);
    text = next;
  }
  throw new Error(`Unresolved template ${file}`);
}

function ordered(text: string, markers: string[]): void {
  let previous = -1;
  for (const marker of markers) {
    const index = text.indexOf(marker);
    expect(index, marker).toBeGreaterThan(previous);
    previous = index;
  }
}

describe('review and ship completion freshness contracts', () => {
  for (const host of ALL_HOST_CONFIGS) {
    for (const skillName of ['review', 'ship']) {
      const ctx: TemplateContext = { host: host.name, skillName, tmplPath: '', paths: HOST_PATHS[host.name] };
      const body = compact(RESOLVERS.QA_REVIEW(ctx));
      const shared = compact(RESOLVERS.QA_EXPLORATORY({ ...ctx, skillName: 'qa' }));
      const gate = body.slice(body.indexOf('**4. Check freshness before reporting.**'), body.indexOf('Return verified defects'));

      test(`${host.name}/${skillName}: dependent probes await prerequisites without serializing independent Reads`, () => {
        expect(shared).toContain('Complete these Reads in order before writing charters or probing');
        expect(shared).toContain('Wait for successful checkpoint publication before dispatch');
        expect(body).toContain('Await clock/guard results before acting');
        expect(body).toContain('Batch only independent Reads');
        expect(shared).toContain('Missing or unreadable assets, prerequisites or permission block affected probes, not independent safe checks');
        ordered(body, ['Batch only independent Reads', '**1.', '**3. Run smoke and plan checks']);
      });

      test(`${host.name}/${skillName}: normal and skipped paths resolve freshness before completion`, () => {
        expect(gate).toContain('Before every completion report or log');
        expect(gate).toContain('even with zero fixes or skipped specialists');
        ordered(gate, ['a. Read agent/user updates', 'await results without batching them with reporting/logging',
          "b. Compare each probe's recorded", 'c. Re-review', 'd. Compare again after revalidation',
          'Report clean/completed only when all required checks pass on current inputs']);
        expect(gate).toContain('even without updates');
      });

      test(`${host.name}/${skillName}: late changes preserve current per-probe evidence`, () => {
        expect(gate).toContain("Compare each probe's recorded source, tests, contracts, commands and fixtures (or input fingerprint) with current inputs");
        expect(gate).toContain('Never rerun valid current passes');
        expect(gate).toContain('Re-review changed or uncertain coverage and repeat step 3 for affected checks');
        expect(gate).toContain('Compare again after revalidation or edits/updates');
      });

      test(`${host.name}/${skillName}: required revalidation uses real limits rather than the optional-work reserve`, () => {
        expect(gate).toContain('repeat step 3 for affected checks');
        expect(shared).toContain('return to step 2 for each affected revalidation');
        expect(shared).toContain('Keep limits/notes; status requires fresh evidence');
        expect(gate).toContain('Reporting reserves cannot stop required revalidation within the caller\'s deadline');
        expect(body).toContain('Await clock/guard results before acting');
        expect(body).toContain('Smoke: 5 minutes/12 probes');
        expect(body).toContain('Then run required plan checks, even after smoke expires');
        expect(body).toContain('no smoke guard; never reset the clock');
        expect(body).toContain('Use finite command timeouts, capped at the caller\'s remaining time if it has a deadline');
      });

      test(`${host.name}/${skillName}: unavailable freshness or insufficient time cannot certify completion`, () => {
        expect(gate).toContain('Failed or unavailable Reads or insufficient time block affected required checks');
        expect(gate).toContain('List failed, blocked, inconclusive and not-run checks');
        expect(gate).toContain('Report clean/completed only when all required checks pass on current inputs; optional untested ideas do not block it');
        expect(body).toContain('Only the parent runs report-only discovery');
        expect(body).toContain('Test creation needs user approval');
        expect(body).toContain('Setup/permission blockers are not defects');
        expect(body).toContain(skillName === 'review'
          ? 'Unresolved coverage makes Step 5.8 incomplete; a ship waiver cannot complete it'
          : 'explicit named-risk acceptance; otherwise blocked');
      });
    }

    test(`${host.name}/ship: finalization consumes the freshness result before summaries and persistence`, () => {
      const ctx: TemplateContext = { host: host.name, skillName: 'ship', tmplPath: '', paths: HOST_PATHS[host.name] };
      const body = render('ship/sections/review-army.md.tmpl', ctx);
      const finalization = body.slice(body.indexOf('4. **Finish and log'), body.indexOf('5. Output summary:'));
      expect(finalization).toContain('Recheck freshness (Step 9.2.1) before items 5–6');
      expect(body).toContain('even with zero fixes or skipped specialists');
      expect(body).toContain('Report clean/completed only when all required checks pass on current inputs');
      ordered(body, ['Recheck freshness (Step 9.2.1) before items 5–6', '5. Output summary:', '6. Persist the review result']);
      expect(body).toContain('Complete items 5–6 exactly once with the original REVIEW_START');
      expect(body).toContain('Missing dispatched output uses `status:"unavailable"`, `completed:false` and `converged:false`');
      expect(body).toContain('Failed, blocked, inconclusive or not-run required probes mean false, never clean');
      expect(body).toContain('Undispatched host-unsupported/gated specialists do not block');
    });
  }
});
