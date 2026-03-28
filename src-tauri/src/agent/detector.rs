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

/// Scan text for Claude Code specific events using structural fingerprints.
/// Detects interactive prompts by structure (numbered options + keyboard footer),
/// NOT by matching specific question text — avoids false positives from source code.
pub fn detect_claude_events(text: &str) -> Vec<AgentEvent> {
    let mut events = Vec::new();

    if detect_interactive_prompt(text) {
        events.push(AgentEvent::PermissionRequest);
    }

    // Task completion — structural fingerprint: "<Capitalized word> for <duration>"
    // Claude uses random verbs (Cogitated, Brewed, Pounced, Preambled, etc.) + time.
    // Pattern: word starting with uppercase + " for " + digits + s/m/h
    if detect_task_completion(text) {
        events.push(AgentEvent::TaskComplete);
    }

    events
}

/// Structural fingerprint for task completion.
/// Claude shows "<RandomVerb> for <duration>" when done thinking.
/// Pattern: capitalized word + " for " + digits + time unit (s/m/h).
/// Often followed by cost: "· $0.03 spent"
fn detect_task_completion(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    // Check last 10 lines
    let start = lines.len().saturating_sub(10);
    for line in &lines[start..] {
        let trimmed = line.trim();
        // Pattern: "Word for Ns" or "Word for Nm Ns" or "Word for Nh Nm"
        if let Some(pos) = trimmed.find(" for ") {
            let word = &trimmed[..pos];
            let after = &trimmed[pos + 5..];
            // Word must be single capitalized word (no spaces, starts uppercase)
            let is_single_cap_word = !word.is_empty()
                && word.chars().next().map_or(false, |c| c.is_uppercase())
                && !word.contains(' ')
                && word.chars().all(|c| c.is_alphabetic());
            // After must start with digit and contain time unit
            let has_duration = after.starts_with(|c: char| c.is_ascii_digit())
                && (after.contains('s') || after.contains('m') || after.contains('h'));
            if is_single_cap_word && has_duration {
                return true;
            }
        }
    }
    false
}

/// Structural fingerprint detection for interactive prompts.
/// Matches the pattern:
///   - 2+ numbered options (N. text) with optional ❯ selector
///   - Keyboard hint footer (Esc/Tab/Enter keywords)
///
/// Key constraint: numbered options and footer/selector must be CLOSE together
/// (within 8 lines). This prevents false positives when Claude's answer contains
/// a numbered list far above the actual prompt.
///
/// This is generic enough for any TUI menu, not just Claude.
/// Source code won't have this exact structural combination.
pub fn detect_interactive_prompt(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 3 { return false; }

    // Scan from the end — prompt is always at the bottom of output
    let scan_start = lines.len().saturating_sub(20);
    let tail_lines = &lines[scan_start..];

    // Track line indices of features to check proximity
    let mut option_indices: Vec<usize> = Vec::new();
    let mut footer_idx: Option<usize> = None;    // keyboard hint line
    let mut selector_idx: Option<usize> = None;

    for (i, line) in tail_lines.iter().enumerate() {
        let trimmed = line.trim();

        // Numbered option: "1. Yes", "❯ 1. Yes", "> 2. No"
        let cleaned = trimmed.trim_start_matches('❯').trim_start_matches('>').trim();
        if let Some(rest) = cleaned.strip_prefix(|c: char| c.is_ascii_digit()) {
            // Handle multi-digit: "10. Something"
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            if rest.starts_with(". ") {
                option_indices.push(i);
                if trimmed.starts_with('❯') || trimmed.starts_with('>') {
                    selector_idx = Some(i);
                }
            }
        }

        // Keyboard hint footer: contains Esc/Tab/Enter as shortcuts
        if (trimmed.contains("Esc") || trimmed.contains("Enter"))
            && (trimmed.contains("cancel") || trimmed.contains("amend") || trimmed.contains("confirm"))
        {
            footer_idx = Some(i);
        }
    }

    if option_indices.len() < 2 {
        return false;
    }

    // Check proximity: options must be close to each other AND to footer/selector.
    // Find the contiguous cluster of options near the anchor (footer or selector).
    let anchor = footer_idx.or(selector_idx);

    if let Some(anchor_idx) = anchor {
        // Count options within 8 lines of the anchor
        let nearby_options = option_indices.iter()
            .filter(|&&idx| {
                let dist = if idx > anchor_idx { idx - anchor_idx } else { anchor_idx - idx };
                dist <= 8
            })
            .count();
        nearby_options >= 2
    } else {
        // No footer and no selector — not a prompt
        false
    }
}

/// Parse a permission prompt with numbered options from terminal output.
/// Returns (question, options) if found. Each option is (label, value).
/// Only parses numbered options that appear AFTER the question line to avoid
/// picking up numbered lists from Claude's response text.
pub fn parse_permission_prompt(text: &str) -> Option<(String, Vec<(String, String)>)> {
    let question_patterns = [
        "Do you want to",   // "proceed?", "create X?", "run X?", etc.
        "Allow ",
        "Approve ",
    ];

    let lines: Vec<&str> = text.lines().collect();
    let mut question = None;
    let mut question_line_idx = None;

    // Find the LAST occurrence of a question pattern (use last to handle repeated text)
    for (i, line) in lines.iter().enumerate() {
        for pattern in &question_patterns {
            if line.contains(pattern) {
                question = Some(line.trim().to_string());
                question_line_idx = Some(i);
            }
        }
    }
    let question = question?;
    let start = question_line_idx?;

    // Only parse numbered options AFTER the question, within 10 lines
    let end = (start + 10).min(lines.len());
    let mut options = Vec::new();
    for line in &lines[start..end] {
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
    fn test_detect_claude_permission_prompt() {
        let text = "Reading 1 file...\n\nRead file\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. Yes, allow reading during this session\n    3. No\n\nEsc to cancel · Tab to amend";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_prompt_without_selector() {
        // Even without ❯, keyboard footer is enough
        let text = "Allow this?\n  1. Yes\n  2. No\n\nEsc to cancel";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_prompt_with_selector_no_footer() {
        // ❯ selector is enough without footer
        let text = "Choose:\n  ❯ 1. Option A\n    2. Option B\n    3. Option C";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_no_false_positive_source_code() {
        let text = r#"if text.contains("Do you want to proceed?") && text.contains("Esc to cancel") {
            events.push(PermissionRequest);
        }
        // Check for "1. Yes" and "2. No" patterns
        let options = vec!["1. Yes", "2. No"];"#;
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_no_false_positive_normal_output() {
        let text = "Building project...\nStep 1. Compile\nStep 2. Link\nStep 3. Package\nDone!";
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_no_false_positive_single_option() {
        // Only 1 numbered item — not a prompt
        let text = "1. Yes\nEsc to cancel";
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_task_complete_cogitated() {
        let text = "some output\nCogitated for 5s\n$0.03 spent";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::TaskComplete));
    }

    #[test]
    fn test_detect_task_complete_random_verb() {
        // Claude uses random verbs — structural detection catches any
        for verb in &["Pounced", "Brewed", "Reasoned", "Preambled", "Wandered", "Mused"] {
            let text = format!("output\n{} for 12s · $0.05 spent", verb);
            let events = detect_claude_events(&text);
            assert!(events.contains(&AgentEvent::TaskComplete), "Failed for verb: {}", verb);
        }
    }

    #[test]
    fn test_detect_task_complete_multiunit() {
        let text = "output\nContemplated for 3m 15s";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::TaskComplete));
    }

    #[test]
    fn test_no_false_completion_normal_text() {
        // "waiting for response" should NOT match — "waiting" has lowercase start, "for" not followed by time
        let text = "Waiting for response from server...\nCompiling for the first time";
        let events = detect_claude_events(text);
        assert!(!events.contains(&AgentEvent::TaskComplete));
    }

    #[test]
    fn test_detect_no_events_normal_output() {
        let text = "Building project...\nCompilation complete\n";
        let events = detect_claude_events(text);
        assert!(events.is_empty());
    }

    // --- Structural fingerprint edge cases ---

    #[test]
    fn test_fingerprint_real_claude_bash_approval() {
        // Real Claude output when asking to run a bash command
        let text = "I'll run this command:\n\n  Bash(npm test)\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. Yes, and don't ask again for Bash commands\n    3. No\n\nEsc to cancel · Tab to amend";
        assert!(detect_interactive_prompt(text));
    }

    #[test]
    fn test_fingerprint_real_claude_write_approval() {
        // Real Claude output when writing a file
        let text = "Write file\n\n  Write(src/main.rs)\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. Yes, allow writing to src/ during this session\n    3. No\n\nEsc to cancel · Tab to amend";
        assert!(detect_interactive_prompt(text));
    }

    #[test]
    fn test_fingerprint_generic_cli_menu() {
        // Any TUI with numbered options + selector works
        let text = "Select an option:\n  > 1. Install\n    2. Update\n    3. Remove\n    4. Exit";
        assert!(detect_interactive_prompt(text));
    }

    #[test]
    fn test_fingerprint_no_match_markdown_list() {
        // Markdown numbered list — no selector, no footer
        let text = "Steps to follow:\n1. Clone the repo\n2. Install dependencies\n3. Run tests\n4. Deploy";
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_fingerprint_no_match_code_with_options() {
        // Code that mentions options as strings
        let text = r#"match choice {
    "1. Yes" => proceed(),
    "2. No" => cancel(),
    _ => {}
}"#;
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_fingerprint_no_match_log_output() {
        // Log lines starting with numbers
        let text = "2026-03-28 10:00:01 Server started\n2026-03-28 10:00:02 Listening on port 8080\n2026-03-28 10:00:03 Ready";
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_fingerprint_prompt_buried_in_output() {
        // Prompt at the end of long output — should still match
        let mut text = "lots of output\n".repeat(100);
        text.push_str("Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n\nEsc to cancel");
        assert!(detect_interactive_prompt(&text));
    }

    #[test]
    fn test_fingerprint_old_prompt_not_at_end() {
        // Old prompt NOT in last 20 lines — should not match
        let mut text = String::from("Old prompt:\n  ❯ 1. Yes\n    2. No\nEsc to cancel\n");
        for _ in 0..25 {
            text.push_str("Agent is working on something...\n");
        }
        assert!(!detect_interactive_prompt(&text));
    }

    // --- False positive prevention tests ---

    #[test]
    fn test_parse_prompt_ignores_numbered_list_before_question() {
        let text = "Here's my analysis:\n1. First bug in code\n2. Second issue found\n3. Third problem\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. No\n\nEsc to cancel";
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (q, opts) = result.unwrap();
        assert_eq!(q, "Do you want to proceed?");
        assert_eq!(opts.len(), 2); // Only Yes/No, not the analysis items
        assert_eq!(opts[0].0, "Yes");
        assert_eq!(opts[1].0, "No");
    }

    #[test]
    fn test_fingerprint_ignores_numbered_answer_before_prompt() {
        let text = "Analysis:\n1. Bug found\n2. Issue here\n3. Fix needed\n4. Test required\n5. Deploy\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. No\nEsc to cancel";
        // Should detect as prompt (numbered options near footer)
        assert!(detect_interactive_prompt(text));
    }

    #[test]
    fn test_fingerprint_no_match_just_numbered_answer() {
        // Claude's numbered answer WITHOUT any prompt
        let text = "Here's what I found:\n1. First issue\n2. Second issue\n3. Third issue\n\nLet me fix these.";
        assert!(!detect_interactive_prompt(text));
    }
}
