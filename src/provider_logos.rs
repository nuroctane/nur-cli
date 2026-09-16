//! Provider brand logos, embedded at compile time and rendered inline in
//! the TUI via the terminal graphics protocol (kitty / sixel / iTerm2),
//! with an emoji fallback for text-only surfaces (terminal tab titles).
//!
//! PNGs are 64x64, transparent, and optically standardized (every glyph
//! fits the same central box) under assets/provider-logos/. Regenerate all
//! of them with `python scripts/fetch_provider_logos.py`. Sources: the real
//! brand marks - simple-icons (Apache-2.0) for anthropic/deepseek/gemini/
//! googlecloud/kimi/meta/ollama/opencode/openrouter/perplexity/copilot, the
//! official OpenAI flower (simple-icons v9, tile path stripped), the
//! official xAI mark cropped from the Wikipedia render of their logo, the
//! Nous Research wordmark from their GitHub org avatar, and Cline's official
//! app icon (glyph only - the dark tile it sits on is dropped).

/// One provider logo: embedded PNG + emoji fallback.
#[derive(Clone, Copy)]
pub struct Logo {
    pub png: &'static [u8],
    /// Emoji fallback for text-only surfaces (consumed by the tab-title
    /// path via [`title_emoji`]; kept here so the mapping lives in one
    /// place).
    #[allow(dead_code)]
    pub emoji: &'static str,
}

const ANTHROPIC_PNG: &[u8] = include_bytes!("../assets/provider-logos/anthropic.png");
const CLINE_PNG: &[u8] = include_bytes!("../assets/provider-logos/cline.png");
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
    let emoji = title_emoji(provider_id);
    let png: &'static [u8] = match provider_id {
        "openai" | "openai-cc" => OPENAI_PNG,
        "anthropic" => ANTHROPIC_PNG,
        "google" | "google-oauth" | "antigravity" => GEMINI_PNG,
        "google-cloud" | "google-vertex" | "googlecloud" => GOOGLECLOUD_PNG,
        "xai" => XAI_PNG,
        "meta" => META_PNG,
        "deepseek" => DEEPSEEK_PNG,
        "kimi" => KIMI_PNG,
        "ollama" => OLLAMA_PNG,
        "opencode" => OPENCODE_PNG,
        "openrouter" => OPENROUTER_PNG,
        "perplexity" => PERPLEXITY_PNG,
        "github-copilot" | "copilot" => COPILOT_PNG,
        "nous" => NOUS_PNG,
        "cline" => CLINE_PNG,
        _ => return None,
    };
    Some(Logo { png, emoji })
}

/// Emoji for the terminal tab title, looked up by provider id OR the user
/// facing chrome label (`active_provider_chrome`: "claude", "grok",
/// "gemini", ...). Single source of truth for both surfaces - the id map
/// above and the chrome aliases must never disagree again. Empty string for
/// unknown providers (the title then shows only the moon marker).
pub fn title_emoji(label: &str) -> &'static str {
    match label {
        "openai" | "openai-cc" => "\u{1F7E2}",             // 🟢
        "anthropic" | "claude" => "\u{1F170}\u{FE0F}",     // 🅰️
        "google" | "google-oauth" | "antigravity" | "gemini" => "\u{2728}", // ✨
        "antigravity-cloud" => "\u{2728}",                 // ✨
        "xai" | "grok" => "\u{274C}",                      // ❌
        "meta" => "\u{267E}\u{FE0F}",                      // ♾️
        "deepseek" => "\u{1F40B}",                         // 🐋
        "kimi" => "\u{1F319}",                             // 🌙
        "ollama" => "\u{1F999}",                           // 🦙
        "opencode" => "\u{1F7E7}",                         // 🟧
        "openrouter" => "\u{1F6F0}\u{FE0F}",               // 🛰️
        "commandcode" => "\u{1F5A5}\u{FE0F}",              // 🖥️
        "perplexity" => "\u{1F50E}",                       // 🔎
        "github-copilot" | "copilot" => "\u{1F97D}",       // 🥽
        "nous" => "\u{1F318}",                             // 🌘
        // Cline's mark is a little agent head; the robot face reads the same
        // at tab-title size.
        "cline" => "\u{1F916}", // 🤖
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_emoji_covers_ids_and_chrome_aliases() {
        assert_eq!(title_emoji("openai"), "\u{1F7E2}");
        assert_eq!(title_emoji("claude"), title_emoji("anthropic"));
        assert_eq!(title_emoji("grok"), title_emoji("xai"));
        assert_eq!(title_emoji("gemini"), title_emoji("google"));
        assert_eq!(title_emoji("lmstudio"), "");
        assert_eq!(title_emoji("totally-unknown"), "");
    }

    #[test]
    fn every_embedded_logo_png_is_64x64() {
        let logos = [
            ANTHROPIC_PNG,
            CLINE_PNG,
            COPILOT_PNG,
            DEEPSEEK_PNG,
            GEMINI_PNG,
            GOOGLECLOUD_PNG,
            KIMI_PNG,
            META_PNG,
            NOUS_PNG,
            OLLAMA_PNG,
            OPENAI_PNG,
            OPENCODE_PNG,
            OPENROUTER_PNG,
            PERPLEXITY_PNG,
            XAI_PNG,
        ];
        for png in logos {
            // PNG IHDR: width/height are big-endian u32 at offsets 16/20.
            assert!(png.len() > 24, "truncated png");
            let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
            let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
            assert_eq!((w, h), (64, 64), "logo png must be standardized 64x64");
        }
    }

    #[test]
    fn for_provider_returns_logo_with_matching_emoji() {
        let logo = for_provider("deepseek").expect("deepseek has a logo");
        assert_eq!(logo.emoji, title_emoji("deepseek"));
        assert!(for_provider("unknown-gateway").is_none());
    }

    /// Cline is a first-class catalog row with an inline logo, and its tab-title
    /// emoji must resolve (an empty glyph would leave the title bare).
    #[test]
    fn cline_has_a_logo_and_a_title_emoji() {
        let logo = for_provider("cline").expect("cline has a logo");
        assert_eq!(logo.emoji, title_emoji("cline"));
        assert_eq!(title_emoji("cline"), "\u{1F916}");
        assert!(!logo.emoji.is_empty());
    }
}
