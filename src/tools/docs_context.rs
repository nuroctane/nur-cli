//! Pack explicitly registered documentation within a conservative budget.
use super::{arg_u64, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;

/// Count bytes this character occupies inside a JSON string. This is a
/// conservative bound for the byte-based tokenizers used by common providers.
fn encoded_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{08}' | '\u{0c}' => 2,
        c if c <= '\u{1f}' => 6,
        _ => c.len_utf8(),
    }
}

fn clip(text: &str, budget: usize) -> String {
    let mut used = 0usize;
    text.chars()
        .take_while(|c| {
            let next = used.saturating_add(encoded_len(*c));
            if next > budget {
                false
            } else {
                used = next;
                true
            }
        })
        .collect()
}

fn encoded_size(text: &str) -> usize {
    text.chars().map(encoded_len).sum()
}

pub fn pack(args: &Value, ctx: &ToolContext) -> Result<String> {
    let requested = arg_u64(args, "token_budget").unwrap_or(1500);
    let inline = crate::config::load_config()
        .map(|c| c.tool_result_max_chars as usize)
        .unwrap_or(0);
    let limit = if inline == 0 {
        crate::agent::context_store::MAX_READ_CHARS
    } else {
        inline.min(crate::agent::context_store::MAX_READ_CHARS)
    };
    let budget = usize::try_from(requested).unwrap_or(usize::MAX).min(limit);
    let names = args
        .get("names")
        .and_then(Value::as_array)
        .ok_or_else(|| NurError::Tool("context pack needs names".into()))?;
    if names.is_empty() || names.iter().any(|n| n.as_str().is_none_or(str::is_empty)) {
        return Err(NurError::Tool(
            "context pack names must be nonempty strings".into(),
        ));
    }
    let mut out = String::new();
    let mut used = 0usize;
    for (index, value) in names.iter().enumerate() {
        let name = value.as_str().unwrap_or_default();
        let share = budget.saturating_sub(used) / (names.len() - index);
        if share == 0 {
            break;
        }
        let var = crate::agent::context_store::get(&ctx.session_id, name)
            .ok_or_else(|| NurError::Tool(format!("unknown context var `{name}`")))?;
        let snippet = crate::agent::context_store::peek(&ctx.session_id, name, 0, share)
            .map_err(NurError::Tool)?;
        let body = snippet
            .split_once("\n---\n")
            .map(|(_, body)| body)
            .unwrap_or("");
        let header = format!("var=`{name}`\n");
        let full = format!("{header}{body}\n");
        let chunk = if body.chars().count() == var.char_count && encoded_size(&full) <= share {
            full
        } else {
            let suffix = "\n[truncated]\n";
            let reserved = encoded_size(&header) + encoded_size(suffix);
            if reserved <= share {
                format!("{header}{}{suffix}", clip(body, share - reserved))
            } else {
                clip(suffix, share)
            }
        };
        used += encoded_size(&chunk);
        out.push_str(&chunk);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::clip;

    #[test]
    fn clipping_respects_json_escaping_and_utf8_boundaries() {
        let clipped = clip("é\n\"\\世界", 9);
        let encoded = serde_json::to_string(&clipped).unwrap();
        assert!(encoded.len() - 2 <= 9);
        assert_eq!(clipped, "é\n\"\\");
    }
}
