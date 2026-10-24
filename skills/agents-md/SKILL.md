---
name: agents-md
description: Write or improve an AGENTS.md (or CLAUDE.md / NUR.md) so coding agents work well in a repository. Use when creating, auditing or trimming agent instructions, or when agents keep repeating the same mistake in a repo.
---

# Writing AGENTS.md

An AGENTS.md is read at the start of every agent session, so every line costs tokens on every
turn and competes for attention with the task. It should hold only what an agent cannot learn
cheaply from the code, and what it would otherwise get wrong. opencode's AGENTS.md
(github.com/anomalyco/opencode) is a good model: short, imperative, specific to the repo, with
examples where a rule is easy to misread.

## What belongs in it

1. **Canonical commands.** The one command for the full check (`bun run check`, `cargo test`),
   the fast command for focused iteration, and where each must run from ("tests cannot run from
   the repo root; run them from the package directory"). Name the source of truth when two tools
   disagree.
2. **Invariants the code cannot show.** Dependency direction between packages, which files are
   generated and must never be hand-edited (and the command that regenerates them), the default
   branch, what not to add (e.g. "this repo does not use Changesets").
3. **Conventions with examples.** Branch names, commit and PR title format, and each style rule
   that people break, shown as a short Good / Bad pair rather than described.
4. **Testing rules.** What a good test looks like here, what to avoid (mocks, duplicating logic
   into the test), which skill or doc to read before writing a given kind of test.
5. **Decisions that are locked.** Settings or designs that were chosen on purpose, so an agent
   does not "fix" them back.
6. **Nested files for local rules.** A package-level AGENTS.md for rules that only matter there
   (a TUI package's theme-token rules, a schema package's codec rules). Agents load the nearest
   one, so the root stays short.

## What to leave out

- Anything the code, the README or `--help` already says. Point to the file instead.
- Generic advice every model already follows ("write clean code").
- History and rationale paragraphs. Keep the rule; move the story to an ADR.
- Rules nobody enforces. If it matters, add a check; if it does not, delete it.

## Lines worth adding to most repos

Copy the ones that fit, in your own words if you prefer:

- "Cut taglines, helper text and secondary labels. If a screen needs them to make sense, fix the
  layout." (UI repos)
- "Make the decision that improves the user experience (UX), developer experience (DX) and agent
  experience (AX) together, without breaking anything."
- "Before building unfamiliar functionality from scratch, look for actively maintained, clearly
  licensed open-source implementations; compare their approach, dependencies and known issues,
  and never copy code whose license is incompatible or origin unclear."
- "Before adding complexity for a speculative or very unlikely race or security edge case,
  explain the concrete failure mode, its likelihood and the complexity cost, and get agreement."
- "Do not extract a single-use helper preemptively; inline it unless it names a real concept or
  hides a genuinely complex boundary."
- "Validate unknown values once, at the boundary that owns them, and pass typed values inward."
- "Tolerate unknown values from outside systems (new enum members, new event types): ignore or
  pass them through instead of failing."
- "Choose theme tokens by semantic role, never by their current color; add a role rather than
  borrowing the nearest-looking one."
- "Put scratch files in a gitignored folder inside the repo (e.g. `.nur/scratch/`), not /tmp,
  and delete them when the task is done."

## Audit an existing file

1. Read it end to end and list each rule.
2. For each rule ask: does the code already show this? Is it enforced? Has an agent broken it?
3. Delete what is redundant or dead, merge duplicates, turn long explanations into one rule plus
   a pointer, and add Good / Bad examples to rules that keep being misread.
4. Move package-specific rules into nested AGENTS.md files.
5. Report what you removed and why, so the owner can object.

Sources: structure and several rules adapted from opencode's AGENTS.md files (MIT); the UX/DX/AX
and taglines lines from @Voxyz_ai; the prior-art rule from @YohoYoung6810; the scratch-folder
rule from a thread started by @peter_szilagyi about agent scratch space in /tmp.
