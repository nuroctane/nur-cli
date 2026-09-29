# Startup performance investigation and implementation

The main delay was synchronous skill discovery before the first interactive
frame. A missing index took 27.87-29.40 seconds before a typed character appeared
on this installation. A valid index reduced that to 0.43-0.50 seconds.
Background ecosystem maintenance deleted the index without replacing it,
allowing the slow path to recur.

Startup now paints an editable shell first. Skills, credentials, model metadata,
and local model discovery load on workers. The current session is saved after
accepting the provider result, so an old worker cannot overwrite a new session.
Built-in controls, including login, help, effort, and workspace changes, work
during indexing. Model requests and skill commands wait visibly for their
dependencies; typing and quitting stay live.

## Measurements

Measured for v0.39.1 on Windows, the existing release profile and default features, real skill/plugin
roots exposed to an isolated writable Nur home. No live credentials were copied
and no model request was submitted in the timing runs. Maintenance was disabled
in both fixtures to avoid modifying real installations. These are missing-index
tests with normal OS caching, not reboot/cold-disk measurements.

| Time from process launch to echoed key | Original | Final local release build |
|---|---:|---:|
| Missing skill index, median of 3 | 28.249 s | 0.326 s |
| Missing index, range | 27.870-29.398 s | 0.326-0.327 s |
| Existing skill index | 0.432-0.499 s | 0.350-0.379 s, median 0.357 s over 5 |

End-to-end time includes terminal creation, process loading, input delivery,
and output collection. The final missing-index samples painted their first
frame 33-40 milliseconds after the internal timer started. An earlier sample
took 0.791 seconds end-to-end, including 0.593 seconds creating the Windows
pseudoterminal, so these measurements are observations rather than a hard
latency guarantee.

With no skill index, complete request readiness still depends on indexing.
Final rebuilds finished in 5.69 and 5.73 seconds after one 21.43-second run
overlapping other cold scans. Earlier uncontended rebuilds took 3.49 and 4.12
seconds. The editor remained responsive throughout. Final valid-index preparation
took 71-243 milliseconds internally. First catalog conversion also
takes longer than reading its compact cache.

The new global inventory exactly matched the old one: **1,599 names, all
descriptions, and all selected paths**, with no additions or removals. This
comparison includes root precedence, not just a name count.

## Implemented changes

| Investigation options | Implementation |
|---|---|
| 1, 6, 8, 15 | First paint precedes worker preparation. Queued submissions retain text, command semantics, and attachments. Provider-bound credentials resolve once for startup; failed active refresh is not immediately repeated through the same fallback. Session writes finish before dispatch. |
| 2, 5 | Invalidation writes a generation marker while retaining the last index. Builders use an OS file lease, recheck after acquiring it, publish atomically, and retry when a generation changes during discovery. Readers do not wait for the builder lease. |
| 3, 4, 9 | One traversal supplies file counts and paths. Directory-entry file types avoid redundant metadata calls. Previously visited plugin subtrees are skipped without losing sibling skills, links, depth behavior, or root precedence. |
| 10, 12 | Discovery reads frontmatter and a description fallback instead of allocating instruction bodies. Refresh reuses unchanged file metadata. An in-process snapshot avoids reparsing the global index each turn. Activation reads original files. |
| 7 | Compact model rates/context metadata is tied to the source catalog's file generation. Corrupt or outdated compact data falls back to the full catalog. Request readiness waits for selected-model context metadata. |
| 11 | Ecosystem completion and explicit plugin CLI mutations warm the replacement index. No build-time absolute-path inventory is shipped. |
| 13, 14 | The TUI reuses its loaded config and resolved client. Automatic update and ecosystem repair start after preparation reaches an idle point, at least two seconds after TUI setup. Defaults remain enabled. |
| 21, 22 | Opt-in phase timings and real pseudoterminal regression scenarios distinguish first paint, input responsiveness, filesystem work, credentials, and request readiness. |

Global freshness uses explicit generation changes, root fingerprints, and the
existing 24-hour reconciliation interval. Project roots are discovered fresh.
A nested external edit that changes neither a tracked root nor a Nur generation
can wait until reconciliation, as with the previous TTL policy; activation still
reads the current original skill body.

Options **16 and 17 were excluded**: no history replay optimization, Cargo
profile change, feature removal, PGO, or LTO tuning. Option 18 would remove skills
and option 19 would weaken freshness or repair, so those alternatives conflict
with preserving functionality. No resident daemon, antivirus exclusion, or host
configuration change was introduced. The measurements support solving this in
the existing process and cache lifecycle; system-wide tracing remains available
if a particular host shows unexplained residual latency.

## Validation

- All five final real-terminal scenarios passed. They hold model lookup, credential refresh, and the skill
  builder lease behind controlled gates. They verify editable drafts, queued
  slash activation, project overrides, exactly-once requests, initialization
  ordering, and quitting during a stall. A failed-refresh case checks API-key
  fallback without repeating the failed refresh.
- All 14 active headless scenarios passed: tool calls, file changes, parallel
  reads, permission modes, provider errors, malformed calls, result spilling,
  and hooks. The existing secret-redaction product decision remains skipped.
- 182 focused Rust tests passed, covering cache concurrency/corruption, atomic
  replacement, junction/depth behavior, authentication, pricing/context metadata,
  skills, queues, and the modules touched by lint cleanup.
- Strict Clippy passes with warnings denied. Small existing lint findings
  encountered during verification were also corrected. Turn budgets, history
  replay, integration defaults, and skill content are unchanged.

Run `python tests/e2e/run_startup.py --bin target/release/nur.exe` for terminal
checks. Artifacts are under `target/startup-e2e/`; headless results are under
`target/e2e/`. Local timing traces and inventory snapshots are under
`.nur/startup-investigation/`. Use `NUR_STARTUP_TRACE=1` for ongoing diagnosis,
as described in [troubleshooting](troubleshooting.md).

These changes are included in **v0.39.1**. The measurements above were collected
before the release version bump, using the same implementation, release profile,
and default features. Release binaries and checksums are available on
[GitHub Releases](https://github.com/nuroctane/nur-cli/releases/tag/v0.39.1).
