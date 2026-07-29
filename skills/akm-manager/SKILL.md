---
name: akm-manager
description: "AKM (Agent Knowledge Management) — package manager for skills/commands/tools across Claude/OpenCode/Cursor."
---

# AKM CLI

npm: `akm-cli` · binary `akm`  
Meta auto-installs; may need [Bun](https://bun.sh) on Windows.

## Use

- Discover / install / update skill packages across agents
- Complements Meta's `skills` CLI and built-in skill loader
- Prefer Meta `skill` tool for day-to-day; use AKM when managing multi-agent skill libraries

```
akm --help
akm list
akm install <package>
```
