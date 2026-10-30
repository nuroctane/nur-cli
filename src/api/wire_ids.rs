//! Tool-call ids, fitted to the protocol they are replayed to.
//!
//! History keeps each call id exactly as the model that made it wrote it, and
//! vendors disagree on what an id may be:
//!
//! - OpenAI rejects ids longer than 40 characters;
//! - Anthropic accepts only `[A-Za-z0-9_-]`, so Kimi's `functions.read_file:0`
//!   is a 400 there;
//! - Mistral accepts exactly nine alphanumerics.
//!
//! An id that already fits passes through unchanged. One that does not is
//! replaced by a stable hash of itself, so a call and its output, rewritten
//! independently, still pair, and the same history maps the same way on every
//! request (prompt caches see a stable prefix).

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdRule {
    /// OpenAI and every OpenAI-compatible route: at most 40 characters.
    Openai,
    /// Anthropic Messages: `[A-Za-z0-9_-]`, at most 40 characters here.
    Anthropic,
    /// Mistral: exactly nine `[A-Za-z0-9]`.
    Mistral,
}

const MAX_LEN: usize = 40;

impl IdRule {
    /// The rule for a Chat Completions route.
    pub fn for_chat(provider_id: &str, base_url: &str) -> Self {
        if provider_id == "mistral" || base_url.contains("api.mistral.ai") {
            IdRule::Mistral
        } else {
            IdRule::Openai
        }
    }

    fn accepts(self, id: &str) -> bool {
        match self {
            IdRule::Openai => id.chars().count() <= MAX_LEN,
            IdRule::Anthropic => {
                id.len() <= MAX_LEN
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            }
            IdRule::Mistral => id.len() == 9 && id.bytes().all(|b| b.is_ascii_alphanumeric()),
        }
    }
}

/// `id` as `rule` accepts it. Empty ids are left alone: pairing repairs them
/// before anything is sent (`agent::loop::normalize_call_ids`).
pub fn fit(id: &str, rule: IdRule) -> String {
    if id.is_empty() || rule.accepts(id) {
        return id.to_string();
    }
    let digest = Sha256::digest(id.as_bytes());
    let token = base62(&digest);
    match rule {
        IdRule::Mistral => token[..9].to_string(),
        IdRule::Openai | IdRule::Anthropic => format!("call_{}", &token[..24]),
    }
}

fn base62(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    bytes
        .iter()
        .flat_map(|b| {
            [
                ALPHABET[(*b % 62) as usize],
                ALPHABET[((*b / 62) % 62) as usize],
            ]
        })
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: a foreign id the vendor rejects; a call and its output
    // mapped to different ids; two distinct ids mapped to one; an id that
    // already works being rewritten (busts the prompt cache for nothing).

    #[test]
    fn foreign_ids_fit_each_vendor() {
        let kimi = "functions.read_file:0";
        let anthropic = fit(kimi, IdRule::Anthropic);
        assert!(IdRule::Anthropic.accepts(&anthropic), "{anthropic}");
        let mistral = fit("toolu_01A09q90qw90lq917835lq9", IdRule::Mistral);
        assert!(IdRule::Mistral.accepts(&mistral), "{mistral}");
        let long = "call_".to_string() + &"x".repeat(60);
        assert!(IdRule::Openai.accepts(&fit(&long, IdRule::Openai)));
    }

    #[test]
    fn mapping_is_stable_and_keeps_ids_apart() {
        for rule in [IdRule::Openai, IdRule::Anthropic, IdRule::Mistral] {
            let a = fit("functions.read_file:0", rule);
            assert_eq!(a, fit("functions.read_file:0", rule));
            assert_ne!(a, fit("functions.read_file:1", rule));
        }
    }

    #[test]
    fn ids_that_already_fit_are_untouched() {
        assert_eq!(fit("call_abc123", IdRule::Openai), "call_abc123");
        assert_eq!(
            fit("toolu_01A09q90qw9", IdRule::Anthropic),
            "toolu_01A09q90qw9"
        );
        assert_eq!(fit("D681PevKs", IdRule::Mistral), "D681PevKs");
        assert_eq!(fit("", IdRule::Mistral), "");
    }
}
