---
name: lightpanda
description: Use Lightpanda, the headless browser built for AI agents, as a fast clean-room browser: one-shot fetches, a CDP server, or an MCP server. Use when a page needs JavaScript rendering that web_fetch cannot do, when a clean session is wanted, or when speed and memory matter more than a logged-in session.
---

# Lightpanda

Lightpanda is an external browser, detected by `nur ecosystem ensure`
but never installed, embedded, or forked by nur-cli. It is AGPL-3.0
licensed; nur-cli uses it only as a separate process via MCP, CDP, or
its fetch command, never as vendored code.

## What it is not

- It is not a replacement for nur-cli's `browser` tool. The `browser`
  tool drives the user's real Chrome session, with login state
  preserved. Lightpanda starts a clean session per MCP session: it has
  no logged-in state, no cookies from the user's browser, and no access
  to authenticated flows. For logged-in work, use `browser`.
- It has no native Windows binary. On Windows, install and run it
  inside WSL, and connect from the Windows host over localhost (WSL
  forwards localhost automatically). Linux binaries need a recent glibc;
  musl (Alpine) does not run them.

## Three ways to use it

1. **One-shot fetch** (simplest, for JS-heavy pages `web_fetch` cannot
   render): `lightpanda fetch --dump markdown <url>` (also `--dump html`,
   `--dump png`, `--dump pdf`, `--dump structured_data`), with
   `--wait-until`, `--wait-ms`, `--wait-selector`, or `--wait-script`
   when the page needs time or an element before it is ready.
2. **CDP server**: `lightpanda serve --host 127.0.0.1 --port 9222`
   (or `--unix-socket /run/user/1000/lightpanda.sock`, owner-only).
   Connect a Puppeteer-style client to the WebSocket endpoint. WebDriver
   BiDi is also available with `--protocol webdriver`.
3. **MCP server**: stdio via the `mcp` tool configuration
   (`lightpanda mcp`), or HTTP via `lightpanda mcp --port 9223` (clients
   POST JSON-RPC to `/mcp` with an `Mcp-Session-Id` header; a fresh id
   is a fresh session, the same id shares a browsing context). MCP tools
   include read-only `getUrl`, `markdown`, `html`, `links`, `tree`,
   `extract`, `findElement`, `nodeDetails`, `getCookies`, `getEnv`,
   `structuredData`, `detectForms`; navigation `goto`, `search`,
   `interactiveElements`; actions `click`, `press`, `fill`,
   `selectOption`, `setChecked`, `hover`, `evaluate`, `screenshot`,
   `download`; and `run` / `save` for PandaScripts (replay a saved
   script deterministically without a model at runtime).

## 1.0 notes (1.0.3, Nov 2026)

1.x ends the beta. New in 1.0: `AudioContext`, the Payment Request API,
WebMCP, new CDP and MCP tools, Grok/xAI and Parallel providers, and
roughly 10% faster page loads. The behavior change that bites: **CORS is
enforced by default** now: a fetch that worked under 0.2.x can fail on
cross-origin requests that the page does not allow. If a 1.x fetch fails
where 0.2.x worked, CORS is the first suspect, not the page.

## Rules

- Telemetry is on by default. Set `LIGHTPANDA_DISABLE_TELEMETRY=true`
  when running it, and say so in the command, so the choice is visible.
- Respect `--obey-robots` where the fetch or serve command supports it.
- Secrets: keep `$LP_*` placeholders in saved PandaScripts; never inline
  a resolved secret. Any credential Lightpanda needs goes through
  nur-cli's `secret` tool and `$NAME`, never pasted into a script, a
  command, or a tool result.
- Vendor performance claims (lighter, faster than Chromium) are the
  vendor's, not nur-cli's. Do not repeat them as measured facts unless
  you measured them on the target machine.
