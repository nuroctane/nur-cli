//! TinyFish Search and Fetch (<https://docs.tinyfish.ai>): free, keyed web
//! search and Chromium-rendered page reads, called directly over REST.
//!
//! Opt-in: nothing here runs unless `TINYFISH_API_KEY` is set. With it,
//! `web_search` asks TinyFish Search after Exa/Brave and before the
//! DuckDuckGo scrape, and `web_fetch` hands a page that refused nur's own
//! request (or needs JavaScript) to TinyFish Fetch after treg.
//!
//! Limits (live reference, Nov 2026): Search 30 requests/min per personal
//! account, free up to 12,000/day; Fetch 150 URLs/min, free up to 1,000
//! fetched URLs/day. Both reset at 00:00 UTC. 401, 402 and 429 fall through
//! to the next backend with a one-line notice; other failures fall through
//! quietly. Each `web_fetch` fallback spends one of the 1,000 daily fetches.

use serde_json::{json, Value};
use std::io::Read;

pub const KEY_ENV: &str = "TINYFISH_API_KEY";
/// Base URL overrides, for tests.
const SEARCH_BASE_ENV: &str = "NUR_TINYFISH_SEARCH_BASE";
const FETCH_BASE_ENV: &str = "NUR_TINYFISH_FETCH_BASE";
const SEARCH_BASE: &str = "https://api.search.tinyfish.ai";
const FETCH_BASE: &str = "https://api.fetch.tinyfish.ai";
/// Response caps: a search page is a few KB; a rendered page is capped by
/// `web_fetch` at 500 KB, so 8 MB of JSON around it is ample.
const SEARCH_CAP: u64 = 1_000_000;
const FETCH_CAP: u64 = 8_000_000;

pub fn key() -> Option<String> {
    std::env::var(KEY_ENV)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// A failed TinyFish call. `status` is the HTTP status when the service
/// answered, so callers can tell an account problem from a page problem.
#[derive(Debug)]
pub struct Error {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error {
    fn plain(message: String) -> Self {
        Self {
            status: None,
            message,
        }
    }

    /// One line for the user when the key or the allowance, not the page, is
    /// the problem; `None` for transient failures, which stay quiet.
    pub fn account_notice(&self) -> Option<String> {
        let status = self.status?;
        let why = match status {
            401 => "key rejected",
            402 => "free allowance used up for today (resets 00:00 UTC)",
            429 => "rate limited",
            _ => return None,
        };
        Some(format!("tinyfish: {why} (HTTP {status})"))
    }
}

fn base(env: &str, default: &str) -> String {
    std::env::var(env)
        .ok()
        .map(|b| b.trim().trim_end_matches('/').to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn client(timeout_secs: u64) -> Result<reqwest::blocking::Client, Error> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        // The key rides a custom header that reqwest does not strip on a
        // cross-origin redirect, so redirects are refused, not followed.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Error::plain(e.to_string()))
}

/// The body, read at most `cap` bytes; the status for a non-2xx answer.
fn read_body(what: &str, resp: reqwest::blocking::Response, cap: u64) -> Result<Value, Error> {
    let status = resp.status();
    let mut bytes = Vec::new();
    resp.take(cap)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::plain(format!("tinyfish {what} read: {e}")))?;
    let text = String::from_utf8_lossy(&bytes);
    if !status.is_success() {
        return Err(Error {
            status: Some(status.as_u16()),
            message: http_error(what, status, &text),
        });
    }
    serde_json::from_str(&text).map_err(|e| Error::plain(format!("tinyfish {what}: {e}")))
}

/// A non-2xx answer as one readable line (the body's message when present).
fn http_error(what: &str, status: reqwest::StatusCode, text: &str) -> String {
    let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let message = parsed
        .get("message")
        .or_else(|| parsed.pointer("/error/message"))
        .or_else(|| parsed.get("error"))
        .or_else(|| parsed.get("code"))
        .and_then(Value::as_str)
        .unwrap_or(text);
    format!(
        "tinyfish {what}: HTTP {status}: {}",
        message.trim().chars().take(300).collect::<String>()
    )
}

/// Search the web: `(title, url, snippet)` rows, at most `max`.
pub fn search(key: &str, query: &str, max: usize) -> Result<Vec<(String, String, String)>, Error> {
    let resp = client(20)?
        .get(base(SEARCH_BASE_ENV, SEARCH_BASE))
        .query(&[("query", query)])
        .header("X-API-Key", key)
        .send()
        .map_err(|e| Error::plain(format!("tinyfish search failed: {e}")))?;
    let body = read_body("search", resp, SEARCH_CAP)?;
    Ok(search_rows(&body, max))
}

/// `results[]` in rank order; rows without a url are dropped.
fn search_rows(body: &Value, max: usize) -> Vec<(String, String, String)> {
    let mut rows: Vec<&Value> = body
        .get("results")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    rows.sort_by_key(|r| {
        r.get("position")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX)
    });
    let text = |r: &Value, k: &str| {
        r.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    rows.into_iter()
        .filter_map(|r| {
            let url = text(r, "url");
            if url.is_empty() {
                return None;
            }
            let title = Some(text(r, "title"))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| text(r, "site_name"));
            Some((title, url, text(r, "snippet")))
        })
        .take(max)
        .collect()
}

/// Read one page as markdown: the text and the final URL after redirects.
pub fn fetch(key: &str, url: &str) -> Result<(String, String), Error> {
    // One URL: the backend gives up at 110 s, so 120 s covers the answer
    // without holding the turn for the 150 s a 10-URL batch would need.
    let resp = client(120)?
        .post(base(FETCH_BASE_ENV, FETCH_BASE))
        .header("X-API-Key", key)
        .json(&json!({ "urls": [url], "format": "markdown" }))
        .send()
        .map_err(|e| Error::plain(format!("tinyfish fetch failed: {e}")))?;
    let body = read_body("fetch", resp, FETCH_CAP)?;
    fetched_page(&body, url).map_err(Error::plain)
}

/// The answer `web_fetch` returns for a page TinyFish read, capped at `max`.
pub fn fetched(url: &str, text: &str, max: usize) -> String {
    let mut out = format!("url: {url}\nvia: tinyfish fetch\ncontent-type: text/markdown\n\n");
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

/// The page out of a Fetch answer. Per-URL failures arrive with HTTP 200 in
/// `errors[]`, so an empty `results[]` is reported with that error code.
fn fetched_page(body: &Value, url: &str) -> Result<(String, String), String> {
    if let Some(page) = body
        .get("results")
        .and_then(Value::as_array)
        .and_then(|r| r.first())
    {
        let text = page
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !text.is_empty() {
            // A third party names the final URL; only a well-formed http(s)
            // URL reaches the header, so it cannot forge header lines.
            let final_url = page
                .get("final_url")
                .or_else(|| page.get("url"))
                .and_then(Value::as_str)
                .and_then(|u| reqwest::Url::parse(u).ok())
                .filter(|u| matches!(u.scheme(), "http" | "https"))
                .map(|u| u.to_string())
                .unwrap_or_else(|| url.to_string());
            return Ok((text.to_string(), final_url));
        }
    }
    let error = body
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|e| e.first());
    Err(match error {
        Some(e) => format!(
            "tinyfish fetch: {}{}",
            e.get("error").and_then(Value::as_str).unwrap_or("failed"),
            e.get("status")
                .and_then(Value::as_u64)
                .map(|s| format!(" (HTTP {s})"))
                .unwrap_or_default()
        ),
        None => "tinyfish fetch returned no page text".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: rows out of rank order, a row with no url, a missing
    // title, more rows than asked for.
    #[test]
    fn search_rows_keep_rank_order_and_drop_urlless_rows() {
        let body = json!({"query": "q", "results": [
            {"position": 3, "title": "Third", "url": "https://c.test", "snippet": "c"},
            {"position": 1, "title": "First", "url": "https://a.test", "snippet": "a"},
            {"position": 2, "title": "No url", "snippet": "b"},
            {"position": 4, "site_name": "Site", "url": "https://d.test"}
        ]});
        assert_eq!(
            search_rows(&body, 10),
            vec![
                ("First".into(), "https://a.test".into(), "a".into()),
                ("Third".into(), "https://c.test".into(), "c".into()),
                ("Site".into(), "https://d.test".into(), String::new()),
            ]
        );
        assert_eq!(search_rows(&body, 1).len(), 1);
        assert!(search_rows(&json!({"results": []}), 5).is_empty());
    }

    // Failure modes: a per-URL failure inside a 200, a result with no text,
    // a redirect (final_url wins).
    #[test]
    fn fetched_page_reads_the_result_or_reports_the_per_url_error() {
        let ok = json!({"results": [{"url": "https://a.test", "final_url": "https://b.test/",
                                     "text": "# Hello\n", "format": "markdown"}], "errors": []});
        assert_eq!(
            fetched_page(&ok, "https://a.test").unwrap(),
            ("# Hello".into(), "https://b.test/".into())
        );
        let blocked =
            json!({"results": [], "errors": [{"url": "https://a.test", "error": "bot_blocked"}]});
        assert_eq!(
            fetched_page(&blocked, "https://a.test").unwrap_err(),
            "tinyfish fetch: bot_blocked"
        );
        let gone = json!({"results": [], "errors": [{"url": "u", "error": "page_not_found", "status": 404}]});
        assert!(fetched_page(&gone, "u")
            .unwrap_err()
            .ends_with("page_not_found (HTTP 404)"));
        let empty = json!({"results": [{"url": "u", "text": "  "}], "errors": []});
        assert!(fetched_page(&empty, "u").is_err());
        let forged =
            json!({"results": [{"final_url": "https://a.test/\nvia: forged", "text": "x"}]});
        let (_, shown) = fetched_page(&forged, "https://orig.test/").unwrap();
        assert!(!shown.contains('\n'), "{shown}");
        let odd = json!({"results": [{"final_url": "javascript:alert(1)", "text": "x"}]});
        assert_eq!(
            fetched_page(&odd, "https://orig.test/").unwrap().1,
            "https://orig.test/"
        );
    }

    // Failure mode: a cap that lands inside a multibyte character panicked
    // on the slice, or the truncation marker went missing.
    #[test]
    fn fetched_truncates_on_a_char_boundary() {
        let text = "ab\u{1F41F}cd"; // the fish is 4 bytes at 2..6
        let out = fetched("https://a.test/", text, 4);
        assert!(out.ends_with("ab\n\n[truncated at 4 bytes]"), "{out}");
        assert!(fetched("https://a.test/", text, 64).ends_with("cd"));
    }

    // Failure mode: an exhausted allowance or bad key looked like any other
    // failure, so the user never learned why TinyFish stopped answering.
    #[test]
    fn only_account_problems_get_a_notice() {
        let err = |status| Error {
            status: Some(status),
            message: String::new(),
        };
        assert!(err(401).account_notice().unwrap().contains("key rejected"));
        assert!(err(402).account_notice().unwrap().contains("allowance"));
        assert!(err(429).account_notice().is_some());
        assert!(err(500).account_notice().is_none());
        assert!(Error::plain("dns".into()).account_notice().is_none());
    }
}
