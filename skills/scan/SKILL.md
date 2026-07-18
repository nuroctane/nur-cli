---
name: scan
description: Analyze this repository and publish a shareable "codebase scan" map to foglamp.dev — architecture, AI usage, tools, and flows. You produce only a small JSON object; a fixed renderer draws the map.
disable-model-invocation: true
---

Analyze THIS repository and publish a shareable "codebase scan" to foglamp — a
map of how the codebase works and how it uses AI. You produce only the data (a
small JSON object); a fixed renderer draws the scan. Write no HTML or CSS.

If the user gave a focus after `/scan`, center the map on that area (still show
enough surrounding context to make it readable). With no focus, map the whole
repository.

## Critical — do not stall

- **Start tools immediately.** List dirs, grep for agents/models/tools, read key
  entrypoints. Do **not** end a turn with only reasoning.
- **Local file first.** Write `.foglamp/scan.json` **before** any upload talk.
  That file is the map. Path: `<cwd>/.foglamp/scan.json`.
- **Ask only before upload.** Investigation + writing `scan.json` need no
  special consent. Only the POST to foglamp.dev needs a clear yes from the user.
- **Plan mode cannot write.** If writes are blocked, tell the user to
  `Shift+Tab` to **manual** or **auto**, then continue. Do not pretend you
  finished.
- When the user asks "where is it" / "go ahead":
  - If `.foglamp/scan.json` exists → print its absolute path + a short summary,
    then upload only if they approved.
  - If missing → investigate and write it now (do not re-ask for explore
    permission).

## Steps
1. **Investigate** the repo with tools (read/grep/list). Build the JSON below.
2. **Write** it to `.foglamp/scan.json` (create `.foglamp/` if needed). Confirm
   the path in your reply: `wrote .foglamp/scan.json (N nodes)`.
3. **Ask once** before publish: "This uploads a high-level summary of your
   architecture (models, tools, integrations, and main flows — no code or
   secrets) to foglamp.dev and creates a public, unlisted link. Upload?"
   Stop here until they say yes / go ahead / upload / publish.
4. On yes: **Upload** (see "Publish"), capture the JSON response.
5. Save the response to `.foglamp/scan.lock.json` (so a later run updates the
   same URL). `.foglamp/` must stay gitignored — the edit token is a secret.
6. Open the returned `url` and give it to the user.

## How to investigate
- Find where AI runs: generateText / streamText / generateObject / streamObject,
  @ai-sdk/* providers, agent loops, tool definitions (tool({...})).
- Identify the models and their provider (OpenAI, Anthropic, Google, …).
- Identify tools models can call (Exa, Firecrawl, Parallel, DB queries, internal
  functions) and external integrations/services.
- Map the business logic too: the internal services/pipelines the product is
  built from (billing, ingestion, background workers, domain services) — these
  become "service" nodes, and the interesting sentence goes on the edge
  (e.g. "charges Stripe on trial end").
- Map the main flows: entry points (routes, webhooks, pages, CLIs), scheduled jobs
  (crons/queues/workers), the agents, the models/tools they use, and the
  datastores/services they read and write.

## Output contract — write EXACTLY this shape to .foglamp/scan.json
{
  "version": 1,
  "project": {
    "name": "string (<=48)",
    "slug": "lowercase-dashed (<=48)",
    "tagline": "one line (<=80, optional)",
    "iconDomain": "favicon domain for the project, e.g. acme.com (optional)",
    "date": "YYYY-MM-DD"
  },
  "stats": { "agents": 0, "models": 0, "tools": 0, "integrations": 0 },
  "topModels":       [ { "id": "gpt-4o", "label": "GPT-4o", "domain": "openai.com" } ],
  "topTools":        [ { "id": "exa", "label": "Exa", "domain": "exa.ai" } ],
  "topIntegrations": [ { "id": "stripe", "label": "Stripe", "domain": "stripe.com" } ],
  "graph": {
    "nodes": [
      { "id": "chat", "label": "Dashboard chat", "kind": "entry", "sub": "/api/chat" },
      { "id": "agent", "label": "Support agent", "kind": "agent", "sub": "streamText",
        "sourceRef": "src/agents/support.ts:42",
        "detail": "Answers tickets with order lookups (<=200, optional)" },
      { "id": "gpt4o", "label": "GPT-4o", "kind": "model", "domain": "openai.com" },
      { "id": "billing", "label": "Billing service", "kind": "service",
        "sourceRef": "src/services/billing.ts" },
      { "id": "pg", "label": "Postgres", "kind": "store", "domain": "postgresql.org" }
    ],
    "edges": [
      { "from": "chat", "to": "agent", "kind": "triggers" },
      { "from": "agent", "to": "gpt4o", "kind": "calls" },
      { "from": "billing", "to": "pg", "kind": "writes", "label": "charges on trial end" }
    ]
  }
}

## Rules (these keep every scan consistent — do not break them)
- Caps: topModels <= 3, topTools <= 10, topIntegrations <= 10, graph.nodes <= 60,
  graph.edges <= 120. One map holds everything — AI flows AND business logic.
  Big maps are welcome (the viewer pans); aim for 20-40 nodes on a substantial
  codebase. Rich, not sparse — but every node must earn its place.
- Give every distinct agent its OWN node when there are <= 10 agents; only
  merge agents into one node when they are numerous and near-identical (then
  say so in sub, e.g. "12 near-identical scrapers"). Chain agents with
  agent->agent edges when one feeds the next.
- group (optional, <=24): tag related nodes with a shared group name — those
  nodes render as one labeled vertical stack. Group by feature/domain the way a
  team would say it ("Billing", "Ingestion", "Setup pipeline"), not by file
  layout. Use 2-3 groups of 3-6 nodes; leave hub-and-spoke nodes ungrouped.
- Node labels <= 28 chars, sub <= 40, edge labels <= 24.
- kind is one of: entry (trigger/route/page/CLI), cron (scheduled job), agent,
  model, tool, service (internal business-logic module/pipeline the project
  owns), store (DB/cache/index), external (3rd-party API).
- Edge kind (optional): "calls" | "reads" | "writes" | "triggers" — what the
  connection does. Prefer setting it; it's shown quietly (revealed when a flow
  is traced). Add a label only when a specific phrase says more (e.g. "charges
  on trial end" — put the business logic on edges); labels are always visible.
- domain is a favicon domain with no scheme (openai.com, anthropic.com, exa.ai,
  clickhouse.com). Add it to anything a recognizable company/product owns; omit it
  for purely internal nodes (entries, crons, services, internal tools). Use the
  product domain for models (gemini.google.com for Gemini, claude.ai for Claude).
- detail (optional, <=200) is shown when a node is clicked — one sentence of
  what it does. sourceRef (optional, <=120) is the repo path (plus :line) where
  the node lives, e.g. "src/agents/support.ts:42" — add it to internal nodes so
  teammates can jump to code.
- Every edge's from/to must reference an existing node id; ids unique.
- Use today's date for project.date.

## Publish
First run (no .foglamp/scan.lock.json):
  curl -sS -X POST https://api.foglamp.dev/scan \
    -H 'content-type: application/json' --data @.foglamp/scan.json

Update run (a .foglamp/scan.lock.json exists) — keep the same URL:
  jq -n --slurpfile d .foglamp/scan.json \
        --arg t "$(jq -r .editToken .foglamp/scan.lock.json)" \
        '{data: $d[0], editToken: $t}' \
  | curl -sS -X POST https://api.foglamp.dev/scan \
      -H 'content-type: application/json' --data @-

On Windows PowerShell (no jq), first run is the same with curl.exe; for updates,
compose JSON with the editToken from scan.lock.json yourself.

The response is JSON: { "slug", "url", "editToken", "expiresAt" }. Save it to
.foglamp/scan.lock.json, then open url. On a 422 error, fix .foglamp/scan.json
to satisfy the rules and retry.
