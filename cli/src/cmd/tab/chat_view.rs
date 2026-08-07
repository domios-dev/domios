/// `-=-codeterm:tab label=X tab=Y ...-=-body` → (Some("X"), "body"). Plain text → (None, text).
fn strip_origin_marker(s: &str) -> (Option<String>, String) {
    if let Some(rest) = s.strip_prefix("-=-codeterm:") {
        if let Some(end) = rest.find("-=-") {
            let label = rest[..end]
                .split_whitespace()
                .find_map(|kv| kv.strip_prefix("label="))
                .map(|l| l.replace("%20", " "));
            return (label, rest[end + 3..].to_string());
        }
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

/// Human-readable transcript: one capped line per message, tool calls show
/// the command, tool results show their output, origin markers become `(from X)`.
pub(super) fn print_tab_chat(v: &serde_json::Value) {
    let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) else {
        println!("(no chat transcript)");
        return;
    };
    if msgs.is_empty() {
        println!("(no chat messages yet)");
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
