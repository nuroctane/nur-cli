---
name: codex
description: Run or resume the OpenAI Codex CLI for code analysis, reviews, refactoring, or authorized edits. Preserve the user's configured model and reasoning effort unless they request an override.
---

# Codex CLI

Check the installed surface with `codex --version`, `codex exec --help`, and
`codex exec resume --help`. The authoritative reference is
[OpenAI's CLI documentation](https://developers.openai.com/codex/cli/reference).
Use local help when a flag differs between installed and documented versions.

## Run and resume

For an authorized review, use `codex exec --sandbox read-only "<task>"`.
For authorized repository edits, use `codex exec --sandbox workspace-write "<task>"`.
Pass `-C <directory>` when selecting a workspace. Pass `--skip-git-repo-check`
only when deliberately operating outside a Git repository.

The prompt can be supplied through stdin with `codex exec -`; use the shell's
literal multiline input syntax so quotes, backticks, and dollar signs stay literal.
Use `codex exec resume <session-id> "<follow-up>"` to continue a known session,
or `codex exec resume --last "<follow-up>"` when the user selected the most recent
session. Consult the resume subcommand's help for supported overrides.

Inherit the installed CLI's model and reasoning configuration. When the user
explicitly chooses a supported model, pass `--model <model>`; when they choose a
supported reasoning effort, pass `-c model_reasoning_effort="<effort>"`.
Do not substitute guessed model names, prices, context limits, or benchmark scores.

## Execution and reporting

Follow the user's existing authorization and the harness's permission policy.
Keep stdout and stderr available so authentication failures, interrupted runs,
and incomplete work remain visible. When structured output is needed, use the
installed `--json` stream and verify completion and the exit status.

Handle recoverable failures within the authorized task. Preserve useful partial
work and report concrete unresolved errors. Ask only for missing information or
authorization that is actually required. Delegating to another agent requires
authorization from the current harness and user; this skill does not grant it.
