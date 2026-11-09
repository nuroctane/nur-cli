# Proposal: native ast-grep tool, sgrep-style

Status: proposal (2026-11-09). Author: CIA agent, from the live ast-grep
CLI reference and the repo's own sgrep tool. This supersedes the Nov 9
triage disposition of detection plus a thin skill: the user ordered a
real tool that models call regularly, just like sgrep, scoped to
ast-grep's most useful functions.

## 1. What ast-grep is

ast-grep (`ast-grep.github.io`, MIT, Rust on tree-sitter, 16.5k stars,
since 2022) is structural code search and rewrite. A pattern is written
as ordinary code with `$VAR` wildcards matching a single AST node and
`$$$` matching zero or more nodes, so a search matches syntactic
structure instead of text: it ignores formatting and skips the
string/comment hits regex reports as false positives. Install:
`cargo install ast-grep --locked`, `brew install ast-grep`,
`npm install --global @ast-grep/cli`, pip. The binary is `ast-grep`
(aliased `sg`, which on Linux is the `setgroups` utility - the tool
never invokes `sg`).

CLI surface, from the official CLI reference (checked Nov 9, 2026):

| Subcommand | What it does |
|------------|--------------|
| `run` (default) | One-shot structural search or rewrite: `-p` pattern, `-r` rewrite, `-l` lang, `--kind` AST node kind, `-A`/`-B`/`-C` context, `--json[=pretty\|stream\|compact]`, `-U`/`--update-all` to apply, `-i` interactive |
| `scan` | Rule-based lint/fix from YAML: `--rule` file, `--inline-rules` YAML text, or the project's `sgconfig.yml` when neither is given; `--json`, `-U` to apply fixes |
| `outline` | Code structure: symbols, imports, exports, members |
| `test` / `new` | Test YAML rules; scaffold a project, rule, or test |
| `lsp` | Language server for editor diagnostics |
| `completions` | Shell completions |

## 2. Current state in nur-cli

There is no structural search. The three search modes in the repo are:

- `grep`: lexical, ripgrep-backed, exact text or regex.
- `sgrep`: semantic, Context.dev binary, plain-English query to ranked
  chunks via embeddings plus BM25, with a ripgrep rerank mode.
- `glob`: file names only.

`ast-grep` appears in the clone only inside vendored skill reference
docs as "use via shell when needed", which the prompt's bash search
refusal already forbids for workspace searches. So today a model that
needs "every call shaped like X" or "every function with three
parameters" has no tool for it: grep over-matches (strings, comments,
formatting variants) and sgrep answers intent, not shape.

`sgrep` is the shape to copy. It shells out to an external binary
resolved by a dedicated module (`src/sgrep.rs`: env override
`NUR_SGREP_BIN`, otherwise the ecosystem binary finder), degrades
gracefully when the binary is missing, bounds the run with a wall-clock
budget and cancel support, and is registered as an always-on tool in
every profile. The new tool follows that shape exactly, including the
binary-resolution module, the ecosystem ensure step, and the
registration touchpoints.

## 3. The change

Add a native `ast_grep` tool. One tool, four actions, mirroring the
`monitor` tool's `action` enum shape.

### 3.1 Name

`ast_grep`. Snake case like every other tool. `sgrep` is taken by the
semantic tool, and `sg` is ambiguous (Linux `setgroups`) and too short
to read in a transcript. The description carries the upstream name so
models that know ast-grep from training recognise it.

### 3.2 Schema

```json
{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": ["search", "scan", "rewrite", "apply"],
      "default": "search",
      "description": "search: find a code shape. scan: run YAML rules. rewrite: preview a rewrite, no writes. apply: apply a rewrite after preview."
    },
    "pattern": {"type": "string", "description": "AST pattern as real code, $VAR for one node, $$$ for zero or more. Required for search, rewrite, apply."},
    "rewrite": {"type": "string", "description": "Replacement pattern using the same metavars. Required for rewrite and apply."},
    "lang": {"type": "string", "description": "Language for the pattern, e.g. rust, typescript, python. Omitted: inferred from file extension."},
    "kind": {"type": "string", "description": "Search only: AST node kind to match, e.g. function_declaration. Optional."},
    "path": {"type": "string", "description": "Directory or file to search (default cwd)."},
    "glob": {"type": "string", "description": "Include or exclude file paths, passed as --globs."},
    "rule": {"type": "string", "description": "Scan only: path to a YAML rule file."},
    "inline_rule": {"type": "string", "description": "Scan only: YAML rule text, no file needed."},
    "context": {"type": "integer", "default": 0, "description": "Lines of context around each match (max 5)."},
    "count": {"type": "integer", "default": 20, "description": "How many matches to return (max 100)."}
  },
  "required": ["action"]
}
```

Description (first sentence is what the deferred catalog and the
prompt show, so it must carry the trigger):

> Structural code search and rewrite (ast-grep, tree-sitter, local).
> Use when the target is a code shape, not text or intent: every call
> shaped like X, every function with Y parameters, every `console.log`
> to rewrite. Write the shape as real code with `$VAR` for one node and
> `$$$` for zero or more. `search` finds it, `scan` runs YAML lint
> rules, `rewrite` previews a replacement without writing, `apply`
> applies it. Prefer this over `grep` when formatting or strings and
> comments would corrupt a regex, and over `sgrep` when you know the
> shape, not just what the code does.

### 3.3 Actions and exact CLI invocations

All invocations run the resolved `ast-grep` binary (never `sg`),
inside the resolved search root, with stdout piped and a 60 s wall
clock budget (ast-grep is parallel tree-sitter; 60 s is generous),
killed on cancel, hidden window on Windows - the same run harness
shape as the sgrep tool's bounded wait. Paths are resolved through the
same sandbox resolver sgrep uses, so scans never leave the workspace.

**search** - read-only. Renders `path:line:col` then the matched text,
matches separated by a blank line, in the same shape as the sgrep
tool's render, so a model reading either tool sees one format.

```text
ast-grep run -p <pattern> [-l <lang>] [--kind <kind>] [--globs <glob>]
  [-C <context>] --json=compact [path]
```

Parse the compact JSON array. Each object carries `file`, `range`
(start/end with line and column), `text`, and metavar bindings; a
rewrite preview adds `replacement`. Verify the exact field names
against a recorded fixture from the installed binary before merging
(the CLI reference documents the flags, not the JSON schema). Unknown
fields are ignored. Matches past `count` are cut with a note naming
how to narrow (pattern, lang, path), never silently truncated.

**scan** - read-only. Runs YAML rules and reports each hit with rule
id, severity, message, `path:line:col`, matched text, and the `fix`
suggestion when the rule carries one. No rule argument means the
project's `sgconfig.yml` rules, by ast-grep's own discovery.

```text
ast-grep scan [--rule <file> | --inline-rules <yaml>] --json=compact [path]
```

**rewrite** - read-only, preview only. Same invocation as search plus
`-r <rewrite>`, and it must never pass `--update-all`. Renders each
match as old text `->` replacement. This is the step the model shows
the user or checks itself before any write happens.

```text
ast-grep run -p <pattern> -r <rewrite> [-l <lang>] --json=compact [path]
```

**apply** - mutating. Two invocations, in order, because `--json` and
`--update-all` together silently do not mutate: a single call with
both flags returns the JSON and writes nothing, which is the one
gotcha in this whole integration and the implementation must never
combine them.

```text
ast-grep run -p <pattern> -r <rewrite> [-l <lang>] --json=compact [path]  # preview
ast-grep run -p <pattern> -r <rewrite> [-l <lang>] --update-all [path]    # apply
```

The preview runs first inside the same call; zero matches returns `no
matches` and writes nothing. Over 200 matches or 50 files, the tool
refuses and says to narrow the pattern or path - no override flag in
v1. The result names files changed and replacements applied. `apply`
is mutating: it goes through the same approval path as the edit tools
(read-only classification below), is denied in plan mode, and asks in
manual mode. Undo is git, like every other edit.

### 3.4 Binary resolution and missing-binary behaviour

A new binary-resolution module mirrors the sgrep one: env override
`NUR_AST_GREP_BIN` wins (empty disables the tool, for the E2E suite);
otherwise the ecosystem binary finder, which already covers
`~/.cargo/bin`, `~/.local/bin`, and the npm global dirs. Install hint
shown when the binary is missing:

```text
cargo install ast-grep --locked  (or: brew install ast-grep / npm i -g @ast-grep/cli / nur ecosystem ensure)
```

There is no grep-style fallback: grep cannot do structural search, and
silently answering a shape query with text matches would be wrong
output, not a degraded answer. When the binary is missing the tool
returns, as its result, `[ast-grep not installed. Install: ...]` plus
the closest available tool for the query (`grep` for exact text,
`sgrep` for intent). It never errors, never crashes, and a model that
leads with `ast_grep` never wastes the call.

Ecosystem: add an `ast_grep` component to the ecosystem status,
mirroring the sgrep ensure step - when the binary is missing and cargo
is on PATH and no install has been attempted this week, run
`cargo install ast-grep --locked` bounded, with a weekly marker file
under the nur cache dir so a broken toolchain cannot stall ensure.
No model warm-up: tree-sitter parsers ship in the binary, there is
nothing to download. `nur doctor` shows the resolved path.

### 3.5 Registration: where the tool joins

`ast_grep` joins the same four lists `sgrep` sits in, because it is a
search primitive of the same rank, not an ecosystem extra. That is
what makes models call it regularly instead of discovering it once:

- The lean always-on set in `src/tools/tool_search.rs` (`LEAN_ALWAYS_ON`),
  next to `sgrep`. One more small schema in lean; the deferred catalog
  gains it automatically from the roster.
- The root core set in `src/tools/mod.rs` (`ROOT_CORE_TOOL_NAMES`),
  so TypeSafe narrowing never drops it on the first round.
- The subagent set (`SUBAGENT_TOOL_NAMES`), so children investigating
  or editing code can use it.
- The core profile set (`CORE_PROFILE_TOOL_NAMES`), the minimal local
  editing shell.

Plus the three mechanical touchpoints every tool needs in
`src/tools/mod.rs`: the module declaration, the `Box::new(...)` entry
in the boxed tools list, and the direct-match dispatch arm, plus the
sorted name in the roster test's wanted list.

Read-only classification: the tool exposes an `is_read_only_action`
function in the monitor/bg shape - `search`, `scan`, and `rewrite`
are read-only, `apply` is not. That single function is what puts
`apply` through approval and plan-mode denial while the other three
stay free.

### 3.6 Prompt text: how models learn to reach for it

The SEARCH bullet in `src/agent/prompt.rs` gains the shape sentence.
Proposed replacement for the opening of that bullet (the rest of the
bullet is unchanged):

> SEARCH - `sgrep` is the search tool for intent, `ast_grep` is the
> search tool for shape, `grep` and `glob` are their two narrow
> exceptions; the shell is not a search path. `sgrep` is semantic
> search (Context.dev sgrep, local, ripgrep-backed): describe what the
> code does in plain English and it returns the few ranked chunks that
> matter. `ast_grep` is structural search (ast-grep, tree-sitter,
> local): use it when the target is a code shape - every call shaped
> like X, every function with Y parameters, every `console.log` to
> rewrite - write the shape as real code with `$VAR` for one node and
> `$$$` for zero or more. Every search starts with one of the two:
> sgrep for intent, ast_grep for shape. `grep` is only for an exact
> identifier, string or regex you already know; `glob` is only for
> file names.

Workflow step 1 becomes: "Orient - git_status + sgrep for intent,
ast_grep for shape, grep for names, lsp for
definitions/references/symbols (load it with tool_search first in
lean), then read". The subagent prompt's focused-tools line gains
ast_grep the same way ("sgrep for intent, ast_grep for shape, grep for
names"). The plan-mode free lists gain `ast_grep` alongside `sgrep`.

Two small companion edits, no new machinery:

- The `grep` tool description gains one clause: use `ast_grep` when
  the target is a code shape, not text. No automatic grep-to-ast_grep
  routing in v1: a `$` in a regex is also a legitimate anchor, and
  auto-routing would misfire. The description is the routing.
- The bash workspace-search refusal gains `ast-grep` and `sg` as
  refused commands, naming the `ast_grep` tool - so the vendored
  skill references that say "use ast-grep via shell" stop working
  through the shell and the model gets the tool instead.

### 3.7 Docs

- `docs/tools.md`: a new `ast_grep` section immediately after the
  `sgrep` section, in the same shape (what it is, the four actions,
  arguments, the preview-then-apply rule, the missing-binary message,
  the install line). The tool-family line and the lean profile line
  gain `ast_grep`.
- No new skill in v1. sgrep has none; the tool description plus the
  docs section carry the `$VAR` / `$$$` syntax and examples. If models
  misuse patterns, a skill is the follow-up, not the launch.

## 4. Tests

- Compact-JSON parsing against a recorded fixture: file, range
  start/end, text, metavar bindings, and `replacement` on a rewrite
  preview; unknown fields ignored; garbage output is an error.
- Render: `path:line:col` plus text, blank-line separated, forward
  slashes, long matches cut on a line with a marker, mirroring the
  sgrep render tests.
- Rewrite preview renders old text `->` replacement and writes
  nothing (assert the fixture dir is untouched).
- Apply two-pass against a temp dir with the real binary installed
  (skip when the binary is absent): preview counts, apply changes the
  files, second apply is a no-op. Assert `--json` and `--update-all`
  are never combined in one invocation.
- Missing binary: returns the install message, names grep/sgrep as
  the alternatives, no error, no crash.
- `is_read_only_action`: search, scan, rewrite read-only; apply not.
- Over-cap apply (200 matches / 50 files): refuses with a narrow-the-
  pattern message.
- E2E scenario in the existing e2e shape: search finds a known
  construct in the fixture repo; rewrite preview then apply round-trip.
- `cargo check`, `cargo clippy -- -D warnings`, full test suite,
  `python3 scripts/check_docs.py` all clean.

## 5. Acceptance criteria

- [ ] `ast_grep` search finds a construct grep would miss or
      over-match (e.g. a call shape across formatting variants,
      skipping string/comment hits).
- [ ] `ast_grep` is offered on every request in lean, root, core,
      and subagent profiles, next to `sgrep`.
- [ ] Models reach for it for shape questions without being told:
      prompt text, description, and grep/bash routing all name it.
- [ ] Rewrite preview never writes; apply never combines `--json`
      with `--update-all`; apply asks in manual mode and is denied in
      plan mode.
- [ ] Binary missing: install message plus the grep/sgrep pointer,
      no crash, no silent text fallback.
- [ ] `nur ecosystem ensure` installs it with cargo when missing;
      `nur doctor` shows the resolved path.
- [ ] No new dependencies; the tool shells out to the binary exactly
      like the sgrep tool does.

## 6. Out of scope

- The `lsp` subcommand (nur-cli already has a native `lsp` tool;
  ast-grep's server is a long-lived editor process and violates the
  hang-prevention rule).
- `outline` (symbol outlines are `repo_map` / `lsp` territory;
  revisit only if a concrete gap shows up).
- `test` / `new` scaffolding (the model writes YAML rule files with
  the edit tools and validates them with `scan`).
- `completions`, `--interactive`, `--stdin` (no TTY in tool calls;
  paths cover the input).
- Custom languages (`--custom-languages` stays at its default: native
  parsers execute code and are never auto-trusted).
- An ast-grep MCP server, vendoring ast-grep as a library, or
  auto-install via npm/brew (cargo only in ensure; the rest are
  manual install hints).
- Automatic grep-to-ast_grep routing (see 3.6: `$` is also a regex
  anchor; description routing only).
- A skill (see 3.7).

## 7. Open verifications (before merging)

1. Exact `--json=compact` field names from a recorded fixture off the
   installed binary (range start/end shape, metavar bindings key).
2. `ast-grep --version` output format, for the doctor row.
3. `scan --inline-rules` accepts raw YAML text in the installed
   version.
4. Confirm the `--json` plus `--update-all` silent no-op on the
   installed version with a temp-dir test.
5. Language aliases accepted by `-l` (pass the value through; do not
   hardcode a language table that rots).
