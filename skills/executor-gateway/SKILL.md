---
name: executor-gateway
description: "Executor MCP gateway: one catalog for OpenAPI/GraphQL/MCP integrations shared across agents. Use for external APIs, multi-agent tool routing, policies."
---

# Executor (executor.sh)

Docs: https://executor.sh/docs
CLI: `npm i -g executor@beta` (Executor 2, currently 2.0.0-beta.37; stable 2.0.0 not yet on npm `latest`)

## What it is

Local (or cloud) MCP gateway: configure integrations once, every agent gets the same tools with shared auth + policies. Executor 2 serves one streamable-HTTP MCP endpoint; the CLI no longer has `tools`, `call`, `mcp`, `web`, or `install`; agents reach tools only through the endpoint.

## Nur integration

- Tool: `executor` (status / start / stop / login / skills / search / describe / call / resume / execute / pair / upgrade)
- Service: `executor service start` starts the durable background daemon (no OS autostart, run it again after a reboot)
- MCP HTTP: `http://127.0.0.1:4312/mcp` (port from `EXECUTOR_PORT` or `executor service set port`)
- Sign-in: the endpoint requires auth. `executor(action=login)` signs in once over MCP OAuth (browser); `EXECUTOR_API_KEY` also works
- Prefer native tools for repo work; use Executor for **external SaaS/APIs**

## How calls work

The endpoint exposes exactly six MCP tools: `skills` (`{}` lists every app skill, `{app, name}` reads one; start with `{app: "executor", name: "executor"}`), `search` (`{query, namespace}`, returns exact paths; send the returned `next` object back as the whole input for the next page), `describe` (`{paths}` from `search`), `call` (`{ "calls": [{ "path": "tools.<app>.<tool>", "input": {...} }] }`), `execute` (`{code}`, a JS program over apps), `resume` (`{requestId}`, plus `{action, content}` when the server asked for an answer). On approval-required or input-required, show it to the user and resume with their answer; never repeat the call.

## Common commands

```
executor service status     # URL, version, process ID
executor service start      # start the background daemon
executor service stop       # stop it
executor pair               # print a new connection link (a credential)
executor upgrade            # newest release on your channel
executor                    # start background + open the dashboard
```

## Notes

- Data lives in `~/.executor/v2/cli` (separate from Executor 1); first launch migrates old desktop data once.
- `executor --version` reports the installed line: `2.x` is V2 (MCP path), `1.x` is V1 (legacy CLI path, still supported by the `executor` tool as a fallback).
- If the MCP endpoint refuses connections after a reboot, the daemon is not running: `executor service start`.
