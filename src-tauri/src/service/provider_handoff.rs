#[cfg(test)]
mod tests;

use crate::agent::chat_types::ChatMsg;

const BYTES_PER_TOKEN: usize = 3;
const TOOL_LINE_LIMIT: usize = 8;
const TOOL_LINE_CHARS: usize = 320;

pub(crate) fn should_handoff_history(source: &str, target: &str, requested: bool) -> bool {
    requested && source != target
}

pub(crate) fn compose_launch_task(existing: Option<&str>, history: &str) -> String {
    let handoff = format!("Previous conversation handoff:\n{history}");
    match existing.filter(|task| !task.trim().is_empty()) {
        Some(task) => format!("{task}\n\n{handoff}"),
        None => handoff,
    }
}

pub(crate) fn reset_tab_identity_for_provider_change(
    tab: &mut crate::multiplexer::types::Tab,
    auto_approve: bool,
) {
    tab.restorable_launch = Default::default();
    tab.agent_planned_model = None;
    tab.agent_requested_model = None;
    tab.agent_provider_resolved_model = None;
    tab.agent_session_model = None;
    tab.agent_session_model_identity = None;
    tab.pending_model_switch_observation = None;
    tab.model_fallback = None;
    tab.model_identity_mismatch = None;
    tab.reasoning_effort_identity = Default::default();
    tab.auto_approve = auto_approve;
}

pub(crate) fn select_history(messages: &[ChatMsg], budget_tokens: usize) -> String {
    let budget_bytes = budget_tokens.saturating_mul(BYTES_PER_TOKEN);
    if budget_bytes == 0 {
        return String::new();
    }
    let first_goal = messages.iter().enumerate().find_map(|(index, message)| {
        let entry = render_entry(message)?;
        entry.starts_with("## User\n").then_some((index, entry))
    });
    let first_goal_index = first_goal.as_ref().map(|(index, _)| *index);
    let mut selected = Vec::new();
    let mut used_bytes = 0usize;
    if let Some((index, entry)) = first_goal {
        if let Some(fitted) = fit_entry(&entry, budget_bytes, false) {
            used_bytes = fitted.len();
            selected.push((index, fitted));
        }
    }
    for index in (0..messages.len()).rev() {
        if first_goal_index == Some(index) {
            continue;
        }
        let Some(entry) = render_entry(&messages[index]) else {
            continue;
        };
        let separator = if selected.is_empty() { 0 } else { 2 };
        let available = budget_bytes.saturating_sub(used_bytes + separator);
        if entry.len() <= available {
            used_bytes += separator + entry.len();
            selected.push((index, entry));
            continue;
        }
        if let Some(fitted) = fit_entry(&entry, available, true) {
            selected.push((index, fitted));
            break;
        }
    }
    selected.sort_by_key(|(index, _)| *index);
    selected
        .into_iter()
        .map(|(_, entry)| entry)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_entry(message: &ChatMsg) -> Option<String> {
    let kind = message.kind.to_ascii_lowercase();
    let is_tool = matches!(kind.as_str(), "tool" | "tool_use" | "tool_result")
        || message.tool_kind.is_some()
        || message.tool_summary.is_some()
        || message.tool_result.is_some();
    let (role, content) = if is_tool {
        (
            "Tool",
            message
                .tool_result
                .as_deref()
                .or(message.tool_summary.as_deref())
                .unwrap_or(&message.content),
        )
    } else if kind == "user" {
        ("User", message.content.as_str())
    } else if kind == "assistant" {
        ("Assistant", message.content.as_str())
    } else {
        return None;
    };
    let content = crate::service::launch_diagnostics::redact_secrets(content);
    let content = if is_tool {
        summarize_tool_output(&content)
    } else {
        content
    };
    let content = content.trim();
    (!content.is_empty()).then(|| format!("## {role}\n{content}"))
}

fn summarize_tool_output(text: &str) -> String {
    let mut lines = text.lines();
    let mut summary: Vec<String> = lines
        .by_ref()
        .take(TOOL_LINE_LIMIT)
        .map(truncate_line)
        .collect();
    if lines.next().is_some() {
        summary.push("… additional output omitted".into());
    }
    summary.join("\n")
}

fn truncate_line(line: &str) -> String {
    let mut chars = line.chars();
    let prefix: String = chars.by_ref().take(TOOL_LINE_CHARS).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn fit_entry(entry: &str, budget_bytes: usize, keep_tail: bool) -> Option<String> {
    if entry.len() <= budget_bytes {
        return Some(entry.to_string());
    }
    let (heading, content) = entry.split_once('\n')?;
    let content_budget = budget_bytes.checked_sub(heading.len() + 1)?;
    if content_budget <= '…'.len_utf8() {
        return None;
    }
    let text_budget = content_budget - '…'.len_utf8();
    let clipped = if keep_tail {
        format!("…{}", take_suffix_bytes(content, text_budget))
    } else {
        format!("{}…", take_prefix_bytes(content, text_budget))
    };
    Some(format!("{heading}\n{clipped}"))
}

fn take_prefix_bytes(text: &str, max_bytes: usize) -> &str {
    let end = text
        .char_indices()
        .map(|(index, character)| index + character.len_utf8())
        .take_while(|end| *end <= max_bytes)
        .last()
        .unwrap_or(0);
    &text[..end]
}

fn take_suffix_bytes(text: &str, max_bytes: usize) -> &str {
    let mut start = text.len().saturating_sub(max_bytes);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}
