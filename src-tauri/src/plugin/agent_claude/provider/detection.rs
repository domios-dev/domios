//! Claude-specific output detection — interactive prompts, task completion,
//! title heuristics, and TUI noise filtering.

use crate::agent::detector::{detect_interactive_prompt, AgentEvent};

pub(super) fn detect_from_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    if lower.contains("claude") {
        return true;
    }
    // Claude Code v2.1+ uses Braille spinner chars or ✳ as title prefix
    // e.g. "⠐ General coding session", "✳ Claude Code"
    let first = title.chars().next().unwrap_or('\0');
    (('\u{2801}'..='\u{28ff}').contains(&first) || first == '✳' || first == '✻')
        && title.len() > 2
}

pub(super) fn detect_from_output(text: &str) -> bool {
    text.contains("Claude Code")
        || text.contains("Do you want to proceed?")
        || text.contains("may I proceed")
        || text.contains("Would you like me to")
        || text.contains("Yes, and don't ask again")
        || text.contains("Enter to confirm")
        || text.contains("Cogitated for")
        || text.contains("Brewed for")
        || text.contains("Reasoned for")
}

pub(super) fn has_event_markers(chunk: &str) -> bool {
    chunk.contains("❯") || chunk.contains("proceed")
        || chunk.contains("Esc to cancel") || chunk.contains("Tab to amend")
        || (chunk.contains("1.") && chunk.contains("2."))
        || chunk.contains(" for 0") || chunk.contains(" for 1") || chunk.contains(" for 2")
        || chunk.contains(" for 3") || chunk.contains(" for 4") || chunk.contains(" for 5")
        || chunk.contains(" for 6") || chunk.contains(" for 7") || chunk.contains(" for 8")
        || chunk.contains(" for 9")
}

pub(super) fn is_output_noise(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() { return true; }
    let noise = [
        "bypass permissions on",
        "shift+tab to cycle",
        "Update available! Run:",
        "brew upgrade claude-code",
        "esc to interrupt",
        "? for shortcuts",
        "ctrl+g to edit",
        "Monthly spend limit",
        "hold Space",
    ];
    noise.iter().any(|n| t.contains(n))
        || t.chars().all(|c| c == '─' || c == '━' || c == '═' || c.is_whitespace())
}

/// Scan text for Claude-specific events using structural fingerprints.
/// Detects interactive prompts by structure (numbered options + keyboard footer)
/// and task completion ("Word for Xs" pattern unique to Claude's status line).
pub(super) fn detect_claude_events(text: &str) -> Vec<AgentEvent> {
    let mut events = Vec::new();

    if detect_interactive_prompt(text) {
        events.push(AgentEvent::PermissionRequest);
    }

    if detect_task_completion(text) {
        events.push(AgentEvent::TaskComplete);
    }

    events
}

/// Structural fingerprint for Claude task completion.
/// Claude shows "<RandomVerb> for <duration>" when done thinking
/// (e.g. "Cogitated for 5s", "Brewed for 12s · $0.05 spent").
/// Pattern: capitalized word + " for " + digits + time unit (s/m/h).
fn detect_task_completion(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(10);
    for line in &lines[start..] {
        let trimmed = line.trim();
        if let Some(pos) = trimmed.find(" for ") {
            let word = &trimmed[..pos];
            let after = &trimmed[pos + 5..];
            let is_single_cap_word = !word.is_empty()
                && word.chars().next().map_or(false, |c| c.is_uppercase())
                && !word.contains(' ')
                && word.chars().all(|c| c.is_alphabetic());
            let has_duration = after.starts_with(|c: char| c.is_ascii_digit())
                && (after.contains('s') || after.contains('m') || after.contains('h'));
            if is_single_cap_word && has_duration {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_from_title() {
        assert!(detect_from_title("Claude Code"));
        assert!(detect_from_title("claude"));
        // Claude v2.1+ spinner titles
        assert!(detect_from_title("⠐ General coding session"));
        assert!(detect_from_title("⠂ General coding session"));
        assert!(detect_from_title("✳ Claude Code"));
        assert!(detect_from_title("✳ Fix bug in parser"));
        // Not Claude
        assert!(!detect_from_title("Aider"));
        assert!(!detect_from_title("C:\\WINDOWS\\System32\\powershell.exe"));
        assert!(!detect_from_title("PS C:\\Users\\User>"));
    }

    #[test]
    fn test_detect_from_output() {
        assert!(detect_from_output("Do you want to proceed?"));
        assert!(detect_from_output("Cogitated for 3s"));
        assert!(!detect_from_output("random text"));
    }

    #[test]
    fn test_detect_claude_permission_prompt() {
        let text = "Reading 1 file...\n\nRead file\n\nDo you want to proceed?\n  ❯ 1. Yes\n    2. Yes, allow reading during this session\n    3. No\n\nEsc to cancel · Tab to amend";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_prompt_without_selector() {
        let text = "Allow this?\n  1. Yes\n  2. No\n\nEsc to cancel";
        let events = detect_claude_events(text);
        assert!(events.contains(&AgentEvent::PermissionRequest));
    }

    #[test]
    fn test_detect_prompt_with_selector_no_footer() {
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
}
