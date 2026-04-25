//! Response capture — polls agent output after sending a message.

use std::time::{Duration, Instant};
use crate::agent::status::PaneStatus;
use crate::{AgentTrackerState, OutputBufferState};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub fn capture_response(
    pty_id: &str,
    output_buf: &OutputBufferState,
    tracker: &AgentTrackerState,
) -> Result<String, String> {
    let offset = output_buf.lock()
        .get_full(pty_id).map(|b| b.len()).unwrap_or(0);

    let deadline = Instant::now() + RESPONSE_TIMEOUT;
    let mut saw_working = false;

    loop {
        if Instant::now() > deadline {
            if saw_working { break; }
            return Err("Response timeout".to_string());
        }
        let status = tracker.lock().get_status(pty_id);
        match status {
            PaneStatus::Working => { saw_working = true; }
            PaneStatus::Idle if saw_working => {
                std::thread::sleep(Duration::from_millis(300));
                break;
            }
            PaneStatus::Dead => return Err("Agent died".to_string()),
            _ => {}
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    let raw = output_buf.lock()
        .get_full(pty_id)
        .map(|b| if offset < b.len() { String::from_utf8_lossy(&b[offset..]).to_string() } else { String::new() })
        .unwrap_or_default();

    if raw.is_empty() { return Err("No response".to_string()); }
    Ok(strip_ansi(&raw))
}

pub fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some('[') => { chars.next(); while let Some(&c) = chars.peek() { chars.next(); if c.is_ascii_alphabetic() || c == '~' { break; } } }
                Some(']') => { chars.next(); while let Some(&c) = chars.peek() { chars.next(); if c == '\x07' { break; } if c == '\x1b' { chars.next(); break; } } }
                _ => { chars.next(); }
            }
        } else if ch != '\r' { result.push(ch); }
    }
    result.trim().to_string()
}

/// Find the largest byte index <= `max_bytes` that lies on a char boundary.
fn floor_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() { return s.len(); }
    let mut i = max_bytes;
    while i > 0 && !s.is_char_boundary(i) { i -= 1; }
    i
}

pub fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len { return vec![text.to_string()]; }
    let tokens = super::sanitize::tokenize(text);

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prefix_for_next = String::new();
    let mut stack: Vec<(String, String)> = Vec::new();

    let close_overhead = |stack: &Vec<(String, String)>| -> usize {
        stack.iter().map(|(n, _)| n.len() + 3).sum()
    };

    let flush = |current: &mut String, prefix_for_next: &mut String, stack: &Vec<(String, String)>, chunks: &mut Vec<String>| {
        for (n, _) in stack.iter().rev() {
            current.push_str("</");
            current.push_str(n);
            current.push('>');
        }
        chunks.push(std::mem::take(current));
        prefix_for_next.clear();
        for (_, raw) in stack.iter() {
            prefix_for_next.push_str(raw);
        }
        current.push_str(prefix_for_next);
    };

    for tok in tokens {
        match tok {
            super::sanitize::Token::Text(t) => {
                let mut remaining = t.as_str();
                while !remaining.is_empty() {
                    let avail = max_len.saturating_sub(current.len() + close_overhead(&stack));
                    if avail == 0 {
                        flush(&mut current, &mut prefix_for_next, &stack, &mut chunks);
                        continue;
                    }
                    if remaining.len() <= avail {
                        current.push_str(remaining);
                        break;
                    }
                    let safe_end = floor_char_boundary(remaining, avail);
                    if safe_end == 0 {
                        let first_char_end = remaining.char_indices().nth(1).map(|(i, _)| i).unwrap_or(remaining.len());
                        if !current.is_empty() {
                            flush(&mut current, &mut prefix_for_next, &stack, &mut chunks);
                            continue;
                        }
                        current.push_str(&remaining[..first_char_end]);
                        remaining = &remaining[first_char_end..];
                        continue;
                    }
                    let slice = &remaining[..safe_end];
                    let break_at = slice.rfind('\n')
                        .or_else(|| slice.rfind(' '))
                        .map(|p| p + 1)
                        .unwrap_or(safe_end);
                    let break_at = if break_at == 0 { safe_end } else { break_at };
                    current.push_str(&remaining[..break_at]);
                    remaining = remaining[break_at..].trim_start_matches('\n');
                    if !remaining.is_empty() {
                        flush(&mut current, &mut prefix_for_next, &stack, &mut chunks);
                    }
                }
            }
            super::sanitize::Token::Open { name, raw } => {
                if current.len() + raw.len() + close_overhead(&stack) + name.len() + 3 > max_len && !current.is_empty() {
                    flush(&mut current, &mut prefix_for_next, &stack, &mut chunks);
                }
                current.push_str(&raw);
                stack.push((name, raw));
            }
            super::sanitize::Token::Close { name, raw } => {
                if let Some(pos) = stack.iter().rposition(|(n, _)| *n == name) {
                    while stack.len() > pos + 1 {
                        let (n, _) = stack.pop().unwrap();
                        current.push_str("</");
                        current.push_str(&n);
                        current.push('>');
                    }
                    stack.pop();
                }
                current.push_str(&raw);
            }
        }
    }

    if !current.is_empty() || !stack.is_empty() {
        for (n, _) in stack.iter().rev() {
            current.push_str("</");
            current.push_str(n);
            current.push('>');
        }
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi("hello"), "hello");
    }

    #[test]
    fn test_chunk() {
        let chunks = chunk_message(&"a".repeat(300), 100);
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_chunk_at_newline() {
        let text = format!("{}\n{}", "a".repeat(100), "b".repeat(100));
        let chunks = chunk_message(&text, 150);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_chunk_multibyte_utf8() {
        // Russian text: each Cyrillic char is 2 bytes in UTF-8
        let text = "Привет ".repeat(600); // ~4200 bytes
        let chunks = chunk_message(&text, 100);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 100);
            // Verify all chunks are valid UTF-8 (would panic if not)
            let _ = chunk.as_str();
        }
    }

    #[test]
    fn test_chunk_emoji() {
        // Emoji: 4 bytes each in UTF-8
        let text = "🔥".repeat(30); // 120 bytes
        let chunks = chunk_message(&text, 10);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 12); // 4-byte emoji, so max overshoot is a single char boundary
        }
    }

    #[test]
    fn test_chunk_html_tag_aware_split_inside_bold() {
        let inner = "x".repeat(200);
        let text = format!("<b>{}</b>", inner);
        let chunks = chunk_message(&text, 120);
        assert!(chunks.len() >= 2, "expected multiple chunks, got: {:?}", chunks);
        for c in &chunks {
            assert!(c.starts_with("<b>"), "chunk should reopen <b>: {}", c);
            assert!(c.ends_with("</b>"), "chunk should close </b>: {}", c);
            assert!(c.len() <= 120, "chunk exceeds max_len: {} bytes", c.len());
        }
        let combined: String = chunks.iter()
            .map(|c| c.trim_start_matches("<b>").trim_end_matches("</b>"))
            .collect();
        assert_eq!(combined, inner);
    }

    #[test]
    fn test_chunk_html_short_unchanged() {
        let chunks = chunk_message("<b>short</b>", 100);
        assert_eq!(chunks, vec!["<b>short</b>".to_string()]);
    }

    #[test]
    fn test_chunk_html_keeps_attrs_on_reopen() {
        let inner = "y".repeat(200);
        let text = format!(r#"<a href="http://example.com">{}</a>"#, inner);
        let chunks = chunk_message(&text, 120);
        assert!(chunks.len() >= 2);
        for c in &chunks[1..] {
            assert!(c.starts_with(r#"<a href="http://example.com">"#),
                "later chunks must reopen full <a> with href: {}", c);
        }
    }

    #[test]
    fn test_floor_char_boundary() {
        let s = "Привет"; // П=2bytes, р=2, и=2, в=2, е=2, т=2 = 12 bytes
        assert_eq!(floor_char_boundary(s, 3), 2); // lands between П and р
        assert_eq!(floor_char_boundary(s, 4), 4);
        assert_eq!(floor_char_boundary(s, 0), 0);
        assert_eq!(floor_char_boundary(s, 100), 12);
    }
}
