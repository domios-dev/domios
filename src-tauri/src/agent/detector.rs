/// Agent detection — scans PTY output for OSC sequences and prompt patterns.

const MAX_BUFFER_SIZE: usize = 4096;

pub struct AgentDetector {
    buffer: Vec<u8>,
    max_buffer_size: usize,
}

#[derive(Debug, Clone)]
pub enum DetectedEvent {
    Notification { title: String, body: String },
    WorkingDirectory { path: String },
    PromptDetected { agent_type: AgentType },
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum AgentType {
    ClaudeCode,
    Aider,
    Generic,
}

impl AgentDetector {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(MAX_BUFFER_SIZE),
            max_buffer_size: MAX_BUFFER_SIZE,
        }
    }

    /// Feed bytes from PTY output, returns any detected events.
    pub fn feed(&mut self, data: &[u8]) -> Vec<DetectedEvent> {
        // Append to buffer, keeping only the last max_buffer_size bytes
        self.buffer.extend_from_slice(data);
        if self.buffer.len() > self.max_buffer_size {
            let excess = self.buffer.len() - self.max_buffer_size;
            self.buffer.drain(..excess);
        }

        let mut events = Vec::new();

        // Scan for OSC sequences in the buffer
        self.scan_osc_sequences(&mut events);

        // Check for prompt patterns in recent text
        if let Ok(text) = std::str::from_utf8(&self.buffer) {
            // Only check the last 512 bytes for prompt patterns
            let check_start = if text.len() > 512 {
                text.len() - 512
            } else {
                0
            };
            let recent = &text[check_start..];
            if let Some(agent_type) = detect_prompt(recent) {
                events.push(DetectedEvent::PromptDetected { agent_type });
            }
        }

        events
    }

    fn scan_osc_sequences(&mut self, events: &mut Vec<DetectedEvent>) {
        // Look for OSC sequences: ESC ] ... (BEL | ESC \)
        // ESC = 0x1b, ] = 0x5d, BEL = 0x07
        let buf = &self.buffer;
        let mut i = 0;

        while i < buf.len() {
            // Find ESC ]
            if buf[i] == 0x1b && i + 1 < buf.len() && buf[i + 1] == 0x5d {
                let osc_start = i + 2; // after ESC ]

                // Find terminator: BEL (0x07) or ESC \ (0x1b 0x5c)
                if let Some((osc_end, _term_len)) = find_osc_terminator(buf, osc_start) {
                    let osc_data = &buf[osc_start..osc_end];
                    if let Ok(osc_str) = std::str::from_utf8(osc_data) {
                        if let Some(event) = parse_osc(osc_str) {
                            events.push(event);
                        }
                    }
                    i = osc_end;
                    continue;
                } else {
                    // Incomplete OSC — might get more data later, stop scanning
                    break;
                }
            }
            i += 1;
        }
    }
}

/// Find the terminator for an OSC sequence starting at `start`.
/// Returns (end_index, terminator_length) or None if not found.
fn find_osc_terminator(buf: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    while i < buf.len() {
        if buf[i] == 0x07 {
            // BEL
            return Some((i, 1));
        }
        if buf[i] == 0x1b && i + 1 < buf.len() && buf[i + 1] == 0x5c {
            // ESC backslash
            return Some((i, 2));
        }
        i += 1;
    }
    None
}

/// Parse an OSC payload string (after "ESC ]" and before terminator).
fn parse_osc(osc: &str) -> Option<DetectedEvent> {
    // OSC 9 ; text — notification (ConEmu/iTerm2 style)
    // Skip OSC 9;4;... (progress indicator) and very short bodies (state updates)
    if let Some(text) = osc.strip_prefix("9;") {
        if !text.starts_with("4;") && text.len() > 5 && !text.chars().all(|c| c.is_ascii_digit() || c == ';') {
            return Some(DetectedEvent::Notification {
                title: "Terminal".to_string(),
                body: text.to_string(),
            });
        }
    }

    // OSC 99 ; ... — notification with parameters
    // Format: 99;key=value;key=value or 99;text
    if let Some(rest) = osc.strip_prefix("99;") {
        let mut title = "Terminal".to_string();
        let mut body = String::new();

        // Try to parse key=value pairs
        for part in rest.split(';') {
            if let Some(val) = part.strip_prefix("title=") {
                title = val.to_string();
            } else if let Some(val) = part.strip_prefix("body=") {
                body = val.to_string();
            } else if body.is_empty() {
                body = part.to_string();
            }
        }

        return Some(DetectedEvent::Notification { title, body });
    }

    // OSC 777 ; notify ; title ; body — rxvt-unicode style
    if let Some(rest) = osc.strip_prefix("777;notify;") {
        let parts: Vec<&str> = rest.splitn(2, ';').collect();
        let title = parts.first().unwrap_or(&"Terminal").to_string();
        let body = parts.get(1).unwrap_or(&"").to_string();
        return Some(DetectedEvent::Notification { title, body });
    }

    // OSC 7 ; file://host/path — current working directory
    if let Some(uri) = osc.strip_prefix("7;") {
        if let Some(path) = uri.strip_prefix("file://") {
            // Strip the hostname part: file://hostname/path -> /path
            let path = if let Some(idx) = path.find('/') {
                &path[idx..]
            } else {
                path
            };
            return Some(DetectedEvent::WorkingDirectory {
                path: path.to_string(),
            });
        }
    }

    None
}

/// Check if recent terminal output matches known agent prompts.
fn detect_prompt(text: &str) -> Option<AgentType> {
    // Claude Code permission/approval patterns
    if text.contains("Do you want to proceed?")
        || text.contains("may I proceed")
        || text.contains("Would you like me to")
        || text.contains("Yes, and don't ask again")
        || text.contains("Enter to confirm")
    {
        return Some(AgentType::ClaudeCode);
    }

    // Aider patterns
    if text.contains("(Y)es/(N)o") || text.contains("y/n/") {
        return Some(AgentType::Aider);
    }

    None
}

/// Detect specific Claude Code events from output.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// Agent is asking for permission (tool approval, confirmation)
    PermissionRequest,
    /// Agent finished working (prompt returned after output)
    TaskComplete,
}

/// Scan text for Claude Code specific events.
pub fn detect_claude_events(text: &str) -> Vec<AgentEvent> {
    let mut events = Vec::new();

    // Permission/approval request patterns
    if text.contains("Do you want to proceed?")
        || text.contains("Yes, and don't ask again")
        || text.contains("Enter to confirm")
        || (text.contains("1. Yes") && text.contains("2. Yes, and"))
        || (text.contains("1.") && text.contains("No") && text.contains("Yes"))
    {
        events.push(AgentEvent::PermissionRequest);
    }

    // Task completion — Claude Code returned to idle prompt.
    // After ANSI stripping, these patterns indicate ready state:
    // - "? for shortcuts" — bottom bar of Claude Code TUI
    // - "for shortcuts" — partial match
    // - The ❯ prompt character at line start (Claude Code prompt)
    // - "Cogitated" / "Brewed" / cost summary lines indicate task just finished
    if text.contains("for shortcuts")
        || text.contains("\u{276f}") // ❯ prompt
        || text.contains("Cogitated for")
        || text.contains("Brewed for")
    {
        events.push(AgentEvent::TaskComplete);
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_osc9_notification() {
        let mut detector = AgentDetector::new();
        let data = b"\x1b]9;Build complete\x07";
        let events = detector.feed(data);
        assert_eq!(events.len(), 1);
        match &events[0] {
            DetectedEvent::Notification { title, body } => {
                assert_eq!(title, "Terminal");
                assert_eq!(body, "Build complete");
            }
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_osc7_cwd() {
        let mut detector = AgentDetector::new();
        let data = b"\x1b]7;file://localhost/home/user/project\x07";
        let events = detector.feed(data);
        assert!(events.iter().any(|e| matches!(
            e,
            DetectedEvent::WorkingDirectory { path } if path == "/home/user/project"
        )));
    }

    #[test]
    fn test_osc777_notification() {
        let mut detector = AgentDetector::new();
        let data = b"\x1b]777;notify;Build;Completed successfully\x07";
        let events = detector.feed(data);
        assert!(events.iter().any(|e| matches!(
            e,
            DetectedEvent::Notification { title, body }
                if title == "Build" && body == "Completed successfully"
        )));
    }

    #[test]
    fn test_prompt_detection_claude() {
        let mut detector = AgentDetector::new();
        let data = b"Do you want to proceed?";
        let events = detector.feed(data);
        assert!(events.iter().any(|e| matches!(
            e,
            DetectedEvent::PromptDetected { agent_type } if *agent_type == AgentType::ClaudeCode
        )));
    }
}
