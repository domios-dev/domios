/// `<domios from="tab" label="X" ...>body</domios>` → (Some("X"), "body").
/// Plain text or an embedded tag remains unchanged.
fn strip_origin_marker(s: &str) -> (Option<String>, String) {
    if let Some(parsed) = codeterm_wire::domios::parse_message(s) {
        let label = parsed
            .attr("label")
            .or_else(|| parsed.attr("source"))
            .map(str::to_string);
        return (label, parsed.body);
    }
    (None, s.to_string())
}

/// One-line, length-capped view of a message body so a single huge blob
/// (skill context, AGENTS.md injection, big tool output) can't swamp the
/// transcript and hide the real turns.
fn truncate_for_chat(s: &str, max: usize) -> String {
    let oneline = s.trim().replace('\n', " ⏎ ");
    let n = oneline.chars().count();
    if n <= max {
        return oneline;
    }
    let cut: String = oneline.chars().take(max).collect();
    format!("{}… [+{} chars]", cut, n - max)
}

/// The line saying a transcript belongs to a tab that no longer exists.
fn closed_notice(v: &serde_json::Value) -> Option<String> {
    if v.get("closed").and_then(|closed| closed.as_bool()) != Some(true) {
        return None;
    }
    let text = |key: &str| {
        v.get("tab")
            .and_then(|tab| tab.get(key))
            .and_then(|value| value.as_str())
            .unwrap_or("?")
    };
    let truncated = v.get("truncated").and_then(|flag| flag.as_bool()) == Some(true);
    Some(format!(
        "(tab {} \"{}\" is closed; read-only transcript{})",
        text("tab_id"),
        text("title"),
        if truncated {
            ", older messages omitted"
        } else {
            ""
        }
    ))
}

/// Human-readable transcript: one capped line per message, tool calls show
/// the command, tool results show their output, origin markers become `(from X)`.
pub(crate) fn print_tab_chat(v: &serde_json::Value) {
    let closed = closed_notice(v);
    if let Some(notice) = &closed {
        println!("{notice}");
    }
    let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) else {
        println!("(no chat transcript)");
        return;
    };
    if msgs.is_empty() {
        println!(
            "{}",
            if closed.is_some() {
                "(no retained messages)"
            } else {
                "(no chat messages yet)"
            }
        );
        return;
    }
    for m in msgs {
        let role = m.get("type").and_then(|t| t.as_str()).unwrap_or("msg");
        let tool_args = m.get("toolArgs").and_then(|t| t.as_str());
        let tool_result = m.get("toolResult").and_then(|t| t.as_str());
        let content = m.get("content").and_then(|t| t.as_str()).unwrap_or("");

        let (label, origin, raw) = if let Some(ta) = tool_args {
            ("⚙ tool", None, ta.to_string())
        } else if let Some(tr) = tool_result {
            ("  ↳ result", None, tr.to_string())
        } else {
            let (origin, body) = strip_origin_marker(content);
            let label = match role {
                "assistant" => "● assistant",
                "thinking" => "  ∴ thinking",
                "system" => "  · system",
                _ => "▸ user",
            };
            (label, origin, body)
        };

        let body = truncate_for_chat(&raw, 280);
        if body.is_empty() {
            continue;
        }
        match origin {
            Some(from) => println!("{} (from {}) {}", label, from, body),
            None => println!("{} {}", label, body),
        }
    }
}
