//! Provider brand logos, embedded at compile time and rendered inline in
//! the TUI via the terminal graphics protocol (kitty / sixel / iTerm2),
//! with an emoji fallback for text-only terminals.
//!
//! PNGs are 64x64 (brand color on transparent) under assets/provider-logos/.
//! Sources: simple-icons (Apache-2.0) for anthropic/deepseek/gemini/kimi/
//! meta/ollama/openrouter/perplexity/copilot/googlecloud; openai from
//! simple-icons v9 (official brand teal); opencode favicon; xai + nous
//! drawn in-repo.

/// One provider logo: embedded PNG + emoji fallback.
#[derive(Clone, Copy)]
pub struct Logo {
    pub png: &'static [u8],
    /// Emoji fallback for text-only terminals (consumed by the tab-title
    /// path; kept here so the mapping lives in one place).
    #[allow(dead_code)]
    pub emoji: &'static str,
}

const ANTHROPIC_PNG: &[u8] = include_bytes!("../assets/provider-logos/anthropic.png");
const COPILOT_PNG: &[u8] = include_bytes!("../assets/provider-logos/copilot.png");
const DEEPSEEK_PNG: &[u8] = include_bytes!("../assets/provider-logos/deepseek.png");
const GEMINI_PNG: &[u8] = include_bytes!("../assets/provider-logos/gemini.png");
const GOOGLECLOUD_PNG: &[u8] = include_bytes!("../assets/provider-logos/googlecloud.png");
const KIMI_PNG: &[u8] = include_bytes!("../assets/provider-logos/kimi.png");
const META_PNG: &[u8] = include_bytes!("../assets/provider-logos/meta.png");
const NOUS_PNG: &[u8] = include_bytes!("../assets/provider-logos/nous.png");
const OLLAMA_PNG: &[u8] = include_bytes!("../assets/provider-logos/ollama.png");
const OPENAI_PNG: &[u8] = include_bytes!("../assets/provider-logos/openai.png");
const OPENCODE_PNG: &[u8] = include_bytes!("../assets/provider-logos/opencode.png");
const OPENROUTER_PNG: &[u8] = include_bytes!("../assets/provider-logos/openrouter.png");
const PERPLEXITY_PNG: &[u8] = include_bytes!("../assets/provider-logos/perplexity.png");
const XAI_PNG: &[u8] = include_bytes!("../assets/provider-logos/xai.png");

/// Map a catalog provider id to its logo. Unlisted ids (local servers,
/// niche gateways) fall back to a generic orbit glyph.
pub fn for_provider(provider_id: &str) -> Option<Logo> {
    let (png, emoji): (&'static [u8], &'static str) = match provider_id {
        "openai" | "openai-cc" => (OPENAI_PNG, "🟢"),
        "anthropic" => (ANTHROPIC_PNG, "🅰"),
        "google" | "google-oauth" => (GEMINI_PNG, "✦"),
        "antigravity" => (GOOGLECLOUD_PNG, "✦"),
        "xai" => (XAI_PNG, "✕"),
        "meta" => (META_PNG, "∞"),
        "deepseek" => (DEEPSEEK_PNG, "🐋"),
        "kimi" => (KIMI_PNG, "🌙"),
        "ollama" => (OLLAMA_PNG, "🦙"),
        "opencode" => (OPENCODE_PNG, "⬛"),
        "openrouter" => (OPENROUTER_PNG, "🛰"),
        "perplexity" => (PERPLEXITY_PNG, "🔎"),
        "github-copilot" | "copilot" => (COPILOT_PNG, "🪁"),
        "nous" => (NOUS_PNG, "🌘"),
        _ => return None,
    };
    Some(Logo { png, emoji })
}
