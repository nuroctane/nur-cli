---
name: ruflo
description: "Agent meta-harness: vector memory, swarm coordination, hive-mind, hooks. Use for multi-agent orchestration patterns, semantic memory search, and self-learning trajectories."
---

# Ruflo — agent orchestration harness

NurCLI auto-installs `ruflo` and provisions global vector memory at
`~/.nur/ruflo/memory.db`. Use the **`ruflo`** tool (or `/ruflo`) — no separate init.

## When to use

- Need semantic pattern memory across sessions → `ruflo(action=memory_search|memory_store)`
- Multi-step parallel research that benefits from swarm topology → `swarm_init` / `hive_status`
- Check harness health → `status`
- List agent types → `agent_list`

## Actions (via `ruflo` tool)

| action | purpose |
|--------|---------|
| status | ruflo + memory status |
| memory_store | store key/value (+ optional vector) in global AgentDB |
| memory_search | semantic/hybrid search |
| memory_stats | entry counts / backend |
| memory_list | list entries |
| agent_list | available agent types |
| swarm_init | init hierarchical swarm (coordination state) |
| swarm_status | swarm health |
| hive_status | hive-mind status |
| doctor | diagnostics |

## Rules

- Default memory lives under Meta's home (`~/.nur/ruflo/`) so project trees stay clean.
- Prefer PLUR for *preferences and corrections*; Ruflo memory for *patterns, trajectories, embeddings*.
- Prefer graphify for *code structure graphs*.
- Do not require Claude Code — Meta is the host agent.
- Swarm spawn of external Claude/Codex workers is optional; Meta's own `agent` tool covers nested research.

Upstream: https://github.com/ruvnet/ruflo
