# Security

NurCLI is community software. Keys and sessions stay on your machine.

On Windows, links and file paths open through native shell APIs, with paths
passed as data. Bridge termination checks process creation identity and the
bridge instance. These changes prevent shell interpretation of filenames and
termination through stale PID records.

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.nur/auth.json` | Provider API key / tokens after `/login` or `nur auth login`; 0600 on Unix, owner-only ACL on Windows, and sealed with Windows DPAPI at rest |
| Env `NUR_API_KEY` / vendor keys | Optional override (never printed in logs) |
| `~/.nur/provider_keys.json`, `provider_sessions.json` | Scoped and sidecar keys (TypeSafe, Enclave, failover routes) and per-provider OAuth sessions; same protection as `auth.json` |
| `~/.nur/secrets.json` | Your own credentials (`/secrets`, `nur secrets set`): owner-only. Shell commands get each as `$NAME`; tool output shows `[secret:NAME]`, never the value, and a key pasted into the prompt is saved here instead of sent |
| `~/.nur/plugins/` | Marketplace skill packs (local clones; no secrets) |
| `~/.nur/sessions/` | Session files + `.json.bak` / `.precompact.bak` (no key) |
| `~/.nur/tool-results/` | Spilled large tool outputs (may include workspace text) |
| `~/.nur/nur.log` | Tracing log (not the terminal; may include paths) |
| `~/.nur/status.json` | Live token usage (no key) |
| `~/.nur/usage.jsonl` | Per-request usage log (no key) |
| Workspace `.nur/frames/` | Extracted video keyframes (local; may be large) |

---

## What is never committed

- `~/.nur/` directory
- `.env` files with keys
- Session dumps
- Workspace `.nur/frames/` dumps of sensitive UI

!!! warning "Session sensitivity"
    Session `input_items` may include base64 media when vision (`look` / auto-attach) is used. Treat session files as potentially sensitive.

The edit tools (`write_file`, `edit_file`, `multi_edit`, `apply_patch`, `hashline_edit`) are
screened before they write: a private key (a `BEGIN ... PRIVATE KEY` block with key material) is
refused, and a live-looking token (OpenAI, Anthropic, GitHub, Slack, AWS, Google) is written with a
warning in the tool result so the model moves it to an environment variable or a gitignored file.
Code and docs that only mention keys, and placeholders such as `sk-xxxx…`, are not flagged.

---

## Execution safety

nur has **no OS sandbox**. Shell commands and tools run with your user
privileges, exactly as if you typed them. What nur provides is a gate in
front of the model, not a wall around it:

- **Approval modes**: `manual` asks before every mutating tool call, `plan`
  allows reads and a known read-only shell set (anything else asks; VCS
  mutations, publishing and dependency installs stay blocked), `auto`
  approves everything - choose it with Shift+Tab or `--mode`. `nur run` and
  `--continuous` auto-approve only with an explicit `--yes`.
- **Bash denylist**: refuses destructive commands even after approval and in
  auto mode. It parses each command segment (quotes, chains, `sudo` /
  `powershell -Command` wrappers) instead of matching substrings: recursive
  deletes of `/`, `~`, `$HOME`, `%USERPROFILE%` or a drive root
  (`rm -fr ~`, `Remove-Item -Recurse -Force C:\`, `rmdir /s /q`, `del /s`),
  raw disk writes (`dd of=/dev/…`, `> /dev/sd…`), `format`, `cipher /w`,
  `mkfs`, machine shutdown, fork bombs and encoded PowerShell. It is a
  heuristic backstop, not a confinement boundary.
- **Per-domain fetch approval**: `web_fetch` to a domain you have not
  approved this session asks first, in every prompting mode - a fetched page
  can carry instructions, and a new host is a choice you should make.
- **Real-browser control**: the `browser` tool drives your logged-in Chrome,
  so control actions always take the approval path, and an "always" grant
  covers one action (`browser:click`), never the whole tool.
- **Peer mail**: sessions on this machine can leave each other messages
  (`~/.nur/peers`, the `message` tool). Inbound mail is **off by default**
  (`message_inbound = "refuse"`); `ask` makes you approve each letter before
  the model sees it, and only `accept` delivers directly. Letters carry the
  sender's per-mailbox registration secret so a local process cannot forge
  mail from a live session, delivery is wrapped in an explicit
  `<peer-mail authority="none">` block, and on Windows the peers directory
  ACL is reset to the owning account. Peer mail is data with no authority:
  it cannot approve actions or change config.
- **Supply chain**: release binaries are minisign-signed in CI and the
  updater verifies the signature against a public key pinned in the source
  before installing (fail closed when the signature asset is missing); every
  ecosystem component installs at an exact pinned version or commit from
  `src/ecosystem/lock.json` (git clones verify `HEAD` equals the pin), the
  `nur-cli` npm shim fetches only its own version over https with a mandatory
  checksum, and ecosystem tools never write credentials outside an approved
  tool launch.
- **Provider keys stay out of tool commands**: model-provider keys
  (`OPENAI_API_KEY`, `NUR_API_KEY` and the rest of the catalog's key
  variables) are removed from the environment of everything tools run
  (`bash`, background jobs, the REPL's shell), so a `printenv` or a
  prompt-injected `curl` cannot put them in the transcript. `GITHUB_TOKEN`,
  `HF_TOKEN`, `CLOUDFLARE_API_TOKEN` and `AWS_BEARER_TOKEN_BEDROCK` pass
  through for your own tools; your `hooks.toml` scripts keep the full
  environment. `[tools] pass_provider_keys = true` passes every key.
- **Protected project paths**: an "always" grant for a write tool covers
  ordinary files only. A write under `.git/`, `.github/`, `.nur/` or
  `.vscode/` needs its own approval, and a write inside `.git/` (hooks,
  `core.fsmonitor`) asks even in `auto` mode, since it runs code on the next
  git command.
- **Workspace-root refusal**: nur refuses to run tools when the working
  directory is a filesystem root (`C:\`, `/`); pass `--cwd` or `/cd` into a
  project folder.
- **SSRF blocks**: web tools reject private-IP and loopback targets, resolve
  DNS and pin the address across redirects.
- **Timeouts**: long-running commands are killed, and a command that just
  timed out cannot be retried unchanged for 15 minutes.
- **Atomic IO**: all writes to `~/.nur/` use atomic file operations (write-to-temp, rename)
- **Session bak**: each session save copies the previous file to `*.json.bak` first
- **Optional rules**: `permissions.toml` deny/ask/allow; plan mode still blocks code authoring / VCS. A project's own `.nur/permissions.toml` cannot loosen anything until you trust it: its deny/ask rules apply at once, its allow rules wait for `/permissions trust` (or `nur permissions trust`), and an edit to that list holds it again. `bash` rules see every command in a chain: deny/ask fire on any of them, allow needs all of them and never covers a substitution or a redirect into a file ([details](configuration.md#permission-rules)). Deny/ask also see the command a wrapper runs: `sudo`, `env`, `xargs`, `time`, `timeout`, `nohup`, `{ …; }`, `if … then`, and the script of `bash -c`, `sh -c`, `cmd /c`, `pwsh -Command` or `eval`. A rules file that does not parse fails closed: every tool call asks, and nur names the file and the error, until it is fixed. Rules hold for read-only tools too: a call a deny or ask rule names leaves the free parallel batch and takes the gated path
- **What the model reads, it sees**: as in Claude Code and Codex, file contents and tool output reach your provider verbatim, secrets included. nur never redacts tool results, because a rewritten key corrupts the config edits the model makes. To keep a file away from the model, deny it, e.g. `deny = ["read_file:*.env"]` in `~/.nur/permissions.toml`. Rules match a tool's primary argument (the path for file tools, the command for `bash`), so a repo-wide `grep` or a `cat` in `bash` can still surface the file; deny those patterns too if that matters. (nur's own spill files, context store and memory still refuse secret-shaped bodies.)
- **Optional hooks**: `hooks.toml` pre/post tool shell (local only; you control the script)
- **Plan mode's read-only shell set** excludes forms that write or run a program without a redirect: `sort -o`, `uniq IN OUT`, `tree -o`, `xxd -r`, `file -C`, git's `-c`, `--ext-diff`, `grep -O` and `--upload-pack`, `printenv`, and PowerShell script blocks. Those ask.
- **Code mode scripts** may call read-only tools only, and never `web_fetch` or `web_search`: a new domain needs its approval prompt, which a script cannot show.
- **Telegram gateway**: answers one chat (`--chat`); in a group it also obeys one sender only (`--user`), and runs nothing without it.
- **Project skills are untrusted code-shaped text**: skills under a repo's own `.nur/skills`, `.claude/skills` or `.agents/skills` are inert until `/skills trust` records a digest of their `SKILL.md` files (same model as project permissions), so a cloned repo cannot inject instructions into the agent on first open. An edit to any project skill holds it again. The project's `.nur/stream-rules.toml` (rules that inject text into the conversation) falls under the same trust. A skill can also mark itself `disable-model-invocation: true` (user-only: no NL activation, and the `skill` tool refuses to read it to the model) or `user-invocable: false` (engine-only). Natural-language skill activation is opt-in (`[skills] auto_activate`).
- **TypeSafe (Jev) guard**: the `typesafe` tool is declared read-only, so it runs without approval, and it refuses secret-shaped content in `state` / `query` / `candidates` before any request is sent. A loopback endpoint needs no key (local engine); a hosted endpoint needs `TYPESAFE_API_KEY`

---

## Provider privacy & cross-provider failover

Each provider in the picker carries a **privacy tier**, shown as a badge:

- `LOCAL` — runs on your machine (Ollama, LM Studio, llama.cpp, vLLM, Jan); prompts never leave localhost.
- `TEE` — hardware-enclave inference with remote attestation (e.g. Venice).
- `ZDR` - the provider's default policy does not train on your data and offers/defaults to zero data retention (OpenAI, Anthropic, Google paid, Groq, Together AI, Fireworks AI, Azure OpenAI, Amazon Bedrock, …).
- *(no badge)* `STANDARD` — standard API terms; assume retention unless you know otherwise (DeepSeek and other train-by-default providers, logging gateways, unclear policies).

Tiers are built in from a review of each provider's public policy, and you can override any of them for your own account/endpoint with **Alt+P** in the provider picker (saved as a `provider_privacy` override — no config file).

**Failover respects privacy.** When the active provider returns a server error, nur can retry against a configured `fallback_providers` chain (set up in `/failover`). Failover **never silently downgrades** you to a weaker privacy tier than your active provider — a weaker fallback is skipped unless you set `failover_allow_downgrade`. It also only fails over *before any output has streamed*, so the transcript never duplicates. Fallback keys come from each provider's own env var or a key/OAuth session you save in `/failover` — never from the active provider's `auth.json`.

**Failover respects billing.** When the active route is subscription-based (an OAuth sign-in, or OpenCode Go's `/zen/go/` endpoint) and the fallback would bill per token, the fallback is skipped with a status line instead of quietly charging you; set `failover_allow_paid = true` in `~/.nur/config.toml` to allow it.

**Subscription sign-ins use the vendor's client identity.** Browser sign-in on the Claude, ChatGPT, and Gemini subscription routes presents nur as the vendor's own first-party CLI, and the login flow says so before the browser opens. The route-by-route table is in [authentication.md](authentication.md#client-identity-on-subscription-routes).

## Enclave security agents

The `enclave` tool sends its key only as a bearer token over https to the
configured Enclave MCP endpoint (plain http only for a loopback host) and never
follows redirects, so the key cannot be replayed elsewhere; the model sees a
fingerprint at most. Listing is read-only. Every `call` takes the approval path
and is treated as high impact regardless of the server's own hints, because it
can start pentests against real systems; plan mode blocks it. Tool results are
external data, not instructions. See [enclave.md](./enclave.md).

---

## Session receipt (verify what ran)

`/receipt` shows an append-only, **hash-chained** log of the session at `~/.nur/receipts/<session>.jsonl`: every model request (provider, model, **privacy tier used**, whether a failover served it, token counts) and every tool call (name, args/result SHA-256, outcome). Each entry folds in the previous entry's hash, so altering any earlier line breaks the chain — `/receipt` prints an integrity check and flags the first tampered entry. This is the proof that your prompts stayed in the tier you chose and that failover didn't move them.

---

## Cost controls

- `/budget` and `max_session_cost_usd` / `max_session_tokens` hard-stop new API turns
- Oversized tool results spill to disk instead of re-entering context forever
- `/poor` reduces prompt bulk without removing tools

---

## Install safety

`install.ps1` / `install.sh` / release **EXE** (`nur install`):

- May **read** a key already present in your environment and store it under `~/.nur/` on your machine
- Do **not** write keys into the git checkout or GitHub
- Write the binary to `~/.local/bin` and verify **SHA-256** of the installed binary
- Best-effort prereq installs (Node, uv, …) are local to your machine. Their fallback installers are pinned to a release (bun, uv) or commit (sgrep, matching `src/ecosystem/lock.json`), and on Windows `rustup-init.exe` runs only if it matches the SHA-256 rustup publishes beside it

---

## Binary integrity

Every release asset ships with a published `<asset>.sha256`. The self-updater (`nur update` and the launch check) and the `npx nur-cli` shim compare each download against it and refuse a mismatch before anything is written; neither ever installs an archive or a checksum file as the binary. After install, the installer records the binary's SHA-256 next to it (`nur.sha256` in the install directory) and `nur doctor` verifies it:

```bash
nur doctor
# should show: sha256  <hash>  (matches install record)
```

---

## Reporting vulnerabilities

Open a private report or issue on [nuroctane/nur-cli](https://github.com/nuroctane/nur-cli) if you find a vulnerability in this client.
