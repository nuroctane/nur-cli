# CLI and skills refresh - v0.41.0

This release combines startup review fixes with a dependency and skill-source
refresh. Login, account changes, built-in commands, typing and quitting remain
responsive while workers prepare credentials and skills.

## Rust stack

All direct dependencies were checked against their primary registry. Cargo.lock
records exact resolved versions. These upgrades required compatibility changes:

| Area | Updated stack | Compatibility work |
|---|---|---|
| HTTP | [reqwest 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/) | Explicit form, query, HTTP/2, system proxy, streaming and Rustls features retain OAuth and provider behavior. |
| Terminal | [ratatui 0.30.2](https://github.com/ratatui/ratatui/blob/main/BREAKING-CHANGES.md), crossterm 0.29, tui-markdown 0.3.10, tui-scrollview 0.6.8 | Shared terminal types upgrade together. Finished Markdown rendering survives width changes; animations remain enabled. |
| Images | [ratatui-image 11.1.0](https://github.com/ratatui/ratatui-image/blob/master/CHANGELOG.md) | Updated picker/font API; terminal and tmux detection runs after first paint on a worker. Kitty outer-terminal precedence and forced protocols remain supported. |
| Documents | anydoc 0.2.4 | Default document conversion remains enabled. |
| Configuration | toml 1.1.6, dirs 7, thiserror 2 | Existing configuration stays compatible. Durable writes atomically replace the old file on Windows. |
| Credentials | aes-gcm 0.11.1, base64 0.23.1, sha2 0.11 | Updated nonce/digest APIs retain the stored credential formats. |
| Windows | windows-sys 0.61.2 | Native URI opening, process identity checks, restricted child handles and atomic replacement use Windows APIs. |

The release profile and history replay remain unchanged, as requested. Default
image/document features and every integration remain available.

An OSV scan of all 529 locked registry packages found three informational
maintenance advisories and no reported vulnerability advisories. Each current
upstream release still includes the affected crate, and none has a patched
version. These dependency migrations remain open:

| Dependency path | Advisory | Migration and required compatibility checks |
|---|---|---|
| tui-markdown -> syntect -> bincode 1.3.3 | [Bincode maintenance](https://rustsec.org/advisories/RUSTSEC-2025-0141.html) | Adopt syntect's replacement serializer or a reviewed maintained fork. Verify embedded syntax/theme dump compatibility and highlighting across every theme before switching. |
| tui-markdown -> syntect -> yaml-rust 0.4.5 | [YAML parser maintenance](https://rustsec.org/advisories/RUSTSEC-2024-0320.html) | Move syntect to yaml-rust2, with syntax-definition parsing and highlighting fixtures. Upstream's default feature selection currently brings in yaml-rust. |
| anydoc -> pdf-inspector -> ttf-parser 0.25.1 | [Font parser maintenance](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | Move pdf-inspector to skrifa/fontations. Compare font metrics, character mapping and extraction on embedded-font PDFs, including malformed fonts. |

Removing highlighting or PDF support would change the harness's functions.
Changing these transitive APIs requires upstream migrations or maintained forks,
beyond updating their currently published versions. The advisory evidence is
retained in [the dependency scan](maintenance/advisories-2026-09-30.json).

## Skills and references

The refresh fetched 77 repositories and records full commit IDs, source paths
and resource hashes in `skills/upstream-lock.json`. Complete trees include
scripts, examples and reference material. First-party SCA remains separate.
Portable text hashes tolerate checkout line endings; the snapshot records
upstream executable modes. CI verifies hashes, tracked resource completeness
and those modes so Git ignore rules cannot silently omit a reference or script.
The check also hashes staged Git objects to catch clean filters that alter
vendored resources. Duplicate-name precedence is identical on Windows and Unix.

The deterministic index contains 1,710 unique skill names. Folder aliases retain
commands such as `/craft` and `/neon` when upstream changes canonical names. Four
startup skills previously represented only by metadata now have complete bundled
trees. No skill was removed.

Six trees received individual review: banner-design, codex, craft, design,
design-system and frontend-design. Five retain Nur adaptations. The Codex guide
now follows installed CLI help; unsupported model claims, fabricated prices and
benchmarks, and stderr suppression were removed.

The audit covers repository links in Rust, docs and vendored Markdown, including
raw download URLs. Moved repositories resolve to their current owners. Broken
operational links were repaired against owning projects, including CryptoGuard,
cryptoskills, Atomic Operator, ShellBags tooling and sbsigntools. Example URLs and
GitHub login/attachment endpoints are classified separately.

The [JSON evidence](maintenance/stack-2026-09-30.json) records a dated registry and
source snapshot. Later upstream releases can differ.

## Repeatable maintenance

Installed Node/Bun/Python companions update through their package managers,
preserving account and workspace state. Compatibility floors are OMP 18.4.4 with
Bun 1.3.14, and Ruflo 3.48.0. Explicit ecosystem refreshes update installed
components instead of only filling gaps.

[OptMem](https://github.com/VictorTaelin/OptMem) remains upstream-pure under
`~/.optmem`. Nur verifies a reviewed commit's SHA256 before replacing its script,
backs up explicit replacements and never touches the memory tree. Fractal stays
Unix-only with no WSL bridge.

```text
python scripts/audit_stack.py
python scripts/sync_upstream_skills.py --offline
python scripts/sync_upstream_skills.py --offline --apply
python scripts/generate_skill_intents.py
python scripts/generate_skill_intents.py --check
python scripts/check_skill_snapshot.py
python tests/test_skill_maintenance.py
nur ecosystem refresh-skills
```

`scripts/install_skill_snapshot.py` plans installed updates by default. Applying
requires a backup directory. It replaces only files matching a managed hash or
an explicit Git baseline, retains local edits, preserves complete resources and
rejects destination escapes. Runtime mirrors also record ownership for refreshes.

For this update, `--refresh-vendored` reapplies the reviewed upstream-owned trees
as complete units, with backups of differing installed files. The separately
reviewed Codex guide is also refreshed. Unmanaged Nur adaptations and local
additions are retained. Installed copies received 16,338 file additions and
1,037 file updates across the Nur and universal agent roots.

The installed companions now include OMP 18.4.5, Graphify 0.9.73, Headroom 0.39.1,
PLUR 0.20.1, Ruflo 3.48.0, Penecho 1.3.5, egaki 0.11.0, AKM 0.9.18, GraphJin
3.20.78 and skills 1.7.0. The older native OMP installation that shadowed Bun's
copy was updated as well. Node 24.14.1 and Bun 1.3.14 satisfy the checked engines.

## Local validation

Both default and no-default-feature builds pass Clippy with warnings denied.
The final Rust suite passes 1,068 tests with 18 intentionally ignored previews
and benchmarks. Nine maintenance-tool tests cover ownership, backups, complete
resources, destination escapes, portable hashes, executable modes, deterministic
indexing, duplicate precedence, staged artifact integrity and folder aliases. Two
helper API tests verify inline compression, returned message selection and
explicit file-read protection.

The real binary passes 18 headless scenarios with one existing policy scenario
explicitly skipped, all ten startup cases and all ten terminal regression cases.
Coverage includes Chat Completions, Responses and Anthropic Messages, streamed
tool calls, context-rejection recovery, blocked credentials, queued resets,
external skill additions, bridge ownership and cancelled judgments.
In the local release terminal checks, first paint took 0.297-0.312 seconds with
preparation workers blocked. Login, help, effort, context, a new session,
workspace changes and quit all worked while indexing was held.

The installed Headroom helper compresses a synthetic 500-row tool result using
its isolated interpreter, saving 5,989 tokens (66.36%). The inline adapter now
preserves structured bodies for detection, supplies the real tool name for
file-read protection, and disables history protection for the single result.
OptMem note/recall is checked in a throwaway
memory directory. Current Penecho/egaki/OMP help confirms the wrapper flags; no
live account login or model generation is used for these checks. Every GitHub
source repository in the final audit resolves; the remaining 14 non-repository
references are examples or GitHub service endpoints. All 77 skill checkouts
are verified against their pinned commits. Typography clears the 3.0 contrast
floor in every theme at both tested widths.

## Harness review fixes

- Account writes and summaries run off the event thread. Late completions cannot
  apply to a different login modal or provider selection.
- Queued work owns its session, workspace, attachments and request identity.
  Resetting or changing sessions discards obsolete requests.
- Jev bridges carry process creation identity and an instance nonce. Stop checks
  ownership before shutdown/termination. Start is serialized and idempotent.
- Judgment requests share cancellation and a total deadline across retries and
  batches. Late questions/approvals cannot reopen cancelled turns.
- Judgment accounting belongs to the actual session, turn and route, including
  cancellation/error outcomes and local bridge classification.
- Markdown links keep complete destinations. Windows treats paths as data;
  visible file checks happen on a bounded worker.
- Context recovery accounts for instructions, tools, output reserve and huge
  retained bodies. Full bodies remain retrievable from context storage. Only an
  explicit user budget ends the turn.
- Release publication is gated by isolated Rust checks and real-binary headless
  and terminal tests on Windows/Linux. Actions use immutable commit pins. E2E
  separates passes/failures/skips and rejects unknown cases.

See [startup measurements](startup-performance.md) for the original baseline.
