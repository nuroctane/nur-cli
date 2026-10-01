# Enclave - security agents over MCP

[Enclave](https://enclave.ai) is an autonomous security team: agents that run
pentests, review code and pull requests for vulnerabilities, triage and verify
findings, and watch for CVEs. nur reaches Enclave **natively** through Enclave's
MCP server, so every provider you chat with can start and read Enclave's
security work through one tool: `enclave`.

## Not a chat provider

Enclave has no chat-completions endpoint, so a base-URL override pointed at it
cannot work. Its developer surface is a remote MCP server at
`https://mcp.enclave.ai/` (Streamable HTTP). nur therefore treats Enclave the
way it treats TypeSafe: a **sidecar** credential, pinned at the top of
`/login`, that is never the active model route and does not change the
provider catalog.

The models behind Enclave's agents (Enclave inference or your own keys) are
chosen in Enclave's settings. nur drives the agents; your active provider's
model decides when to call them.

## Setup

1. In Enclave, open **Settings > MCP** and create an API key
   ([app.enclave.ai/settings?section=mcp](https://app.enclave.ai/settings?section=mcp)).
   Keys look like `enc_sk_...` and expire after 90, 180 or 365 days.
2. Give it to nur, any one of:
   - `nur auth login --provider enclave` (prompts for the key; or `--key enc_sk_...`)
   - in the TUI: `/enclave login`, or `/login` and pick **Enclave · security agents**
   - `ENCLAVE_MCP_API_KEY=enc_sk_...` in the environment (the same variable
     Enclave's own Codex setup uses; it wins over a saved key)
3. Check it: `/enclave` reports the endpoint, which key is in use (fingerprint
   only), the server and its tools.

## Use

Ask for security work in plain language, for example *"use Enclave to list the
critical findings for this repo"*. The model lists Enclave's tools, reads their
schemas and calls the one that fits.

| Command | What it does |
|---|---|
| `/enclave` | Status: endpoint, key source, server, tools (runs in the background) |
| `/enclave tools [name]` | Tool catalog, or one tool's full input schema |
| `/enclave login` | Key entry for the Enclave sidecar |

The `enclave` tool:

| Action | Effect | Approval |
|---|---|---|
| `status` | Endpoint, key source, live server and tool check | free (read-only) |
| `tools` | Every tool with parameters, hints and the server's own instructions; `tool=<name>` prints the full schema | free (read-only) |
| `call` | Runs `tool` with `arguments` (an object matching its schema) | gated, high impact |

A call can start real work against real systems, so it always takes the
approval path whatever the server's own read-only hints say. In manual mode
you approve each call; an "always" rule is recorded per Enclave tool
(`enclave:call:<tool>`), so approving one tool never approves another. Plan
mode blocks calls. The system prompt tells the model to target only what you
named or what Enclave already has in scope.

## Configuration

```toml
[enclave]
url = ""            # empty = https://mcp.enclave.ai/
timeout_secs = 120  # upper bound on one MCP request
```

The key is never read from `config.toml`; use the credential store or
`ENCLAVE_MCP_API_KEY`.

## How it works

`src/mcp_http.rs` is a small MCP client for the Streamable HTTP transport:
`initialize` (session id and negotiated protocol revision), `tools/list` with
pagination, and `tools/call`, with responses as JSON or as an SSE stream. On a
stream, server pings are answered and other server requests are declined.
`src/enclave.rs` adds the key order, endpoint checks, session reuse and
rendering.

- The key travels only as `Authorization: Bearer` over https. Plain http is
  accepted only for a loopback host (local testing), and redirects are not
  followed, so the key cannot be replayed to another host.
- The key never enters the model's context: status shows a fingerprint.
- One MCP session is reused across calls. When Enclave ends it (HTTP 404) nur
  opens a new one and repeats the request once. A rejected key (401/403) is
  reported with how to replace it, and nothing is retried.
- Tool results are external data, not instructions. Images and binary
  resources are described rather than inlined.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `no Enclave key` | Create one at Settings > MCP, then `nur auth login --provider enclave` |
| `Enclave rejected the key: HTTP 401` | The key expired or was revoked; create a new one and log in again |
| `[enclave] url must use https` | Point `url` at an https endpoint (http only for `localhost`) |
| `timed out` | Raise `[enclave] timeout_secs`; long work should run as an Enclave agent session you check on |

Enclave also supports OAuth sign-in for MCP clients; nur uses the API key.
