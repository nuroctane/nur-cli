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

## Scenarios

One JSON file per scenario in `scenarios/`:

| Key | Meaning |
|-----|---------|
| `prompt`, `mode` | the `nur run` prompt; `auto` (default), `plan` or `manual` |
| `script` | provider replies in order: `{"text"}`, `{"tool_calls": [{"name", "arguments"}]}` (string arguments are sent verbatim), or `{"status", "error"}` |
| `setup_files` | files created in the workspace first |
| `setup_home_files` | files created in the nur home (`hooks.toml`, …); `{home}` and `{workspace}` expand |
| `expect` | `exit_code`, `stdout_contains`/`stdout_lacks`, `stderr_contains`, `files` (exact text or `contains`/`lacks`), `absent_files`, `requests`, `tool_results` (per request), `requests_lack`, `max_tool_result_chars` |
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

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
