use crate::agent::chat_types::ChatMsg;

pub const MAX_INPUT_BYTES: usize = 48_000;
pub const MAX_RECENT_MESSAGES: usize = 12;

pub fn select_history_literal(messages: &[ChatMsg], budget_bytes: usize) -> String {
    if budget_bytes < 2 {
        return String::new();
    }
    let tokens = budget_bytes.saturating_sub(2) / 6 / super::BYTES_PER_TOKEN;
    history_literal(&super::select_history(messages, tokens))
}

fn history_literal(history: &str) -> String {
    serde_json::to_string(history)
        .expect("string serialization cannot fail")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

pub struct HandoffFragments<'a> {
    pub instructions: &'a str,
    pub context: &'a str,
    pub history: &'a str,
}

pub fn assemble(fragments: HandoffFragments<'_>) -> Result<String, String> {
    let size = fragments
        .instructions
        .len()
        .saturating_add(fragments.context.len())
        .saturating_add(fragments.history.len())
        .saturating_add(4);
    if size > MAX_INPUT_BYTES {
        return Err("provider handoff exceeds the input size bound".into());
    }
    Ok([fragments.instructions, fragments.context, fragments.history].join("\n\n"))
}

pub fn recent_messages(messages: &[ChatMsg]) -> Vec<ChatMsg> {
    let mut selected: Vec<_> = messages
        .iter()
        .rev()
        .filter(|message| {
            matches!(message.kind.as_str(), "user" | "assistant")
                && message.tool_kind.is_none()
                && message.tool_result.is_none()
                && message.tool_summary.is_none()
                && !message.content.trim().is_empty()
        })
        .take(MAX_RECENT_MESSAGES)
        .cloned()
        .collect();
    selected.reverse();
    selected
}

#[cfg(test)]
mod tests;
