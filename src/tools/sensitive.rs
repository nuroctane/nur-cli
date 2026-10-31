//! Shared heuristics for payloads that must not be spilled / compressed casually.

/// True when `body` looks like it contains credentials or private key material.
pub fn body_looks_sensitive(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "api_key",
        "apikey",
        "api-key",
        "secret_key",
        "secretkey",
        "access_token",
        "refresh_token",
        "bearer ",
        "authorization:",
        "-----begin ",
        "private_key",
        "privatekey",
        "client_secret",
        "openai_api_key",
        "anthropic_api_key",
        "xai_api_key",
        "aws_secret",
        "sk-ant-",
        "sk-proj-",
        "ghp_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "akia",
        "aws_access_key_id",
    ];
    if NEEDLES.iter().any(|n| lower.contains(n)) {
        return true;
    }
    // OpenAI-style keys: `sk-` + >=16 alnum (avoids false positives like "task-").
    static SK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let sk = SK.get_or_init(|| regex::Regex::new(r"(?i)\bsk-[a-z0-9_-]{16,}").expect("sk pattern"));
    sk.is_match(body)
}

/// Secret material in what an edit tool is about to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteSecret {
    /// A private key block (header plus key material): the write is refused.
    PrivateKey,
    /// A live-looking credential of this kind: the write goes ahead with a warning.
    Token(&'static str),
}

/// The text an edit tool call adds to a file: `content`, each `new_string`,
/// and the added lines of a patch or hashline body. What it removes
/// (`old_string`, `-` lines) is not new material, so deleting a key from a
/// file is never blocked. Returned as JSON so [`secret_in_write`] sees the
/// same escaping in every case.
pub fn written_text(tool: &str, arguments: &str) -> String {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return arguments.to_string();
    };
    let s = |v: &serde_json::Value, k: &str| {
        v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    let added_lines = |text: String| -> String {
        text.lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .map(|l| &l[1..])
            .collect::<Vec<_>>()
            .join("\n")
    };
    let text = match tool {
        // `contents` is write_file's argument; `content` is what models often send.
        "write_file" => {
            let contents = s(&args, "contents");
            if contents.is_empty() {
                s(&args, "content")
            } else {
                contents
            }
        }
        "edit_file" => s(&args, "new_string"),
        "multi_edit" => args
            .get("edits")
            .and_then(|e| e.as_array())
            .map(|edits| {
                edits
                    .iter()
                    .map(|e| s(e, "new_string"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "apply_patch" => added_lines(if args.get("patch").is_some() {
            s(&args, "patch")
        } else {
            s(&args, "input")
        }),
        "hashline_edit" => added_lines(s(&args, "edits")),
        _ => return arguments.to_string(),
    };
    serde_json::to_string(&text).unwrap_or_default()
}

/// Scan an edit tool's raw arguments (JSON, so newlines arrive as `\n`).
/// [`body_looks_sensitive`] matches words like `api_key` that appear in
/// ordinary code; this matches only the values themselves, so writing code
/// or docs that talk about keys is never flagged, and placeholders such as
/// `sk-xxxx...` are skipped.
pub fn secret_in_write(arguments: &str) -> Option<WriteSecret> {
    if has_private_key(arguments) {
        return Some(WriteSecret::PrivateKey);
    }
    live_tokens(arguments)
        .first()
        .map(|token| WriteSecret::Token(token.kind))
}

/// A live-looking credential found in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundToken {
    /// What it is ("GitHub token").
    pub kind: &'static str,
    /// The environment variable such a credential usually lives in.
    pub env_name: &'static str,
    /// Byte range in the text.
    pub range: std::ops::Range<usize>,
}

/// Every live-looking credential in `text`, in order; placeholders such as
/// `sk-xxxx…` are skipped.
pub fn live_tokens(text: &str) -> Vec<FoundToken> {
    use std::sync::OnceLock;
    type Pattern = (&'static str, &'static str, regex::Regex);
    static TOKENS: OnceLock<Vec<Pattern>> = OnceLock::new();
    let tokens = TOKENS.get_or_init(|| {
        [
            (
                "Anthropic API key",
                "ANTHROPIC_API_KEY",
                r"\bsk-ant-[A-Za-z0-9_-]{32,}",
            ),
            (
                "OpenRouter API key",
                "OPENROUTER_API_KEY",
                r"\bsk-or-v1-[A-Za-z0-9]{32,}",
            ),
            (
                "OpenAI API key",
                "OPENAI_API_KEY",
                r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}",
            ),
            (
                "GitHub token",
                "GITHUB_TOKEN",
                r"\b(?:gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{50,})",
            ),
            (
                "GitLab token",
                "GITLAB_TOKEN",
                r"\bglpat-[A-Za-z0-9_-]{20,}",
            ),
            (
                "Slack token",
                "SLACK_TOKEN",
                r"\bxox[abprs]-[A-Za-z0-9-]{20,}",
            ),
            (
                "AWS access key",
                "AWS_ACCESS_KEY_ID",
                r"\bAKIA[0-9A-Z]{16}\b",
            ),
            (
                "Google API key",
                "GOOGLE_API_KEY",
                r"\bAIza[0-9A-Za-z_-]{35}",
            ),
            (
                "Stripe key",
                "STRIPE_API_KEY",
                r"\b[rs]k_live_[A-Za-z0-9]{24,}",
            ),
            ("Hugging Face token", "HF_TOKEN", r"\bhf_[A-Za-z0-9]{30,}"),
            ("npm token", "NPM_TOKEN", r"\bnpm_[A-Za-z0-9]{36}\b"),
            ("xAI API key", "XAI_API_KEY", r"\bxai-[A-Za-z0-9]{40,}"),
        ]
        .into_iter()
        .map(|(kind, env, pattern)| {
            (
                kind,
                env,
                regex::Regex::new(pattern).expect("token pattern"),
            )
        })
        .collect()
    });
    let mut found: Vec<FoundToken> = Vec::new();
    for (kind, env_name, pattern) in tokens {
        for m in pattern.find_iter(text) {
            // The first pattern to claim a span names it (sk-ant- before sk-).
            let taken = found
                .iter()
                .any(|f| f.range.start < m.end() && m.start() < f.range.end);
            if !taken && looks_random(m.as_str()) {
                found.push(FoundToken {
                    kind,
                    env_name,
                    range: m.range(),
                });
            }
        }
    }
    found.sort_by_key(|f| f.range.start);
    found
}

/// A PEM or PGP private key block with a body: the header, then at least
/// 40 base64 characters on body lines. Line breaks may arrive raw, JSON-
/// escaped once (`\n`) or twice (`\\n`, a key inside a string literal), or
/// as `\u000a`; armor headers (`Proc-Type:`, `DEK-Info:`, `Version:`) and
/// short wrapped lines are allowed. A header with no body is a mention.
fn has_private_key(text: &str) -> bool {
    use std::sync::OnceLock;
    static HEADER: OnceLock<regex::Regex> = OnceLock::new();
    let header = HEADER.get_or_init(|| {
        regex::Regex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----")
            .expect("private key header")
    });
    header.find_iter(text).any(|m| {
        let mut end = (m.end() + 2_000).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut body = text[m.end()..end].to_string();
        for escaped in [
            "\\\\r\\\\n",
            "\\\\n",
            "\\\\r",
            "\\r\\n",
            "\\n",
            "\\r",
            "\\u000a",
            "\\u000A",
        ] {
            body = body.replace(escaped, "\n");
        }
        let mut base64 = 0;
        for line in body.lines() {
            let line = line.trim_matches(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | '+' | ',' | ';')
            });
            if line.starts_with("-----END") {
                break;
            }
            if line.is_empty() || line.contains(':') {
                continue;
            }
            if !line
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
            {
                break;
            }
            base64 += line.len();
        }
        base64 >= 40
    })
}

/// Real keys are random; `sk-xxxxxxxx…` and `AKIAEXAMPLEEXAMPLE` are not.
fn looks_random(token: &str) -> bool {
    let distinct: std::collections::HashSet<char> = token.chars().collect();
    let lower = token.to_ascii_lowercase();
    distinct.len() >= 12 && !lower.contains("example") && !lower.contains("xxxx")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: a real private key written to disk; code or docs that
    // only mention a key header refused; a live token written silently; a
    // placeholder token flagged.
    #[test]
    fn private_keys_are_refused_and_mentions_are_not() {
        // Assembled from parts so the source holds no literal key block for
        // secret scanners to trip over.
        let (begin, end) = ("-----BEGIN OPENSSH ", "-----END OPENSSH ");
        let key = format!(
            r#"{{"path":"id","contents":"{begin}PRIVATE KEY-----\n{}\n{end}PRIVATE KEY-----"}}"#,
            "b3BlbnNzaC1yZWFsLWxvb2tpbmctZml4dHVyZS1ub3QtYS1rZXktMDEyMzQ1Njc4OWFi"
        );
        assert_eq!(secret_in_write(&key), Some(WriteSecret::PrivateKey));
        let mention = r#"{"path":"a.rs","contents":"if line.starts_with(\"-----BEGIN RSA PRIVATE KEY-----\") {"}"#;
        assert_eq!(secret_in_write(mention), None);
    }

    // The common hard-coded forms: a key inside a string literal (its `\n`
    // arrives doubly escaped), an encrypted PEM with armor headers, PGP
    // armor, and bodies wrapped narrower than 40 columns.
    #[test]
    fn escaped_and_armored_keys_are_caught() {
        let body = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7";
        let begin = "-----BEGIN ";
        let literal =
            format!("let k = \"{begin}PRIVATE KEY-----\\n{body}\\n-----END PRIVATE KEY-----\";");
        let args = serde_json::json!({"path": "k.rs", "contents": literal}).to_string();
        assert_eq!(
            secret_in_write(&written_text("write_file", &args)),
            Some(WriteSecret::PrivateKey)
        );
        let encrypted = format!(
            "{begin}RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-128-CBC,0A1B\n\n{}\n{}\n",
            &body[..30],
            &body[30..]
        );
        let args = serde_json::json!({"path": "k.pem", "contents": encrypted}).to_string();
        assert_eq!(
            secret_in_write(&written_text("write_file", &args)),
            Some(WriteSecret::PrivateKey)
        );
        let pgp = format!("{begin}PGP PRIVATE KEY BLOCK-----\nVersion: GnuPG v2\n\n{body}\n");
        let args = serde_json::json!({"path": "k.asc", "contents": pgp}).to_string();
        assert_eq!(
            secret_in_write(&written_text("write_file", &args)),
            Some(WriteSecret::PrivateKey)
        );
        let unicode = format!("{begin}PRIVATE KEY-----\\u000a{body}");
        assert_eq!(secret_in_write(&unicode), Some(WriteSecret::PrivateKey));
    }

    #[test]
    fn removing_a_key_is_not_writing_one() {
        let (begin, end) = ("-----BEGIN OPENSSH ", "-----END OPENSSH ");
        let key = format!(
            "{begin}PRIVATE KEY-----\nb3BlbnNzaC1yZWFsLWxvb2tpbmctZml4dHVyZS1ub3QtYS1rZXktMDEyMzQ1Njc4OWFi\n{end}PRIVATE KEY-----"
        );
        let removal =
            serde_json::json!({"path": "id", "old_string": key, "new_string": ""}).to_string();
        assert_eq!(secret_in_write(&written_text("edit_file", &removal)), None);
        let added = serde_json::json!({"path": "id", "contents": key}).to_string();
        assert_eq!(
            secret_in_write(&written_text("write_file", &added)),
            Some(WriteSecret::PrivateKey)
        );
    }

    #[test]
    fn live_tokens_warn_and_placeholders_do_not() {
        let live = r#"{"contents":"OPENAI_API_KEY=sk-proj-Q7vLm2Xa9Pz4Rt8Kw1Hy6Ub3Nc5Jd0Fe"}"#;
        assert_eq!(
            secret_in_write(live),
            Some(WriteSecret::Token("OpenAI API key"))
        );
        let placeholder =
            r#"{"contents":"OPENAI_API_KEY=sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}"#;
        assert_eq!(secret_in_write(placeholder), None);
        let docs_example = r#"{"contents":"AKIAIOSFODNN7EXAMPLE"}"#;
        assert_eq!(secret_in_write(docs_example), None);
        let code = r#"{"contents":"let api_key = std::env::var(\"OPENAI_API_KEY\")?;"}"#;
        assert_eq!(secret_in_write(code), None);
    }

    #[test]
    fn catches_bearer_and_pem() {
        assert!(body_looks_sensitive("Authorization: Bearer abcdef"));
        assert!(body_looks_sensitive("-----BEGIN PRIVATE KEY-----\nxx"));
        assert!(!body_looks_sensitive("just a normal tool log"));
        assert!(!body_looks_sensitive("task-manager output"));
        assert!(body_looks_sensitive("key=sk-abcdefghijklmnopqrstuvwxyz"));
    }
}
