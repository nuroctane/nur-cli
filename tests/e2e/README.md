# E2E suite

Black-box tests for the real `nur` binary. Each scenario runs `nur run` in a
throwaway nur home and workspace against `fake_provider.py`, a scripted
OpenAI-compatible server, and checks what a user can observe: files on disk,
stdout and stderr, the exit code, and what nur actually sent the model.

```bash
cargo build --bin nur
python tests/e2e/run_e2e.py --bin target/debug/nur.exe
```

Name scenarios to run a subset: `python tests/e2e/run_e2e.py 02_write_read_edit`.
Every run writes `target/e2e/report.md`, plus per-scenario `stdout.txt`,
`stderr.txt`, `requests.jsonl` and `report.json`.

The interactive startup suite drives a real pseudoterminal and reconstructs its
screen. It holds model lookup, credential refresh, and the skill-builder lease
behind controlled gates, then verifies typing, queued skill activation with
project overrides, exactly-once submission, durable initialization, and quitting.
It also checks that `/effort` saves its level and that a relaunch starts on it.
Built-in controls are exercised while indexing is blocked, and a model change
during discovery must win over the older worker result.

```bash
python -m pip install pyte
# Windows uses the native ConPTY API (Windows 10 1809 or later).
python tests/e2e/run_startup.py --bin target/release/nur.exe
```

Artifacts are under `target/startup-e2e/`. Optional scenario names:
`blocked-models`, `queued-skill`, `quit-during-startup`, `blocked-auth`,
`failed-auth-fallback`, `effort-persists`, `commands-during-indexing`,
`controls-during-models`, `compact-during-indexing`, `banner-animates`.

## Scenarios

One JSON file per scenario in `scenarios/`:

| Key | Meaning |
|-----|---------|
| `prompt`, `mode` | the `nur run` prompt; `auto` (default), `plan` or `manual` |
| `provider`, `model`, `env` | run as a catalog provider (its wire format, pointed at the fake server) with this model; extra environment such as the provider's key variable |
| `script` | provider replies in order: `{"text"}`, `{"tool_calls": [{"name", "arguments"}]}` (string arguments are sent verbatim), or `{"status", "error"}`. On the Responses wire a reply may add `reasoning` (an encrypted reasoning item) and `extra_output` (raw output items) |
| `setup_files` | files created in the workspace first |
| `setup_home_files` | files created in the nur home (`hooks.toml`, …); `{home}` and `{workspace}` expand |
| `setup_commands` | `nur` subcommands run in the workspace before the prompt, e.g. `[["permissions", "trust"]]` |
| `expect` | `exit_code`, `stdout_contains`/`stdout_lacks`, `stderr_contains`, `files` (exact text or `contains`/`lacks`), `absent_files`, `requests`, `tool_results` (per request), `requests_lack`, `max_tool_result_chars`, `absent_request_fields` (dotted paths such as `reasoning.summary`), `input_item_types` and `content_part_types` (the only types any request may send) |
| `skip` | the reason a scenario is parked, for open product decisions |

A run also fails if a process nur started is still holding files in the
scenario's home when it exits.

## Testing rules

- New user-visible behavior gets a scenario. A bug fix starts with a scenario,
  or the narrowest test, that reproduces it.
- Do not add unit tests that restate the code: no asserting that a constant,
  tool description or prompt contains a substring, no `include_str!` of the
  crate's own source, no zero-assertion tests that spawn real machine probes.
  Previews and benchmarks are `#[ignore]`.
- Isolated tests are for what E2E cannot reach cheaply (parsers, wire formats,
  credential stores, security classification, concurrency). List the ways it
  can fail first, then write one test per failure.
- Tests never write to the real nur home: `cargo test` does not set `NUR_HOME`.
- On Windows `dirs::home_dir()` ignores `HOME`/`USERPROFILE`, so a scenario
  still sees the real `~/.agents`, `~/.claude` and `~/.optmem`. Scenarios must
  not depend on their contents.
