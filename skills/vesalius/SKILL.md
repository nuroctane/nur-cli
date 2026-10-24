---
name: vesalius
description: Pointer skill for Vesalius, a local codebase map for coding agents (macOS on Apple silicon). Use when the user mentions Vesalius or asks to cut the tokens spent orienting in a repo on a Mac, or to answer "what calls this", "where does this feature live" or "what breaks if this changes" in one query.
---

# Vesalius (pointer)

Vesalius (github.com/haplollc/Vesalius, by Haplo) indexes a repository once on the user's Mac
into a local SQLite graph of files, types, functions, imports and references, then answers
structural questions with file:line evidence in one call instead of a chain of searches. Its
authors measured 51% fewer tokens and 43% fewer turns over 20 questions on 6 repositories, with
no measurable change in answer quality. Nothing is uploaded.

## Requirements and install

Apple silicon, macOS 15 or later. Not available on Windows or Linux: there, use `sgrep` for
intent search and `graphify` for the code graph.

```bash
brew install haplollc/tap/vesalius
vesalius setup        # registers the MCP server and indexes the current repo
vesalius doctor       # checks the connection
```

`vesalius setup --undo` reverses setup. It never writes to the repository.

## Using it

Vesalius is an MCP server (`vesalius mcp`, stdio). `vesalius setup` registers it with Claude
Code and Codex; Cursor and other stdio clients take it by hand. nur has no built-in stdio MCP
client, so from nur reach it through the Executor gateway (`executor`, action `install`) if your
Executor build can add a stdio source, or run the questions in one of those clients. Its five
read-only tools are:

| Tool | Answers |
| --- | --- |
| `usages` | who calls or uses X, and what breaks if X changes (`change`: rename, signature, remove) |
| `feature_map` | where and how a feature is implemented |
| `outline` | what a file, type or function contains, at about a fifth of the tokens of reading it |
| `architecture` | targets, packages, dependency direction, cycles |
| `dependency_cycles` | every import cycle with its import lines |

Ask it first for structural questions; read files only for the lines it points to.

## Why nur loads tools on demand

Vesalius' own first version exposed 20 tools and was used in 2 of 24 runs. Cut to 5, with the
two most useful offered up front, it was used in every run. nur's default `lean` tool profile
follows the same finding: core tools are always offered, the rest load through `tool_search`.
