---
name: skill-authoring
description: Author a nur-cli skill the way a model actually follows one: a portable directory with a SKILL.md entry point, reference files, executable lint, an acceptance profile, and an eval that scores whether a model followed it. Use when asked to create, fix, or review a skill, or to make a skill enforceable instead of advisory.
---

# Skill authoring for nur-cli

A skill that only describes a workflow will be skimmed and ignored. A skill
that a model follows has five parts, and this skill is the template for all
five. Steal the packaging pattern, not any one skill's content: the content
of a skill must fit the harness it runs in, which for nur-cli means Rust,
its tool roster, and its prompt, not another author's stack.

## The five parts

1. **Entry point.** One `SKILL.md` with frontmatter: `name` (kebab-case, one
   word or hyphenated, matching the directory name) and `description` that
   says what the skill does *and when to use it*. The description is what
   triggers activation, so a description without a trigger is a skill that
   never fires. Keep the entry point short: what, when, and the method in
   order. Long detail lives in references, not in the entry point.
2. **References.** `references/*.md` for detail a model needs only sometimes:
   recipes, checklists, edge cases, examples. Name them by job, not by
   number, and link them from the entry point only where the job needs them.
   A reference the entry point never points to is dead weight.
3. **Executable lint.** `scripts/` with a script that checks the skill
   itself and, where the skill is about code, checks the code it governs.
   A lint that only a human runs is a wish. Run it in the skill's own
   method, and run it again after any edit to the skill. See
   `scripts/lint_skill.py` in this directory for the minimum checks a
   nur-cli skill must pass.
4. **Acceptance profile.** A short, checkable list of what "done" means:
   what the model must do, what it must never do, and what evidence it must
   leave (a file, a command output, a ranked list with file:line evidence).
   If acceptance cannot be checked from the output alone, it is not an
   acceptance profile, it is a hope.
5. **Eval.** One scenario that scores whether a model followed the skill,
   not whether it produced plausible text. The eval in this directory is a
   checklist, not a benchmark harness: give the model a task that tempts it
   to skip the skill's hard step, then check whether the hard step happened
   and left evidence. Record the result with the skill, so a later edit
   cannot silently make the skill weaker.

## Rules that apply to every nur-cli skill

- No secrets, tokens, passwords, or absolute user paths in a skill file.
  Credentials go through the `secret` tool (masked modal, `$NAME` in shell),
  never through a skill's text.
- No tool the roster does not offer. If a skill needs a deferred tool, say
  so explicitly: load it with `tool_search` first, then call it by its real
  name. Never invent a tool call whose schema is not available.
- No second allowlist. If a skill restricts what a model may do, it must
  point at the harness rule that already enforces it (capabilities,
  permissions, plan-mode gating), not invent a parallel rule that will drift.
- A skill changes nothing until the user picks items, unless the user asked
  for the change. An audit skill reports ranked findings; a fix skill fixes
  only what the user named.
- Test the skill the way its eval says, after every edit, and keep the
  eval result with the skill. A skill whose eval was never run is a draft.

## Method: authoring a new skill

1. Name the job and the trigger first, in one sentence each. If the trigger
   is vague ("when writing code"), the skill will fire at the wrong times
   or never. Rewrite until the trigger names a concrete user request or
   artifact.
2. Write the entry point, then the acceptance profile, then the references.
   If the acceptance profile cannot be written before the references, the
   job is not understood yet.
3. Write the lint script, then run it against the skill itself. Fix what it
   flags before asking anyone to use the skill.
4. Run the eval: a task that tempts the model to skip the hard step.
   Check the evidence, not the prose. If the model skipped the step, the
   skill is wrong, not the model: make the step earlier, shorter, or
   checkable, and re-run.
5. Add the skill to `skills/bundled.json` (name, source directory, files)
   so it ships in the skill pack, and run `python scripts/build_skill_pack.py
   --check` and `python scripts/generate_skill_intents.py --check`. A skill
   that is not in the pack is not shipped, however good its text is.

## Eval checklist (run this, do not just read it)

- [ ] The skill fires on its trigger and does not fire on a nearby
  non-trigger (e.g. a skill about audits does not fire on "fix this bug").
- [ ] The model follows the acceptance profile and leaves the evidence it
  names, without being reminded mid-task.
- [ ] The lint script passes on the skill directory itself.
- [ ] No secret, token, or absolute user path appears in any skill file.
- [ ] The skill is in `skills/bundled.json` and the pack check passes.
