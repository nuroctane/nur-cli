---
name: executor-gateway
description: "Executor MCP gateway — one catalog for OpenAPI/GraphQL/MCP integrations shared across agents. Use for external APIs, multi-agent tool routing, policies."
---

# Executor (executor.sh)

Docs: https://executor.sh/docs
CLI: `npm i -g executor@beta` (Executor 2, currently 2.0.0-beta.37; stable 2.0.0 not yet on npm `latest`)

## What it is

Local (or cloud) MCP gateway: configure integrations once, every agent gets the same tools with shared auth + policies. Executor 2 serves one streamable-HTTP MCP endpoint; the CLI no longer has `tools`, `call`, `mcp`, `web`, or `install` — agents reach tools only through the endpoint.

## Meta integration

- Tool: `executor` (status / start / stop / sources / search / describe / call / resume / execute / pair / upgrade)
- Service: `executor service start` starts the durable background daemon (no OS autostart — run it again after a reboot)
- MCP HTTP: `http://127.0.0.1:4312/mcp` (port from `EXECUTOR_PORT` or `executor service set port`)
- Prefer Meta's native tools for repo work; use Executor for **external SaaS/APIs**

## How calls work

The endpoint exposes exactly six MCP tools: `skills` (list apps), `search` (find tools by words, returns exact paths), `describe` (full signature for paths from `search`), `call` (invoke by path, `{ "calls": [{ "path": "tools.<app>.<tool>", "input": {...} }] }`), `execute` (run a JS program over apps), `resume` (answer an approval/input pause). Approvals come back through the agent by default (`model` elicitation mode); answer them with `resume`.

## Common commands

```
executor service status     # URL, version, process ID
executor service start      # start the background daemon
executor service stop       # stop it
executor pair               # print a connection link (read-only)
executor upgrade            # newest release on your channel
executor                    # start background + open the dashboard
```

## Notes

- Data lives in `~/.executor/v2/cli` (separate from Executor 1); first launch migrates old desktop data once.
- `executor --version` reports the installed line: `2.x` is V2 (MCP path), `1.x` is V1 (legacy CLI path, still supported by the `executor` tool as a fallback).
- If the MCP endpoint refuses connections after a reboot, the daemon is not running: `executor service start`.
