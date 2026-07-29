---
name: executor-gateway
description: "Executor MCP gateway — one catalog for OpenAPI/GraphQL/MCP integrations shared across agents. Use for external APIs, multi-agent tool routing, policies."
---

# Executor (executor.sh)

Docs: https://executor.sh/docs  
CLI: `npm i -g executor` (Meta auto-installs)

## What it is

Local (or cloud) MCP gateway: configure integrations once, every agent gets the same tools with shared auth + policies.

## Meta integration

- Tool: `executor` (status / tools search / call / sources)
- Service: `executor install` starts durable local daemon
- MCP HTTP: `http://127.0.0.1:4788/mcp` (stdio: `executor mcp`)
- Prefer Meta's native tools for repo work; use Executor for **external SaaS/APIs**

## Common commands

```
executor tools sources
executor tools search "send email"
executor call <namespace> <tool> '<json>'
executor web          # UI at :4788
```
