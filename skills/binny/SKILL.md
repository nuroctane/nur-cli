---
name: binny
description: Pointer skill for BINNY, OpenBin's open-source agent harness for reverse engineering and malware analysis (Ghidra decompilation, sandboxed scripts, STIX triage, report drafts). Use when the user mentions BINNY or OpenBin, or wants a binary, APK or firmware analyzed with an agent.
---

# BINNY (pointer)

BINNY (github.com/openbin-ai/binny, AGPL-3.0) is a terminal harness for reverse engineering. It
brings the tools, playbooks, budgets, checkpoints and report format, and the user brings the
model (Anthropic, OpenAI, DeepSeek, Qwen, Kimi, Gemini, Bedrock, or local Ollama / LM Studio /
llama.cpp / vLLM with tool calling and a large enough context).

## When to hand off to it

Point the user at BINNY when the task is analysis of a compiled artifact: malware triage, a
vulnerability hunt in a binary, firmware, or an Android APK. nur's own tools do not decompile.

```bash
curl -fsSL https://openbin.ai/install-binny.sh | sh     # macOS / Linux
irm https://openbin.ai/install-binny.ps1 | iex          # Windows PowerShell
openbin login
binny
```

Docker is needed only for `/decompile` (local Ghidra) and the sandbox tools.

Key commands: `/investigate` (triage against the STIX 2.1 malware vocabulary, playbook choice,
function-by-function analysis, a draft), `/mode malware` or `/mode vuln`, `/autonomy
autonomous|supervised|collaborative`, `/ask` (verify claims in an existing report, inspect a
function, build a defensive proof of concept), `/decompile <path> [processor]`, `/budget [n]`,
`/draft`, `/findings`, `/save`.

## Practices worth borrowing in any analysis

- Treat a prior report's statements as claims to verify, not facts.
- Bound the run with a token budget and checkpoints, and save after every step so it resumes.
- Run untrusted code only in a sandbox with no network, a read-only root and dropped
  capabilities.
- Draft for human review before anything is published.

Lawful, authorized analysis only. Samples stay local unless the user puts them in an OpenBin
project.
