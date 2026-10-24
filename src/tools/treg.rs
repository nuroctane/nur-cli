//! treg web access (<https://treg.to/web-access>): free web search and page
//! reads through one token, with treg falling back across vendors (TinyFish,
//! Crawl4AI, AnyAPI, ...) when one comes back empty or blocked.
//!
//! Opt-in: nothing here runs unless `TREG_TOKEN` is set (a team or agent token
//! from `treg login` / treg.to/app). With it, `web_fetch` hands a page that
//! refused nur's own request (401/403/429/5xx, or a JavaScript-only shell such
//! as x.com) to `treg.web.extract`, and `web_search` asks `treg.web.search`
//! before DuckDuckGo. Every call prefers the free vendor and carries a cost
//! ceiling, so a routed miss cannot walk up treg's default $1 of vendors.

use serde_json::{json, Value};

pub const TOKEN_ENV: &str = "TREG_TOKEN";
/// Base URL override, for tests and self-hosted treg.
pub const BASE_ENV: &str = "NUR_TREG_BASE";
const DEFAULT_BASE: &str = "https://treg.to";
/// The vendor treg serves search and page reads from for free.
const FREE_VENDOR: &str = "tinyfish";
/// Ceiling per call in USD (`X-Treg-Route-Max-Cost`): a fallback read costs
/// fractions of a cent, so this only stops runaway waterfalls.
const MAX_COST_USD: &str = "0.02";

pub fn token() -> Option<String> {
    std::env::var(TOKEN_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn base() -> String {
    std::env::var(BASE_ENV)
        .ok()
        .map(|b| b.trim().trim_end_matches('/').to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

/// POST one routed job and return the parsed body (or a readable error).
fn call(token: &str, tool: &str, body: &Value) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{}/call/{tool}", base()))
        .header("X-Treg-Token", token)
        .header("X-Treg-Route-Prefer", FREE_VENDOR)
        .header("X-Treg-Route-Max-Cost", MAX_COST_USD)
        .json(body)
        .send()
        .map_err(|e| format!("treg request failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().map_err(|e| format!("treg read failed: {e}"))?;
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
    if !status.is_success() {
        let message = parsed
            .get("message")
            .or_else(|| parsed.pointer("/error/message"))
            .or_else(|| parsed.get("error"))
            .and_then(Value::as_str)
            .unwrap_or(text.as_str());
        return Err(format!(
            "treg {tool}: HTTP {status}: {}",
            message.chars().take(300).collect::<String>()
        ));
    }
    Ok(parsed)
}

/// Read a page as markdown through `treg.web.extract`: the text and the
/// vendor that served it.
pub fn extract(token: &str, url: &str) -> Result<(String, Option<String>), String> {
    let body = call(token, "treg.web.extract", &json!({ "url": url }))?;
    let text =
        page_text(&body).ok_or_else(|| "treg.web.extract returned no page text".to_string())?;
    Ok((text, served_by(&body).map(str::to_string)))
}

/// Search the web through `treg.web.search`: `(title, url, snippet)` rows.
pub fn search(token: &str, query: &str) -> Result<Vec<(String, String, String)>, String> {
    let body = call(token, "treg.web.search", &json!({ "q": query }))?;
    Ok(search_rows(&body))
}

/// The answer `web_fetch` returns for a page treg read.
pub fn fetched(url: &str, text: &str, served: Option<&str>, max: usize) -> String {
    let mut out = format!(
        "url: {url}\nvia: treg.web.extract{}\ncontent-type: text/markdown\n\n",
        served.map(|s| format!(" ({s})")).unwrap_or_default()
    );
    if text.len() > max {
        let mut cut = max;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push_str(&text[..cut]);
        out.push_str(&format!("\n\n[truncated at {max} bytes]"));
    } else {
        out.push_str(text);
    }
    out
}

/// Who answered, for the result header (`_treg.served_by`).
pub fn served_by(body: &Value) -> Option<&str> {
    body.pointer("/_treg/served_by").and_then(Value::as_str)
}

/// The page text inside a routed answer. treg returns `{output, raw, _treg}`
/// for routed jobs and `{result, _treg}` for catalog tools, and the field
/// holding the markdown depends on the vendor, so look in the common places.
fn page_text(body: &Value) -> Option<String> {
    let payload = body
        .get("output")
        .or_else(|| body.get("result"))
        .unwrap_or(body);
    if let Some(text) = payload.as_str().filter(|t| !t.trim().is_empty()) {
        return Some(text.to_string());
    }
    for key in ["markdown", "content", "text", "body", "page_content"] {
        if let Some(text) = payload
            .get(key)
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
        {
            return Some(text.to_string());
        }
    }
    // A vendor that nests one level deeper (`{data: {markdown}}`).
    for key in ["data", "page", "results"] {
        if let Some(inner) = payload.get(key) {
            let nested = match inner {
                Value::Array(items) => items.first().cloned(),
                other => Some(other.clone()),
            };
            if let Some(text) = nested.and_then(|v| page_text(&json!({ "output": v }))) {
                return Some(text);
            }
        }
    }
    None
}

/// Search rows from whichever list the answer carries.
fn search_rows(body: &Value) -> Vec<(String, String, String)> {
    let payload = body
        .get("output")
        .or_else(|| body.get("result"))
        .unwrap_or(body);
    let list = payload.as_array().or_else(|| {
        ["results", "organic", "items", "data", "web"]
            .iter()
            .find_map(|key| payload.get(*key).and_then(Value::as_array))
    });
    let field = |row: &Value, keys: &[&str]| {
        keys.iter()
            .find_map(|k| row.get(*k).and_then(Value::as_str))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    list.map(|rows| {
        rows.iter()
            .map(|row| {
                (
                    field(row, &["title", "name"]),
                    field(row, &["url", "link", "href"]),
                    field(row, &["snippet", "description", "content", "text"]),
                )
            })
            .filter(|(_, url, _)| !url.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Did nur's own request fail in a way another vendor can fix: refused,
/// rate-limited, broken upstream, or a page that only renders with
/// JavaScript (x.com and most single-page apps answer 200 with a stub)?
pub fn should_fall_back(status: u16, content_type: &str, body: &str) -> bool {
    if matches!(status, 401 | 403 | 429) || status >= 500 {
        return true;
    }
    if !(200..300).contains(&status) || !content_type.contains("html") {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    let js_wall = [
        "javascript is not available",
        "enable javascript",
        "javascript is disabled",
        "requires javascript",
        "you need to enable javascript",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    js_wall || visible_text_len(&lower) < 200
}

/// Rough count of visible characters: tags, scripts and styles removed.
fn visible_text_len(html: &str) -> usize {
    let mut count = 0;
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        count += rest[..open].trim().len();
        let tail = &rest[open..];
        let skip_to = if tail.starts_with("<script") {
            tail.find("</script>").map(|i| i + 9)
        } else if tail.starts_with("<style") {
            tail.find("</style>").map(|i| i + 8)
        } else {
            tail.find('>').map(|i| i + 1)
        };
        match skip_to {
            Some(n) => rest = &tail[n..],
            None => return count,
        }
    }
    count + rest.trim().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Routed jobs, catalog tools and vendors that nest the page all yield
    /// the text; an answer with no text is a miss, not an empty page.
    #[test]
    fn page_text_is_found_wherever_the_vendor_puts_it() {
        let cases = [
            json!({"output": {"markdown": "# Stripe"}, "_treg": {"served_by": "tinyfish"}}),
            json!({"output": "# Stripe"}),
            json!({"result": {"content": "# Stripe"}}),
            json!({"output": {"data": {"markdown": "# Stripe"}}}),
            json!({"output": {"results": [{"text": "# Stripe"}]}}),
        ];
        for body in cases {
            assert_eq!(page_text(&body).as_deref(), Some("# Stripe"), "{body}");
        }
        assert_eq!(page_text(&json!({"output": {"markdown": "  "}})), None);
        assert_eq!(
            served_by(&json!({"_treg": {"served_by": "tinyfish"}})),
            Some("tinyfish")
        );
    }

    #[test]
    fn search_rows_accept_the_common_list_shapes_and_drop_urlless_rows() {
        let body = json!({"output": {"results": [
            {"title": "treg", "url": "https://treg.to", "snippet": "tools"},
            {"name": "docs", "link": "https://treg.to/docs", "description": "api"},
            {"title": "no url"}
        ]}});
        assert_eq!(
            search_rows(&body),
            vec![
                ("treg".into(), "https://treg.to".into(), "tools".into()),
                ("docs".into(), "https://treg.to/docs".into(), "api".into()),
            ]
        );
        assert_eq!(
            search_rows(&json!({"output": [{"url": "https://a.b"}]})).len(),
            1
        );
        assert!(search_rows(&json!({"output": {}})).is_empty());
    }

    /// Refusals and JavaScript walls fall back; a real page, a redirect, a
    /// 404 and non-HTML answers do not.
    #[test]
    fn fallback_only_for_pages_another_vendor_can_read() {
        let article = format!("<html><body><p>{}</p></body></html>", "word ".repeat(100));
        assert!(should_fall_back(403, "text/html", ""));
        assert!(should_fall_back(429, "text/html", ""));
        assert!(should_fall_back(503, "text/html", ""));
        assert!(should_fall_back(
            200,
            "text/html",
            "<html><body><noscript>JavaScript is not available.</noscript></body></html>"
        ));
        assert!(should_fall_back(
            200,
            "text/html; charset=utf-8",
            "<html><head><script>var app = {};</script></head><body><div id=root></div></body></html>"
        ));
        assert!(!should_fall_back(200, "text/html", &article));
        assert!(!should_fall_back(404, "text/html", ""));
        assert!(!should_fall_back(200, "application/json", "{}"));
    }
}
