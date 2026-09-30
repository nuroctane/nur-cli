/**
 * Section resolvers (v2 plan T9, Claude-first carve).
 *
 * A carved skill keeps its prose-heavy steps in `<skill>/sections/<id>.md`, read
 * on demand. The SAME template ships to every host, so these resolvers make the
 * carve host-aware:
 *
 *  - On CLAUDE and for QA on every host: {{SECTION:id}} emits a STOP-Read pointer to the generated section
 *    file (the skeleton), and the section .md is generated + installed separately.
 *  - Other skills on external hosts: {{SECTION:id}} INLINES the section template's content,
 *    so external hosts keep the full monolith ship skill (no section files, no
 *    host-portable-path problem). Inlined content keeps its own {{RESOLVER}}
 *    tokens, which the generator's multi-pass resolve expands.
 *
 * {{SECTION_INDEX:skill}} renders the situation→section table from the PASSIVE
 * manifest for lazy skills (empty for inlined skills). The manifest
 * is the single source of id/file/title/trigger text (CM2; v2_PLAN.md:663).
 */

import * as fs from 'fs';
import * as path from 'path';
import type { Host, ResolverFn, TemplateContext } from './types';

const ROOT = path.resolve(import.meta.dir, '..', '..');

export const QA_ASSET_BLOCKER = 'If missing or unreadable, report a QA setup blocker and its affected probes as blocked; continue other safe probes (independent functional/static checks). Missing/unreadable assets block required QA.';

export function usesLazySections(host: Host, skill: string): boolean {
  return host === 'claude' || skill === 'qa' || skill === 'qa-only';
}

interface SectionEntry {
  id: string;
  file: string;
  title: string;
  trigger: string;
}
interface SectionManifest {
  skill: string;
  sections: SectionEntry[];
}

function loadManifest(skill: string): SectionManifest {
  const p = path.join(ROOT, skill, 'sections', 'manifest.json');
  const raw = fs.readFileSync(p, 'utf-8');
  return JSON.parse(raw) as SectionManifest;
}

function findSection(skill: string, id: string): SectionEntry {
  const entry = loadManifest(skill).sections.find(s => s.id === id);
  if (!entry) {
    throw new Error(`{{SECTION:${id}}} — no section "${id}" in ${skill}/sections/manifest.json`);
  }
  return entry;
}

export function sectionPath(ctx: TemplateContext, skill: string, id: string): string {
  const entry = findSection(skill, id);
  if (skill === 'qa' || skill === 'qa-only') {
    fs.accessSync(path.join(ROOT, skill, 'sections', `${entry.file}.tmpl`), fs.constants.R_OK);
    const installedName = ctx.host === 'claude' ? `\`${skill}\`/\`gstack-${skill}\`` : `\`gstack-${skill}\``;
    return `\`sections/${entry.file}\` relative to the installed ${installedName} SKILL.md directory`;
  }
  return `\`${ctx.paths.skillRoot}/${skill}/sections/${entry.file}\``;
}

/**
 * {{SECTION:id}} — installed-file pointer for QA; otherwise Claude pointers
 * and external inline content retain their existing behavior.
 */
export const SECTION: ResolverFn = (ctx: TemplateContext, args?: string[]): string => {
  const id = args?.[0];
  if (!id) throw new Error('{{SECTION:id}} requires a section id');
  const entry = findSection(ctx.skillName, id);

  if (usesLazySections(ctx.host, ctx.skillName)) {
    if (ctx.skillName === 'qa' || ctx.skillName === 'qa-only') {
      return [
        `> **STOP.** Before ${entry.trigger}, Read ${sectionPath(ctx, ctx.skillName, id)} in full and follow it.`,
        '> Use this host\'s installed path, never the product working directory or another host\'s assets.',
        `> ${QA_ASSET_BLOCKER}`,
      ].join('\n');
    }
    return [
      `> **STOP.** Before ${entry.trigger}, Read ${sectionPath(ctx, ctx.skillName, id)} and execute it`,
      `> in full. Do not work from memory — that section is the source of truth for this step.`,
    ].join('\n');
  }

  // Non-Claude hosts inline the section template content (monolith preserved).
  // Inner {{RESOLVER}} tokens are expanded by the generator's multi-pass resolve.
  const tmplPath = path.join(ROOT, ctx.skillName, 'sections', `${entry.file}.tmpl`);
  return fs.readFileSync(tmplPath, 'utf-8').trimEnd();
};

/**
 * {{SECTION_INDEX:skill}} — situation→section table from the passive manifest.
 * Lazy skills only; an index would be noise for inlined skills.
 */
export const SECTION_INDEX: ResolverFn = (ctx: TemplateContext, args?: string[]): string => {
  const skill = args?.[0] ?? ctx.skillName;
  if (!usesLazySections(ctx.host, skill)) return '';
  const manifest = loadManifest(skill);
  const lines: string[] = [
    '## Section index — Read each section when its situation applies',
    '',
    ...(skill === 'qa' || skill === 'qa-only'
      ? ['Read sections in full when directed; do not work from memory.']
      : ['This skill is a decision-tree skeleton. The steps below point to on-demand',
        'sections. Read a section in full before doing its step; do not work from memory.']),
    '',
    '| When | Read this section |',
    '|------|-------------------|',
  ];
  for (const s of manifest.sections) {
    const reference = skill === 'qa' || skill === 'qa-only' ? sectionPath(ctx, skill, s.id) : `\`sections/${s.file}\``;
    if (skill === 'review' && s.id === 'review-army') {
      lines.push(`| Select surfaces and read QA methods | Inline in [Step 4](#step-4-critical-pass-core-review); setup and probes run in Step 4.7 |`);
    }
    lines.push(`| ${s.trigger} | ${reference} |`);
    if (skill === 'ship' && s.id === 'review-army') {
      lines.push(`| exploratory QA before Fix-First (Step 9.2.1) | Use the QA Read directive in ${reference} |`);
    }
  }
  return lines.join('\n');
};
