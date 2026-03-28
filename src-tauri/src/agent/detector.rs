/// Agent detection — scans PTY output for OSC sequences and prompt patterns.

#[allow(dead_code)]
const MAX_BUFFER_SIZE: usize = 4096;

#[allow(dead_code)]
pub struct AgentDetector {
    buffer: Vec<u8>,
    max_buffer_size: usize,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

/// Scan text (output buffer tail) for Claude Code specific events.
/// Text should be the last ~2000 chars of stripped terminal output.
pub fn detect_claude_events(text: &str) -> Vec<AgentEvent> {
    let mut events = Vec::new();

    // Permission/approval — must be an ACTIVE prompt (near end of output).
    // "Esc to cancel" appears at the very end when Claude waits for input.
    // Check last 300 chars to avoid matching old prompts in history.
    let tail = if text.len() > 300 { &text[text.len() - 300..] } else { text };
    if tail.contains("Esc to cancel")
        && (tail.contains("Do you want to proceed?")
            || (tail.contains("1. Yes") && tail.contains("2.")))
    {
        events.push(AgentEvent::PermissionRequest);
    }

    // Task completion — only specific Claude Code cost/timing patterns
    // Also check tail to avoid matching old completions
    if tail.contains("Cogitated for")
        || tail.contains("Brewed for")
        || tail.contains("Reasoned for")
    {
        events.push(AgentEvent::TaskComplete);
    }

    events
}

/// Parse a permission prompt with numbered options from terminal output.
/// Returns (question, options) if found. Each option is (label, value).
pub fn parse_permission_prompt(text: &str) -> Option<(String, Vec<(String, String)>)> {
    let question_patterns = [
        "Do you want to proceed?",
        "Allow ",
        "Approve ",
    ];

    let mut question = None;
    for pattern in &question_patterns {
        if let Some(pos) = text.rfind(pattern) {
            let end = text[pos..].find('\n').map(|i| pos + i).unwrap_or(text.len());
            question = Some(text[pos..end].trim().to_string());
            break;
        }
    }
    let question = question?;

    // Parse numbered options: "1. Yes", "2. Yes, allow...", "3. No"
    let mut options = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches('❯').trim_start_matches('>').trim();
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let after = &trimmed[digits.len()..];
            if let Some(label) = after.strip_prefix(". ") {
                if !options.iter().any(|(_, v): &(String, String)| *v == digits) {
                    options.push((label.trim().to_string(), digits));
                }
            }
        }
    }

    if options.is_empty() {
        return None;
    }

    Some((question, options))
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

    #[test]
    fn test_parse_permission_prompt() {
        let text = "Some output\n\nDo you want to proceed?\n  \u{276F} 1. Yes\n    2. Yes, allow reading from T/ during this session\n    3. No\n\nEsc to cancel \u{00B7} Tab to amend";
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (question, options) = result.unwrap();
        assert_eq!(question, "Do you want to proceed?");
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].0, "Yes");
        assert_eq!(options[0].1, "1");
        assert_eq!(options[2].0, "No");
        assert_eq!(options[2].1, "3");
    }

    #[test]
    fn test_parse_permission_prompt_no_match() {
        let text = "Regular output with no prompts";
        assert!(parse_permission_prompt(text).is_none());
    }

    // --- detect_claude_events tests ---

    #[test]
    fn test_detect_active_permission_prompt() {
        // Active prompt: "Esc to cancel" near end
        let text = "Do you want to proceed?\n  1. Yes\n  2. Yes, allow\n  3. No\n\nEsc to cancel";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_no_false_positive_old_prompt() {
        // Old prompt buried in history — "Esc to cancel" not in last 300 chars
        let padding = "x".repeat(400);
        let text = format!("Do you want to proceed?\n1. Yes\n2. No\nEsc to cancel\n\n{}\nClaude is working on your task\nwriting files\ndone", padding);
        let events = detect_claude_events(&text);
        assert!(!events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_task_complete() {
        let text = "some output\nCogitated for 5s\n$0.03 spent";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::TaskComplete));
    }

    #[test]
    fn test_detect_no_events_normal_output() {
        let text = "Building project...\nCompilation complete\n";
        let events = detect_claude_events(text);
        assert!(events.is_empty());
    }
}
