---
name: graphify
description: "Code knowledge graph. Prefer graphify query/path/explain over broad grep when graphify-out/ exists. Build with extract (local AST, no API key)."
---

# Graphify — knowledge graph over the workspace

Meta auto-installs `graphifyy` (CLI: `graphify`) and registers the agents skill.
Use the **`graphify`** tool or `/graphify`.

## Fast path

1. If `graphify-out/graph.json` exists and the question is architectural →
   `graphify(action=query|path|explain)` immediately.
2. If missing → `graphify(action=extract)` (defaults to `--code-only`, free, local).
3. Full docs/PDF semantic pipeline: read upstream skill references or run with `code_only=false`
   (needs an LLM backend).

## Actions

status · query · path · explain · affected · report · extract · update

Upstream: https://github.com/Graphify-Labs/graphify
