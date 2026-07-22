# t3code Integration

## Overview
`t3code` (https://github.com/pingdotgg/t3code) is a minimal GUI for coding agents (Codex, Claude, Cursor, OpenCode, Grok) that **delegates 100% of LLM auth to vendor CLIs** and never stores API keys. Its control plane (pairing + DPoP bearer) secures its own server.

NurCLI traditionally stores tokens in `~/.nur/auth.json` and implements custom OAuth flows. t3code's model offers security and ergonomic wins we should adopt.

## Auth Architecture (t3code)
- **Driver layer — BYO-auth**: `ClaudeDriver`, `CodexDriver`, `CursorDriver`, `OpenCodeDriver`, `GrokDriver` each have `configSchema`, `defaultConfig()`, `create(): Effect<ProviderInstance>`, `supportsMultipleInstances`. Probes credentials by reading vendor config dir (e.g. `CLAUDE_CONFIG_DIR` not `$HOME` to preserve macOS keychain) and binary presence. No token exchange.
- **Control plane**: `t3 auth pairing create/list/revoke`, `session issue/list/revoke`. Contracts: `ServerAuthPolicy` = `desktop-managed-local | loopback-browser | remote-reachable | unsafe-no-auth`, Bootstrap `desktop-bootstrap` vs `one-time-token`, scopes `AuthAdministrativeScopes` vs `Standard`.
- **DPoP** (RFC 9449): `verifyRequestDpopProof()` validates DPoP header, method+URL, thumbprint, anti-replay via `SHA-256(thumbprint:jti)` stored in SQLite.
- **Persistence**: SQLite via `effect/unstable/http`, atomic writes via `atomicWrite.ts`.

## What nur-cli can improve (from t3code)
1. **Import-first**: Probe vendor CLI auth files before prompting. Already have `import_existing_session` for openai/xai/kimi/anthropic/hf, but missing Cursor/OpenCode. Should default on `auth status` and hint `codex login` etc. if missing.
2. **Env isolation**: Per-instance env merging (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) — prevents breaking host keychain. Nur currently has global env. Add local `.nur/env` or per-provider env override UI.
3. **Pairing flow for remote**: One-time pairing link `/pair#token=...` elegant for headless/SSH. Reuse for `nur serve` — issue one-time link for remote TUI.
4. **DPoP**: If nur adds server mode, copy anti-replay (store jti hash).
5. **Scope separation**: Administrative vs standard scopes — split `auth.json` tokens into read/write scopes.
6. **No-secret-storage / delegate mode**: Offer `nur login --delegate` that verifies vendor CLI auth exists without storing token — reduces surface.
7. **Atomic writes**: Use atomic write for `auth.json` (t3code does) to avoid corruption on crash.
8. **Driver registry pattern**: Refactor `providers.rs` + `oauth/flows.rs` into driver registry — each driver has `displayName`, `supportsMultipleInstances`, `configSchema`, `checkStatus()`.

## Full Integration Plan
- **Phase 1 (this commit)**: Add `src/t3code.rs` compat module that mirrors t3code's driver probing with env isolation, adds Cursor/OpenCode import, atomic write wrapper, delegate probe, and pairing token generator (simplified, no DPoP yet). Wire into `auth.rs` and `oauth/flows.rs`.
- **Phase 2**: Implement `ProviderDriver` trait in Rust, refactor providers into drivers, add `nur t3code` subcommand to launch/check t3code server status, and add `t3code` tool (`t3code: action=status|probe|pairing_create`) similar to `akarso`/`graphjin`.
- **Phase 3**: Full pairing + DPoP for `nur serve` remote, SQLite session store, and scope separation.

## Usage
- `/t3code` or `use t3code skill` triggers this playbook.
- `nur auth status` now shows vendor CLI probe results (green if `claude auth login` etc. present).
- `nur login --delegate` verifies without storing.
- Future: `nur t3code probe` lists all drivers and their auth status.

## References
- https://github.com/pingdotgg/t3code
- `apps/server/src/provider/Drivers/*`, `provider/ProviderDriver.ts`, `auth/dpop.ts`, `cli/auth.ts`, `packages/contracts/src/auth.ts`
