import { expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { runGeneration } from '../scripts/gen-skill-docs';
import { ALL_HOST_NAMES, getHostConfig } from '../hosts';

test('every host exposes the DX per-call rule before the pre-review audit and Step 0', async () => {
  const outputRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'devex-rule-free-'));
  try {
    const generated = await runGeneration({ host: 'all', outputRoot });
    expect(generated.exitCode).toBe(0);
    const skills = generated.artifacts.filter(artifact => artifact.kind === 'skill'
      && /(?:^|\/)gstack-plan-devex-review\/SKILL\.md$|^plan-devex-review\/SKILL\.md$/.test(artifact.relativePath));
    expect(skills.map(artifact => artifact.host).sort()).toEqual([...ALL_HOST_NAMES].sort());
    for (const artifact of skills) {
      const content = fs.readFileSync(path.join(outputRoot, artifact.relativePath), 'utf8');
      const audit = content.indexOf('## PRE-REVIEW SYSTEM AUDIT');
      expect(audit).toBeGreaterThan(0);
      const preReview = content.slice(audit, content.indexOf('## Auto-Detect Product Type', audit));
      expect(preReview).toContain('origin/<detected-base-branch>...HEAD');
      expect(preReview).not.toContain('git merge-base HEAD main');
      expect(preReview).toContain('Defer exhaustive branch exploration until after product type and persona are confirmed.');
      const productGate = content.slice(content.indexOf('## Auto-Detect Product Type', audit),
        content.indexOf('## Step 0: DX Investigation', audit));
      expect(productGate).toContain('STOP. Ask for product-type confirmation before deeper branch research.');
      const brain = content.indexOf('## Brain Context (preflight)', audit);
      const productType = content.indexOf('## Auto-Detect Product Type', audit);
      const persona = content.indexOf('### 0A. Developer Persona Interrogation', productType);
      const personaStop = content.indexOf('**STOP.** Do NOT proceed until user responds.', persona);
      const prerequisite = content.indexOf('## Prerequisite Skill Offer', persona);
      expect(brain).toBeGreaterThan(audit);
      expect(brain).toBeLessThan(productType);
      expect(prerequisite).toBeGreaterThan(personaStop);
      expect(prerequisite).toBeLessThan(content.indexOf('### 0B. Empathy Narrative', persona));
      const beforeAudit = content.slice(0, audit);
      expect(beforeAudit).toContain('including Step 0 and outside voice');
      expect(beforeAudit).toContain('One independent choice per AskUserQuestion call, never separate tabs');
      // Claude loads the review section later; these evidence limits must also
      // govern Step 0's first journey questions on every host.
      const earlyEvidence = beforeAudit.replace(/\s+/g, ' ');
      expect(earlyEvidence).toContain('A description of what a reporter includes does not establish its exact words');
      expect(earlyEvidence).toContain('Confirmation of an empathy narrative is not runtime observation');
      // The retained DX run reopened an approved CLI/library entrypoint after
      // its outside prompt reduced the mode to "no new APIs". These are source
      // propagation checks; the native eval still owns behavioral acceptance.
      expect(earlyEvidence).toContain('selected option, answer reference and approved scope');
      expect(earlyEvidence).toContain("user's task boundaries and requested mode, amended only by exact approved exceptions");
      expect(earlyEvidence).toContain("A mode's default does not cancel an explicitly approved exception");
      // Required factual/navigation repair is review work; it must not become
      // an approval solely because the rating pass finds a gap. New approaches
      // and policies still go through the same gate.
      const classification = earlyEvidence.slice(earlyEvidence.indexOf('2. **Classify the finding.**'),
        earlyEvidence.indexOf('3. **Check the scope.**'));
      expect(classification).toContain('verifying sources, correcting facts and restoring docs or navigation');
      expect(classification).toContain('unverified behavior or destinations stay unknown');
      expect(classification).toContain('A new presentation approach, guarantee, channel, scope extension or optional verification depth remains a decision');
      const rating = content.slice(content.indexOf('## The 0-10 Rating Method'));
      expect(rating).toContain('4. Run the Decision gate for each gap.');
      expect(rating).not.toContain('Resolve each new in-scope gap via AskUserQuestion');
      const journey = content.slice(content.indexOf('### 0F.'), content.indexOf('### 0G.'));
      const wholeGate = journey.indexOf('Run all four Decision gate steps');
      expect(wholeGate).toBeGreaterThanOrEqual(0);
      expect(wholeGate).toBeLessThan(journey.indexOf('> "Journey Stage: INSTALL'));
      expect(journey).not.toContain('AskUserQuestion per friction point');
      const mode = content.slice(content.indexOf('### 0E. Mode Selection'), content.indexOf('Context-dependent defaults:'));
      expect(mode).toContain('Use the mode the user explicitly requested for this review.');
      expect(mode).toContain('skip the mode question and continue to 0F. Otherwise, ask below.');
      const sectionText = generated.artifacts.filter(item => item.kind === 'section'
        && item.host === artifact.host && item.relativePath.startsWith('plan-devex-review/'))
        .map(item => fs.readFileSync(path.join(outputRoot, item.relativePath), 'utf8')).join('\n');
      const allContent = content + '\n' + sectionText;
      expect(allContent).toContain('if viable, split them before asking');
      expect(allContent).toContain('A code example and an optional checklist are separate choices');
      expect(allContent).toContain('as are a timer and its release-gate policy');
      const gate = beforeAudit.indexOf('### Decision gate');
      const ground = beforeAudit.indexOf('1. **Ground the evidence.**', gate);
      const classify = beforeAudit.indexOf('2. **Classify the finding.**', gate);
      const scope = beforeAudit.indexOf('3. **Check the scope.**', gate);
      const options = beforeAudit.indexOf('4. **Draft and answer one decision.**', gate);
      expect([gate, ground, classify, scope, options].every((offset, i, offsets) =>
        offset >= 0 && (i === 0 || offset > offsets[i - 1]!))).toBe(true);
      const localRule = allContent.slice(allContent.indexOf('## CRITICAL RULE — How to ask questions'),
        allContent.indexOf('## Required Outputs', allContent.indexOf('## CRITICAL RULE — How to ask questions')));
      expect(localRule).toContain('Run the Decision gate before drafting options.');
      expect(localRule).not.toContain('use AskUserQuestion for each gap');
      expect(allContent).toContain('Record observed human onboarding separately from automated execution');
      expect(allContent).toContain('a warm snippet timer is neither a fresh-start check nor a human benchmark');
      expect(allContent).toContain('Keep estimates labeled until measured');
      const benchmark = content.slice(content.indexOf('### 0C. Competitive DX Benchmarking'),
        content.indexOf('### 0D. Magical Moment Design'));
      expect(benchmark.indexOf('Define the clock before comparing')).toBeGreaterThanOrEqual(0);
      expect(benchmark.indexOf('Define the clock before comparing')).toBeLessThan(benchmark.indexOf('AskUserQuestion:'));
      expect(benchmark).toContain('Compare times only across equivalent boundaries');
      expect(benchmark).toContain('Never infer no wait from a peer');
      expect(benchmark).toContain('Start → result');
      expect(benchmark).toContain('Carry the approved clock and target into 0D, Pass 1, Pass 8 and the report');
      expect(allContent).toContain('Measure the approved 0C clock and 0D useful result');
      expect(allContent).not.toContain('**STOP.** AskUserQuestion once per issue.');
      expect(allContent).toContain('A target tier does not itself approve telemetry, an automated');
      expect(allContent).toContain('only when proposing them, with ownership and frequency explicit');
      expect(allContent).toContain('Continue the same working list from Step 0');
      expect(allContent).toContain('compare its evidence with the prior decision and options already considered');
      expect(allContent).toContain('A disclosed tradeoff or rejected alternative is not new evidence merely because a reviewer prefers it');
      expect(allContent).toContain('identify a concrete contradiction or changed assumption before reopening');
      expect(allContent).toContain('Unverified loss of existing coverage remains a risk to verify');
      expect(allContent).toContain('not proof that a new release policy is needed');
      expect(allContent).toContain('If a necessary remedy crosses an explicit scope boundary, name that boundary');
      expect(allContent).toContain('obtain scope approval before choosing or applying the remedy');
      expect(allContent).toContain('Implementation details and proof of one chosen behavior\nstay together; independent policies each need their own decision');
      if (!getHostConfig(artifact.host!).suppressedResolvers.includes('CODEX_PLAN_REVIEW')) {
        const outside = allContent.slice(allContent.indexOf('## Outside Voice — Independent Plan Challenge'));
        const context = outside.indexOf('REVIEW CONTEXT (from the full working list, outside the truncated plan body)');
        const planBody = outside.indexOf('THE PLAN:\n<plan content>');
        expect(context).toBeGreaterThan(0);
        expect(planBody).toBeGreaterThan(context);
        expect(outside.slice(context, planBody)).toContain('selected option, answer reference and exact scope');
        expect(outside.slice(context, planBody)).toContain('including any explicitly approved exception');
        expect(outside.slice(context, planBody)).toContain('Missing implementation remains a verification');
        expect(outside.slice(context, planBody)).toContain('concrete new evidence or a changed assumption');
        expect(outside).toContain("Apply the Decision gate's distinction between routine review work and a new choice.");
      } else {
        expect(allContent).not.toContain('REVIEW CONTEXT (from the full working list, outside the truncated plan body)');
      }

    }
  } finally { fs.rmSync(outputRoot, { recursive: true, force: true }); }
}, 20_000);
