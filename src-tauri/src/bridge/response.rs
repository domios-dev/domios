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

pub fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len { return vec![text.to_string()]; }
    let mut chunks = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.len() <= max_len { chunks.push(remaining.to_string()); break; }
        let slice = &remaining[..max_len];
        let break_at = slice.rfind('\n').unwrap_or(max_len);
        let break_at = if break_at == 0 { max_len } else { break_at };
        chunks.push(remaining[..break_at].to_string());
        remaining = remaining[break_at..].trim_start_matches('\n');
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
}
