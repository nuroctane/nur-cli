---
name: context-pruning
description: "Dynamic context pruning patterns (OpenCode DCP / Sleev). Meta has native auto-compact; use these rules to manage long sessions."
---

# Context pruning (DCP-inspired)

Upstream: https://github.com/Opencode-DCP/opencode-dynamic-context-pruning  
Successor focus: https://sleev.ai (`npm i -g sleev`)

OpenCode's DCP plugin is **OpenCode-specific**. Meta implements the same goals natively:

## Meta native behavior

- Auto-compact when context pressure is high (~55% of window, once per turn)
- Manual `/compact` slash command
- Tool results are capped; prefer re-query over replaying huge dumps

## Practices for long sessions

1. After a milestone, summarize and drop raw tool blobs (user can `/compact`)
2. Prefer graphify/plur recall over re-grepping the whole repo
3. Don't re-read files already summarized unless editing
4. Parallel read-only tools only — mutating tools stay sequential
5. If using OpenCode elsewhere: `opencode plugin @tarquinen/opencode-dcp@latest --global`

## Compress modes (conceptual)

- **range** — compress a span of turns into one summary
- **dedupe** — identical tool+args keep latest output only
- **purge errors** — drop large error inputs after N turns
