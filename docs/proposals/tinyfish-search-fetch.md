# Proposal: TinyFish Search + Fetch as direct backends

Status: proposal (2026-11-07). Author: CIA agent, from live docs and the
repo's current `web_search` / `web_fetch` code.

## 1. What TinyFish is

TinyFish (`tinyfish.ai`) is a web layer built for agents. One API key, four
endpoints. Search and Fetch are **free, no credit card**. The other two are
metered and out of scope here.

| Endpoint | What it does | Pricing |
|----------|--------------|---------|
| **Search** `api.search.tinyfish.ai` | Query in, structured JSON out: title, URL, snippet, result position. Rank-stable across calls, under 0.5 s. Location, language, and recency/date filters. | Free (30 req/min per the current docs; one older comparison says 5/min - confirm against the live reference) |
| **Fetch** `api.fetch.tinyfish.ai` | Any URL in, clean markdown / JSON / HTML out. Renders in a real Chromium (JavaScript, SPAs), then strips nav bars, cookie banners, ads, and scripts. Failed URLs are free. | Free (150 URLs/min per the current docs; one older comparison says 25/min - confirm) |
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

## 3. The change

Add TinyFish as a **direct** backend in both tools. No new key management,
no new dependencies (`reqwest` blocking client already in use), no MCP, no
Agent/Browser endpoints.

### 3.1 `web_search`: TinyFish Search backend

Insert it in the chain **between Brave and the DuckDuckGo scrape** (after
Exa/Brave so paid neural search keeps precedence when the user configured
it, ahead of the scrape so the scrape becomes the last resort it should
be). Fires when `TINYFISH_API_KEY` is set. Any failure or empty result
falls through to the DuckDuckGo scrape exactly like Exa/Brave do today.

```rust
// src/tools/web_search.rs - next to search_exa / search_brave.
/// TinyFish Search (`GET /?query=...`), free structured search.
/// Key: TINYFISH_API_KEY (https://tinyfish.ai).
fn search_tinyfish(
    key: &str,
    query: &str,
    max: usize,
) -> std::result::Result<Vec<SearchResult>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.search.tinyfish.ai")
        .query(&[("query", query)])
        .header("X-API-Key", key)
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("tinyfish search: HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    // Response shape (confirm field names against the live Search API
    // reference before merging): an array of results, each with title,
    // url, snippet, and position. Unknown fields are ignored.
    let rows = body
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(rows
        .iter()
        .take(max)
        .filter_map(|r| {
            let title = r.get("title")?.as_str()?.to_string();
            let url = r.get("url")?.as_str()?.to_string();
            let snippet = r.get("snippet").and_then(|s| s.as_str()).unwrap_or("");
            Some(SearchResult {
                title,
                url,
                snippet: snippet.to_string(),
            })
        })
        .collect())
}
```

And in `execute`, after the Brave block, before the DuckDuckGo client:

```rust
// TinyFish Search (free structured search) when a key is set.
if let Some(key) = search_key("TINYFISH_API_KEY") {
    match search_tinyfish(&key, &query, max) {
        Ok(rows) if !rows.is_empty() => {
            return Ok(render_results(&query, "tinyfish", &rows));
        }
        Ok(_) => {}
        Err(e) => tracing::debug!("tinyfish search: {e}"),
    }
}
```

### 3.2 `web_fetch`: TinyFish Fetch backend

Add a Fetch path that POSTs the URL to `https://api.fetch.tinyfish.ai`
and returns the clean markdown, used when `TINYFISH_API_KEY` is set and
the URL is not already handled by a cheaper local path. The exact request
body shape (URL field name, format selector) must be confirmed against
the live Fetch API reference / cookbook before merging - do not guess it
from the Search shape. The response is markdown, HTML, or JSON per the
requested format; prefer markdown for model consumption.

```rust
// src/tools/web_fetch.rs - sketch, body shape TBD against the live reference.
/// TinyFish Fetch (`POST /`), free Chromium-rendered fetch to clean markdown.
/// Key: TINYFISH_API_KEY.
fn fetch_tinyfish(key: &str, url: &str) -> std::result::Result<String, String> {
    // POST https://api.fetch.tinyfish.ai with the URL; response is the
    // cleaned page (markdown preferred). Confirm the request body against
    // https://github.com/tinyfish-io/tinyfish-cookbook before merging.
    todo!("confirm Fetch request body against the live reference")
}
```

### 3.3 Docs

- `docs/tools.md`: `web_search` backend list gains TinyFish (free,
  `TINYFISH_API_KEY`), and the chain order; `web_fetch` gains the
  TinyFish Fetch path.
- `nur doctor` (if it lists search backends): show `tinyfish` when the
  key is set, same as the Exa/Brave rows.

## 4. Tests

- `search_tinyfish` against a recorded response fixture (a small JSON
  file in `tests/`): parses title/url/snippet, takes `max`, skips rows
  missing title or url, returns `Err` on non-2xx. Mirror the existing
  `decodes_uddg_redirect` / `cleans_html` unit tests in `web_search.rs`.
- `web_search` end-to-end (existing e2e scenario shape): with
  `TINYFISH_API_KEY` unset and no Exa/Brave keys, the chain still ends
  at the DuckDuckGo scrape (no regression); with the key set and the
  endpoint unreachable, it falls through to the scrape instead of
  failing the call.
- `fetch_tinyfish` unit test against a recorded markdown response once
  the body shape is confirmed.

## 5. Acceptance criteria

- [ ] `TINYFISH_API_KEY` set + Exa/Brave unset: `web_search` returns
      TinyFish results, rendered `(via tinyfish)`.
- [ ] `TINYFISH_API_KEY` set + TinyFish down: falls through to the
      DuckDuckGo scrape, call succeeds.
- [ ] No key set anywhere: behavior unchanged (DDG scrape).
- [ ] `cargo check`, `cargo clippy -- -D warnings`, full test suite,
      `python3 scripts/check_docs.py` all clean.
- [ ] No new dependencies; no MCP; no Agent/Browser endpoints.

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
