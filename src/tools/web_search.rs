//! Web search via DuckDuckGo Lite (no API key). Returns title · url · snippet.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct WebSearch;

impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web (treg when TREG_TOKEN is set, Exa with EXA_API_KEY, Brave with \
         BRAVE_API_KEY, else DuckDuckGo - no key needed). Returns top results \
         as title, url, snippet. Use for docs, error messages, APIs; follow up with web_fetch \
         on a result url."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "max_results": {"type": "integer", "default": 8}
            },
            "required": ["query"]
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let query = arg_str(args, "query")?;
        let max = arg_u64(args, "max_results").unwrap_or(8).clamp(1, 15) as usize;

        // treg first when configured (free search, vendor fallback); any
        // failure or an empty answer falls through to DuckDuckGo.
        if let Some(token) = super::treg::token() {
            match super::treg::search(&token, &query) {
                Ok(rows) if !rows.is_empty() => {
                    let mut out = format!("results for: {query} (via treg)\n\n");
                    for (i, (title, url, snippet)) in rows.iter().take(max).enumerate() {
                        out.push_str(&format!("{}. {}\n   {}\n", i + 1, title, url));
                        if !snippet.is_empty() {
                            out.push_str(&format!("   {snippet}\n"));
                        }
                        out.push('\n');
                    }
                    return Ok(out.trim_end().to_string());
                }
                Ok(_) => {}
                Err(e) => tracing::debug!("treg search: {e}"),
            }
        }

        // Exa (neural search, best for technical queries) when a key is set;
        // any failure falls through rather than failing the call.
        if let Some(key) = search_key("EXA_API_KEY") {
            match search_exa(&key, &query, max) {
                Ok(rows) if !rows.is_empty() => {
                    return Ok(render_results(&query, "exa", &rows));
                }
                Ok(_) => {}
                Err(e) => tracing::debug!("exa search: {e}"),
            }
        }

        // Brave Search API when a key is set.
        if let Some(key) = search_key("BRAVE_API_KEY") {
            match search_brave(&key, &query, max) {
                Ok(rows) if !rows.is_empty() => {
                    return Ok(render_results(&query, "brave", &rows));
                }
                Ok(_) => {}
                Err(e) => tracing::debug!("brave search: {e}"),
            }
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| NurError::Tool(e.to_string()))?;

        let resp = client
            .post("https://html.duckduckgo.com/html/")
            .form(&[("q", query.as_str())])
            .send()
            .map_err(|e| NurError::Tool(format!("search failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(NurError::Tool(format!("search failed: HTTP {status}")));
        }
        // Bounded like web_fetch: an unexpectedly large proxy/HTML response must
        // not be buffered whole.
        let body = {
            use std::io::Read;
            let mut capped = String::new();
            let _ = resp
                .take(2_000_000)
                .read_to_string(&mut capped)
                .map_err(|e| NurError::Tool(format!("search read: {e}")))?;
            capped
        };

        let results = parse_ddg_html(&body, max);
        if results.is_empty() {
            return Ok(format!("no results for: {query}"));
        }
        let mut out = format!("results for: {query}\n\n");
        for (i, r) in results.iter().enumerate() {
            out.push_str(&format!("{}. {}\n   {}\n", i + 1, r.title, r.url));
            if !r.snippet.is_empty() {
                out.push_str(&format!("   {}\n", r.snippet));
            }
            out.push('\n');
        }
        Ok(out.trim_end().to_string())
    }
}

struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// A saved search-provider key, from the environment (same shape as
/// TREG_TOKEN). The secrets store has no raw getter by design - a credential
/// never enters the conversation - so these keys ride the environment.
fn search_key(env_name: &str) -> Option<String> {
    std::env::var(env_name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Shared renderer for the API-backed providers.
fn render_results(query: &str, via: &str, rows: &[SearchResult]) -> String {
    if rows.is_empty() {
        return format!("no results for: {query}");
    }
    let mut out = format!("results for: {query} (via {via})\n\n");
    for (i, r) in rows.iter().enumerate() {
        out.push_str(&format!("{}. {}\n   {}\n", i + 1, r.title, r.url));
        if !r.snippet.is_empty() {
            out.push_str(&format!("   {}\n", r.snippet));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Exa neural search (`POST /search`), best for technical/natural-language
/// queries. Key: EXA_API_KEY (https://exa.ai).
fn search_exa(
    key: &str,
    query: &str,
    max: usize,
) -> std::result::Result<Vec<SearchResult>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "query": query,
        "numResults": max,
        "contents": { "text": { "maxCharacters": 300 } },
    });
    let resp = client
        .post("https://api.exa.ai/search")
        .header("x-api-key", key)
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in v
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let title = row
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let url = row
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let snippet = row
            .get("text")
            .and_then(Value::as_str)
            .map(clean_text)
            .unwrap_or_default();
        if url.is_empty() {
            continue;
        }
        out.push(SearchResult {
            title,
            url,
            snippet,
        });
        if out.len() >= max {
            break;
        }
    }
    Ok(out)
}

/// Brave Search API (`GET /res/search`), 1 query/sec free tier.
/// Key: BRAVE_API_KEY (https://brave.com/search/api).
fn search_brave(
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
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", key)
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", &max.to_string())])
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in v
        .pointer("/web/results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let title = row
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let url = row
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let snippet = row
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if url.is_empty() {
            continue;
        }
        out.push(SearchResult {
            title,
            url,
            snippet,
        });
        if out.len() >= max {
            break;
        }
    }
    Ok(out)
}

/// Parse DuckDuckGo html endpoint results (`result__a` links, `result__snippet`).
fn parse_ddg_html(html: &str, max: usize) -> Vec<SearchResult> {
    let link_re = regex::Regex::new(
        r#"<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#,
    )
    .unwrap();
    let snip_re =
        regex::Regex::new(r#"class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>|class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</td>"#)
            .unwrap();

    let snippets: Vec<String> = snip_re
        .captures_iter(html)
        .map(|c| {
            let raw = c
                .get(1)
                .or_else(|| c.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            clean_text(raw)
        })
        .collect();

    let mut out = Vec::new();
    for (i, cap) in link_re.captures_iter(html).enumerate() {
        if out.len() >= max {
            break;
        }
        let href = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let title = clean_text(cap.get(2).map(|m| m.as_str()).unwrap_or(""));
        let url = resolve_ddg_href(href);
        if url.is_empty() || title.is_empty() {
            continue;
        }
        // Skip ad links
        if url.contains("duckduckgo.com/y.js") {
            continue;
        }
        out.push(SearchResult {
            title,
            url,
            snippet: snippets.get(i).cloned().unwrap_or_default(),
        });
    }
    out
}

/// DDG wraps results as `//duckduckgo.com/l/?uddg=<percent-encoded-url>&…`.
fn resolve_ddg_href(href: &str) -> String {
    if let Some(pos) = href.find("uddg=") {
        let rest = &href[pos + 5..];
        let enc = rest.split('&').next().unwrap_or(rest);
        return percent_decode(enc);
    }
    if href.starts_with("//") {
        return format!("https:{href}");
    }
    href.to_string()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Strip tags and decode the handful of entities DDG emits.
fn clean_text(html: &str) -> String {
    let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
    let no_tags = tag_re.replace_all(html, "");
    no_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_uddg_redirect() {
        let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust%2Dlang.org%2Fbook%2F&rut=abc";
        assert_eq!(resolve_ddg_href(href), "https://doc.rust-lang.org/book/");
    }

    #[test]
    fn cleans_html() {
        assert_eq!(
            clean_text("<b>Rust</b> &amp; <i>Cargo</i>  guide"),
            "Rust & Cargo guide"
        );
    }
}
