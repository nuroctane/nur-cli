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
    let sk = SK.get_or_init(|| {
        regex::Regex::new(r"(?i)\bsk-[a-z0-9_-]{16,}").expect("sk pattern")
    });
    sk.is_match(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_bearer_and_pem() {
        assert!(body_looks_sensitive("Authorization: Bearer abcdef"));
        assert!(body_looks_sensitive("-----BEGIN PRIVATE KEY-----\nxx"));
        assert!(!body_looks_sensitive("just a normal tool log"));
        assert!(!body_looks_sensitive("task-manager output"));
        assert!(body_looks_sensitive("key=sk-abcdefghijklmnopqrstuvwxyz"));
    }
}
