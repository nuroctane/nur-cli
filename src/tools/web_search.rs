//! Web search via DuckDuckGo Lite (no API key). Returns title · url · snippet.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct WebSearch;

impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web (DuckDuckGo). Returns top results as title, url, snippet. \
         Use for docs, error messages, APIs; follow up with web_fetch on a result url."
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

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(format!("meta-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| MuseError::Tool(e.to_string()))?;

        let resp = client
            .post("https://html.duckduckgo.com/html/")
            .form(&[("q", query.as_str())])
            .send()
            .map_err(|e| MuseError::Tool(format!("search failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(MuseError::Tool(format!("search failed: HTTP {status}")));
        }
        let body = resp
            .text()
            .map_err(|e| MuseError::Tool(format!("search read: {e}")))?;

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

/// Parse DuckDuckGo html endpoint results (`result__a` links, `result__snippet`).
fn parse_ddg_html(html: &str, max: usize) -> Vec<SearchResult> {
    let link_re =
        regex::Regex::new(r#"<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
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

/// DDG wraps results as //duckduckgo.com/l/?uddg=<percent-encoded-url>&…
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
