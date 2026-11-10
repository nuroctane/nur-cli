# Proposal: TinyFish Search + Fetch as direct backends

Status: implemented in v0.67.0 (proposed 2026-11-07). Author: CIA agent, from live docs and the
repo's current `web_search` / `web_fetch` code.

**Implemented in v0.67.0** (`src/tools/tinyfish.rs`). Checked against the live
references on 2026-11-09: Search is `GET https://api.search.tinyfish.ai?query=`,
results in `results[]` (`position`, `site_name`, `title`, `snippet`, `url`), 30
requests/min per personal account and free up to 12,000/day. Fetch is
`POST https://api.fetch.tinyfish.ai` with `{"urls": [...], "format": "markdown"}`,
pages in `results[].text`, per-URL failures in `errors[]` with HTTP 200, 150
URLs/min and free up to 1,000 fetched URLs/day. The older 5/min and 25/min
figures are stale. Fetch runs as `web_fetch`'s fallback after treg, not ahead of
nur's own request.

## 1. What TinyFish is

TinyFish (`tinyfish.ai`) is a web layer built for agents. One API key, four
endpoints. Search and Fetch are **free, no credit card**. The other two are
metered and out of scope here.

| Endpoint | What it does | Pricing |
|----------|--------------|---------|
| **Search** `api.search.tinyfish.ai` | Query in, structured JSON out: title, URL, snippet, result position. Rank-stable across calls, under 0.5 s. Location, language, and recency/date filters. | Free: 30 requests/min, 12,000/day |
| **Fetch** `api.fetch.tinyfish.ai` | Any URL in, clean markdown / JSON / HTML out. Renders in a real Chromium (JavaScript, SPAs), then strips nav bars, cookie banners, ads, and scripts. Failed URLs are free. | Free: 150 URLs/min, 1,000 fetched URLs/day |
| Agent | URL + natural-language goal; multi-step browser flow, returns clean JSON. | Metered (~$0.016/step) - out of scope |
| Browser | Stealth cloud browser sessions (Cloudflare/Akamai), ~85% success per TinyFish. | Metered (~$0.002/min) - out of scope |

Access: REST (`GET https://api.search.tinyfish.ai?query=...` with an
`X-API-Key` header; `POST https://api.fetch.tinyfish.ai`), an MCP server at
`https://mcp.tinyfish.ai`, a CLI (`npm i -g @tiny-fish/cli`, key in
`~/.tinyfish/config.json` or `TINYFISH_API_KEY`), and a skill
(`npx skills add github.com/tinyfish-io/tinyfish-cookbook --skill
use-tinyfish`). Sources: [What is TinySearch?](https://www.tinyfish.ai/blog/what-is-tinysearch),
[TinyFish cookbook](https://github.com/tinyfish-io/tinyfish-cookbook/blob/HEAD/README.md).
Hermes Agent (Nous Research) is already a TinyFish integration.

## 2. Current state in nur-cli

`src/tools/web_search.rs` (`WebSearch::execute`) chains backends in order,
each falling through on failure or empty results:

1. `treg` first, when `TREG_TOKEN` is set (`super::treg::search`).
2. Exa (`EXA_API_KEY`), neural search.
3. Brave (`BRAVE_API_KEY`).
4. DuckDuckGo HTML scrape (`POST https://html.duckduckgo.com/html/`,
   `parse_ddg_html`) - the no-key last resort.

TinyFish is already in the repo, but only inside `treg`:
`src/tools/treg.rs` (`FREE_VENDOR: &str = "tinyfish"`) falls back across
vendors with TinyFish as the free one, and `served_by: tinyfish` appears in
its tests. `src/tools/web_fetch.rs` has no TinyFish reference. So today
TinyFish only fires when a `TREG_TOKEN` exists, and the no-key fallback is
scraping DuckDuckGo HTML - the fragile link in the chain (HTML parsing,
rate limits, blocks, no structured data).

Keys ride the environment by design (`search_key` in `web_search.rs`):
the secrets store has no raw getter, so `EXA_API_KEY` / `BRAVE_API_KEY` /
`TREG_TOKEN` are env vars. `TINYFISH_API_KEY` follows the same pattern.

## 3. The change (as built)

TinyFish is a **direct** backend in both tools, in `src/tools/tinyfish.rs`. No
new key management, no new dependencies (`reqwest` blocking client), no MCP,
no Agent/Browser endpoints. `NUR_TINYFISH_SEARCH_BASE` and
`NUR_TINYFISH_FETCH_BASE` override the hosts for tests.

### 3.1 `web_search`: TinyFish Search backend

After Exa and Brave, before the DuckDuckGo scrape, when `TINYFISH_API_KEY` is
set. `GET https://api.search.tinyfish.ai?query=...` with `X-API-Key`; rows come
from `results[]`, sorted by `position`, title falling back to `site_name`, rows
without a `url` dropped, rendered `(via tinyfish)`. Any failure or empty answer
falls through to the scrape. A 401, 402 or 429 adds one line to the answer
(`tinyfish: key rejected (HTTP 401), answered by DuckDuckGo`); other failures
stay quiet.

### 3.2 `web_fetch`: TinyFish Fetch fallback

Fetch is a fallback, not a first hop: nur's own SSRF-checked request runs
first, and only a page that refuses it or needs JavaScript
(`treg::should_fall_back`), or a connection failure, goes out. treg runs first
when `TREG_TOKEN` is set, then TinyFish. The request is
`POST https://api.fetch.tinyfish.ai` with `{"urls": [url], "format": "markdown"}`;
the page is `results[0].text`, and a per-URL failure (`errors[].error`, HTTP 200)
is reported by its code. When the fallback fails too, nur keeps its own error
and adds the fallback's (`[rendered fallback failed: ...]` on a refused page).
Each fallback spends one of the 1,000 free daily fetches.

Hardening: response reads are capped (1 MB search, 8 MB fetch), redirects are
refused so the key header cannot follow a cross-origin redirect, the final URL
reaches the header only as a parsed http(s) URL, and the fetch timeout is 120 s
(the backend gives up on a URL at 110 s).

### 3.3 Docs

`docs/tools.md` (`web_search` order, `web_fetch` fallback),
`docs/configuration.md` (`TINYFISH_API_KEY` row with the limits), README tool
table. treg, Exa and Brave have no `nur doctor` row, so TinyFish has none.

## 4. Tests

- Unit (`src/tools/tinyfish.rs`): rank order and url-less rows, per-URL errors
  inside a 200, empty text, forged and non-http final URLs, truncation on a
  multibyte boundary, and which statuses earn a notice.
- E2E 173: key set, Search answers through a stand-in that requires the key;
  rank order, url-less row dropped, key never in the result.
- E2E 174: a rejected key (401) falls through and the answer names it.
- Fetch has no E2E: `web_fetch` refuses loopback targets, so a local page
  cannot trigger the fallback without the real network.

## 5. Acceptance criteria

- [x] `TINYFISH_API_KEY` set + Exa/Brave unset: `web_search` returns
      TinyFish results, rendered `(via tinyfish)` (E2E 173).
- [x] `TINYFISH_API_KEY` set + TinyFish failing: falls through to the
      DuckDuckGo scrape, call succeeds (E2E 174).
- [x] No key set anywhere: behavior unchanged (DDG scrape).
- [x] `cargo clippy -- -D warnings`, full test suite,
      `python scripts/check_docs.py` clean.
- [x] No new dependencies; no MCP; no Agent/Browser endpoints.

## 6. Out of scope

- TinyFish Agent and Browser endpoints (metered; nur-cli already has
  `browser` / `terminal_browser` tools for that job).
- The TinyFish MCP server (nur-cli calls REST directly; no MCP client
  needed for two endpoints).
- The TinyFish CLI / skill (`@tiny-fish/cli`, `use-tinyfish`) - a
  different integration surface; revisit only if the REST backends
  prove insufficient.
- Changing the `treg` vendor chain (TinyFish stays its free vendor;
  this proposal adds the direct path alongside it).
