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

pub(crate) fn select_history(messages: &[ChatMsg], budget_tokens: usize) -> String {
    let budget_bytes = budget_tokens.saturating_mul(BYTES_PER_TOKEN);
    if budget_bytes == 0 {
        return String::new();
    }
    let entries: Vec<Option<String>> = messages.iter().map(render_entry).collect();
    let first_goal = messages.iter().enumerate().find_map(|(index, message)| {
        (message.kind.eq_ignore_ascii_case("user") && entries[index].is_some()).then_some(index)
    });
    let mut selected = vec![None; messages.len()];
    let mut used_bytes = 0usize;
    if let Some(index) = first_goal {
        if let Some(entry) = entries[index].as_ref() {
            let fitted = fit_entry(entry, budget_bytes, false);
            if let Some(fitted) = fitted {
                used_bytes = fitted.len();
                selected[index] = Some(fitted);
            }
        }
    }
    for index in (0..messages.len()).rev() {
        if selected[index].is_some() {
            continue;
        }
        let Some(entry) = entries[index].as_ref() else {
            continue;
        };
        let separator = if selected.iter().any(Option::is_some) {
            2
        } else {
            0
        };
        let available = budget_bytes.saturating_sub(used_bytes + separator);
        if entry.len() <= available {
            used_bytes += separator + entry.len();
            selected[index] = Some(entry.clone());
            continue;
        }
        if let Some(fitted) = fit_entry(entry, available, true) {
            used_bytes += separator + fitted.len();
            selected[index] = Some(fitted);
            break;
        }
    }
    selected
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_entry(message: &ChatMsg) -> Option<String> {
    let kind = message.kind.to_ascii_lowercase();
    let (role, content, is_tool) = if kind == "user" {
        ("User", message.content.as_str(), false)
    } else if kind == "assistant" {
        ("Assistant", message.content.as_str(), false)
    } else if matches!(kind.as_str(), "tool" | "tool_use" | "tool_result")
        || message.tool_kind.is_some()
        || message.tool_result.is_some()
    {
        (
            "Tool",
            message
                .tool_summary
                .as_deref()
                .or(message.tool_result.as_deref())
                .unwrap_or(&message.content),
            true,
        )
    } else {
        return None;
    };
    let content = if is_tool {
        summarize_tool_output(content)
    } else {
        content.to_string()
    };
    let content = crate::service::launch_diagnostics::redact_secrets(&content);
    let content = content.trim();
    (!content.is_empty()).then(|| format!("## {role}\n{content}"))
}

fn summarize_tool_output(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut summary: Vec<String> = lines
        .iter()
        .take(TOOL_LINE_LIMIT)
        .map(|line| truncate_line(line))
        .collect();
    if lines.len() > TOOL_LINE_LIMIT {
        summary.push(format!("… {} lines omitted", lines.len() - TOOL_LINE_LIMIT));
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
