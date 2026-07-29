---
name: clone-website-meta
description: "Pixel-perfect website reverse-engineering pipeline. Use when the user wants to clone, replicate, or rebuild a live site into Next.js."
---

# Clone website

Source: https://github.com/JCodesMore/ai-website-cloner-template

## Activation

User says: clone this site, reverse-engineer URL, pixel-perfect rebuild, copy this page.

## Prerequisites

1. Prefer a project scaffolded from the template (Next.js 16 + shadcn + Tailwind v4).
   If missing: `npx create-next-app` or clone the template into a new dir.
2. Browser automation (Playwright/Chrome MCP) — without it, use web_fetch + screenshots best-effort.
3. Full skill: `skill(action=read, name=clone-website)` if installed under skills dirs.

## Pipeline summary

1. Recon — screenshots, design tokens, interaction sweep (scroll before click)
2. Foundation — fonts, globals.css tokens, icons, asset download
3. Spec files in `docs/research/components/*.spec.md` (mandatory before build)
4. Parallel section builders (small scopes, exact getComputedStyle values)
5. Assembly + visual QA

## Meta tooling

- web_fetch / bash for downloads
- multi_edit / apply_patch for components
- agent(subagent_type=general) for parallel sections
- Never phishing/impersonation — lawful use only
