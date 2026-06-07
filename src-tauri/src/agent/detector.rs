//! Generic agent output detection — provider-agnostic helpers.
//!
//! Each `AgentProvider` plugin owns its own detection (title patterns,
//! noise filters, completion fingerprints). This module hosts the generic
//! primitives shared across all providers: structural prompt detection,
//! permission-prompt parsing, and the `AgentEvent` type.

/// Generic event surfaced by an agent provider's `detect_events()` method.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// Agent is asking for permission (tool approval, confirmation)
    PermissionRequest,
    /// Agent finished working (prompt returned after output)
    TaskComplete,
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

/// Known selector prefixes in prompt option lines.
///
/// `❯` (U+276F) is the canonical selector, `>` is the ASCII fallback.
/// Some VT100 parsers mangle UTF-8 — the byte sequence E2 9D AF (❯) can lose
/// the middle byte 0x9D (C1 control) and become `â¯` (U+00E2 U+00AF).
/// We match all variants so detection works regardless of terminal encoding.
const SELECTOR_CHARS: &[char] = &['❯', '>', '›'];
const GARBLED_SELECTOR: &str = "\u{00e2}\u{00af}"; // â¯ — VT100-mangled ❯

/// Strip selector prefix from a line, returning the remainder trimmed.
fn strip_selector(trimmed: &str) -> &str {
    // Try single-char selectors first
    let s = trimmed.trim_start_matches(SELECTOR_CHARS).trim_start();
    if s.len() < trimmed.trim_start().len() {
        return s;
    }
    // Try garbled multi-byte selector
    if let Some(rest) = trimmed.strip_prefix(GARBLED_SELECTOR) {
        return rest.trim_start();
    }
    trimmed
}

/// Check if a line starts with a known selector prefix.
fn has_selector_prefix(trimmed: &str) -> bool {
    SELECTOR_CHARS.iter().any(|&c| trimmed.starts_with(c)) || trimmed.starts_with(GARBLED_SELECTOR)
}

pub fn detect_interactive_prompt(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 3 {
        return false;
    }

    // Scan from the end — prompt is always at the bottom of output
    let scan_start = lines.len().saturating_sub(20);
    let tail_lines = &lines[scan_start..];

    // Track line indices of features to check proximity
    let mut option_indices: Vec<usize> = Vec::new();
    let mut footer_idx: Option<usize> = None; // keyboard hint line
    let mut selector_idx: Option<usize> = None;

    for (i, line) in tail_lines.iter().enumerate() {
        let trimmed = line.trim();

        // Numbered option: "1. Yes", "❯ 1. Yes", "> 2. No"
        let cleaned = strip_selector(trimmed);
        if let Some(rest) = cleaned.strip_prefix(|c: char| c.is_ascii_digit()) {
            // Handle multi-digit: "10. Something"
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            if rest.starts_with(". ") {
                option_indices.push(i);
                if has_selector_prefix(trimmed) {
                    selector_idx = Some(i);
                }
            }
        }

        // Keyboard hint footer: contains Esc/Tab/Enter as shortcuts
        let lower = trimmed.to_lowercase();
        if (lower.contains("esc") || lower.contains("enter"))
            && (lower.contains("cancel")
                || lower.contains("amend")
                || lower.contains("confirm")
                || lower.contains("continue"))
        {
            footer_idx = Some(i);
        }
    }

    if option_indices.len() < 2 {
        return false;
    }

    // Check proximity: options must be close to each other AND to footer/selector. Find the contiguous cluster of options near the anchor (footer
    // or selector).
    let anchor = footer_idx.or(selector_idx);

    if let Some(anchor_idx) = anchor {
        // Count options within 8 lines of the anchor
        let nearby_options = option_indices
            .iter()
            .filter(|&&idx| {
                let dist = if idx > anchor_idx {
                    idx - anchor_idx
                } else {
                    anchor_idx - idx
                };
                dist <= 8
            })
            .count();
        nearby_options >= 2
    } else {
        // No footer and no selector — not a prompt
        false
    }
}

/// Parse an interactive prompt with numbered options from terminal output.
/// Returns (question, options) if found. Each option is (label, value).
///
/// Uses structural detection — no hardcoded question text:
/// 1. Find numbered options cluster (N. text) near the end
/// 2. The question is the nearest line ending with `?` above the options
/// 3. If no `?` line, use the line immediately before the first option
pub fn parse_permission_prompt(text: &str) -> Option<(String, Vec<(String, String)>)> {
    parse_interactive_picker(text).map(|p| (p.question, p.options))
}

#[derive(Debug, Clone, PartialEq)]
pub struct InteractivePicker {
    pub question: String,
    pub options: Vec<(String, String)>,
    pub selected_index: Option<usize>,
}

/// Parse the active numbered TUI picker near the bottom of the terminal.
/// This is intentionally generic: Claude Code uses the same numbered-option
/// shape for permission prompts and local command pickers like `/model`.
pub fn parse_interactive_picker(text: &str) -> Option<InteractivePicker> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 2 {
        return None;
    }

    // Scan from end for numbered options — take only the LAST cluster (closest to the end of text, which is where the active prompt is)
    let scan_start = lines.len().saturating_sub(20);
    let mut all_opts: Vec<(String, String, usize)> = Vec::new();

    for i in scan_start..lines.len() {
        let trimmed = strip_selector(lines[i].trim());
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let after = &trimmed[digits.len()..];
            if let Some(label) = after.strip_prefix(". ") {
                all_opts.push((label.trim().to_string(), digits, i));
            }
        }
    }

    if all_opts.len() < 2 {
        return None;
    }

    // Extract last cluster: walk backwards from the last option, include options that are within 3 lines of each other
    let mut options: Vec<(String, String, usize)> = Vec::new();
    let mut prev_idx = all_opts.last()?.2;
    for opt in all_opts.iter().rev() {
        if prev_idx - opt.2 <= 2 || options.is_empty() {
            options.push(opt.clone());
            prev_idx = opt.2;
        } else {
            break; // gap too big — different cluster
        }
    }
    options.reverse();

    if options.len() < 2 {
        return None;
    }
    let first_opt = options.first()?.2;
    let last_opt = options.last()?.2;

    // Must have interactive markers nearby: ❯/> selector OR keyboard footer Without these, it's just a regular numbered list
    let has_selector = options
        .iter()
        .any(|(_, _, idx)| has_selector_prefix(lines[*idx].trim()));
    let has_footer = (last_opt..lines.len().min(last_opt + 5)).any(|i| {
        let t = lines[i].trim().to_lowercase();
        (t.contains("esc") || t.contains("enter"))
            && (t.contains("cancel")
                || t.contains("amend")
                || t.contains("confirm")
                || t.contains("continue"))
    });
    if !has_selector && !has_footer {
        return None;
    }

    // Find question: nearest line ending with `?` above the first option
    let mut question = None;
    for i in (0..first_opt).rev() {
        let trimmed = lines[i].trim();
        if trimmed.ends_with('?') && trimmed.len() > 3 {
            question = Some(trimmed.to_string());
            break;
        }
        // Don't search more than 5 lines above
        if first_opt - i > 5 {
            break;
        }
    }

    // Fallback: use the non-empty line just above first option
    if question.is_none() {
        for i in (0..first_opt).rev() {
            let trimmed = lines[i].trim();
            if !trimmed.is_empty() && trimmed.len() > 3 {
                question = Some(trimmed.to_string());
                break;
            }
            if first_opt - i > 3 {
                break;
            }
        }
    }

    let question = question.unwrap_or_else(|| "Permission required".to_string());
    let selected_index = options
        .iter()
        .position(|(_, _, idx)| has_selector_prefix(lines[*idx].trim()));
    let result_options: Vec<(String, String)> = options
        .into_iter()
        .map(|(label, value, _)| (label, value))
        .collect();

    if result_options.is_empty() {
        return None;
    }

    Some(InteractivePicker {
        question,
        options: result_options,
        selected_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_parse_interactive_picker_selected_index() {
        let text = "Choose model\n    1. Opus\n  ❯ 2. Sonnet\n    3. Haiku\n\nEnter to confirm · Esc to cancel";
        let result = parse_interactive_picker(text).unwrap();
        assert_eq!(result.question, "Choose model");
        assert_eq!(result.selected_index, Some(1));
        assert_eq!(result.options[1], ("Sonnet".to_string(), "2".to_string()));
    }

    #[test]
    fn test_parse_permission_prompt_no_match() {
        let text = "Regular output with no prompts";
        assert!(parse_permission_prompt(text).is_none());
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
        let text =
            "Steps to follow:\n1. Clone the repo\n2. Install dependencies\n3. Run tests\n4. Deploy";
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

    // --- Synthetic data: Claude answers with numbered lists (must NOT trigger) ---

    #[test]
    fn test_no_false_positive_bug_analysis() {
        let text = "Here's what I found:\n1. **Memory leak** in the connection pool\n2. **Race condition** in the cache layer\n3. **Missing error handling** in the API endpoint";
        assert!(
            parse_permission_prompt(text).is_none(),
            "Bug analysis list should not be parsed as prompt"
        );
        assert!(
            !detect_interactive_prompt(text),
            "Bug analysis should not trigger prompt detection"
        );
    }

    #[test]
    fn test_no_false_positive_step_instructions() {
        let text = "To fix this:\n1. Update the dependency to v2.3.0\n2. Run the database migration\n3. Restart the application server\n4. Verify the fix with integration tests";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_feature_list_with_code() {
        let text = "The module provides:\n1. `parse_config` — reads TOML configuration files\n2. `validate_schema` — checks JSON schema compliance\n3. `emit_events` — sends notifications to subscribers\n4. `retry_with_backoff` — handles transient failures";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_code_review() {
        let text = "Issues found in the PR:\n1. Line 42: unused import `std::collections::HashMap`\n2. Line 87: missing error check on file open\n3. Line 156: hardcoded timeout value of 30s\n4. Line 201: variable shadowing in inner scope";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_question_with_options_no_selector() {
        // Question with ? before numbered list — but no selector/footer = NOT a prompt
        let text = "Which approach do you prefer?\n\n1. Refactor the entire module into smaller components\n2. Add a compatibility layer for backward support\n3. Keep backward compatibility and add feature flags";
        assert!(
            parse_permission_prompt(text).is_none(),
            "Question before list without selector is not a prompt"
        );
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_tool_results() {
        let text = "1. `save_file` — completed successfully\n2. `run_tests` — 3 failures detected\n3. `lint_check` — 2 warnings\n4. `deploy_staging` — pending approval";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_nested_list() {
        let text = "Here are the results:\n\n1. **Performance**: 23% faster than baseline\n   - Latency: 45ms avg\n   - Throughput: 1200 req/s\n2. **Memory**: 15% reduction\n   - Peak: 512MB\n   - Steady state: 340MB\n3. **Reliability**: 99.97% uptime\n   - Error rate: 0.03%";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_phased_plan() {
        let text = "Phase 1: Foundation\n1. Set up project scaffolding\n2. Configure CI pipeline\n3. Define API contracts\n\nPhase 2: Implementation\n1. Build core service\n2. Add authentication layer\n3. Implement rate limiting";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_comparison() {
        let text = "Option A (monolith):\n1. Fast to develop\n2. Simple deployment\n3. Easy to debug\n\nOption B (microservices):\n1. Flexible scaling\n2. Independent deployments\n3. Technology diversity\n4. Better fault isolation";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_security_audit() {
        let text = "Key findings from the audit:\n\n1. **SQL injection** vulnerability in search endpoint\n   - Severity: Critical\n   - File: `handlers/search.rs`\n2. **XSS** in user profile display\n   - Severity: High\n   - File: `templates/profile.html`\n3. **CSRF** token not validated on form submission\n   - Severity: Medium\n   - File: `middleware/csrf.rs`";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_three_options_summary() {
        // Three approaches listed — looks like options but no selector/footer
        let text = "I see three possible approaches:\n\n1. **Use a feature flag** — toggle the new behavior at runtime, minimal risk\n2. **Create a new endpoint** — keep old behavior, add `/v2/resource`, gradual migration\n3. **In-place migration** — modify existing endpoint, all clients must update simultaneously\n\nI recommend option 1 for the safest rollout.";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_no_false_positive_numbered_list_inside_code_block() {
        // Numbered list that happens to be in a markdown code block context
        let text = "```\n1. First item\n2. Second item\n3. Third item\n```\nThis shows the expected output format.";
        assert!(parse_permission_prompt(text).is_none());
        assert!(!detect_interactive_prompt(text));
    }

    // --- Synthetic data: real prompts that SHOULD trigger ---

    #[test]
    fn test_real_prompt_yes_no() {
        let text = "Do you want to proceed?\n  ❯ 1. Yes\n    2. No\n\nEsc to cancel · Tab to amend";
        assert!(
            detect_interactive_prompt(text),
            "Standard yes/no prompt should trigger"
        );
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (q, opts) = result.unwrap();
        assert_eq!(q, "Do you want to proceed?");
        assert_eq!(opts.len(), 2);
    }

    #[test]
    fn test_real_prompt_file_write() {
        let text = "Create src/main.rs?\n  ❯ 1. Yes\n    2. Yes, allow writes to src/\n    3. No\n\nEsc to cancel";
        assert!(detect_interactive_prompt(text));
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (q, opts) = result.unwrap();
        assert!(q.contains("main.rs"));
        assert_eq!(opts.len(), 3);
    }

    #[test]
    fn test_real_prompt_bash_execution() {
        let text = "Run `npm test`?\n  ❯ 1. Yes\n    2. Yes, don't ask again for Bash\n    3. No\n\nEsc to cancel · Tab to amend";
        assert!(detect_interactive_prompt(text));
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (_, opts) = result.unwrap();
        assert_eq!(opts.len(), 3);
    }

    #[test]
    fn test_real_prompt_with_preceding_answer() {
        // Answer text BEFORE the prompt — prompt at the end should still trigger
        let text = "Found 3 issues:\n1. Bug in parser\n2. Missing test\n3. Type error\n\nApply fixes?\n  ❯ 1. Yes\n    2. No\n\nEsc to cancel";
        assert!(detect_interactive_prompt(text));
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (q, opts) = result.unwrap();
        assert_eq!(q, "Apply fixes?");
        assert_eq!(opts.len(), 2); // Only Yes/No, not the issue list
    }

    #[test]
    fn test_real_prompt_long_option_labels() {
        let text = "Allow reading from /home/dev/project/config/?\n  ❯ 1. Yes\n    2. Yes, allow reading from /home/dev/ during this session\n    3. No\n\nEsc to cancel · Tab to amend";
        assert!(detect_interactive_prompt(text));
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (q, opts) = result.unwrap();
        assert!(q.contains("/home/dev/project/config/"));
        assert_eq!(opts.len(), 3);
    }

    #[test]
    fn test_real_prompt_generic_tui_selector() {
        // Generic TUI menu with > selector (no footer)
        let text = "Select mode:\n  > 1. Development\n    2. Staging\n    3. Production";
        assert!(detect_interactive_prompt(text));
    }

    #[test]
    fn test_real_prompt_enter_to_confirm() {
        let text = "Confirm action:\n  ❯ 1. Apply changes\n    2. Review diff first\n    3. Cancel\n\nEnter to confirm · Esc to cancel";
        assert!(detect_interactive_prompt(text));
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (_, opts) = result.unwrap();
        assert_eq!(opts.len(), 3);
    }

    #[test]
    fn test_real_prompt_with_plan_before() {
        // Answer with numbered plan, then a prompt at the end
        let text = "Here is the plan:\n1. Update schema\n2. Migrate data\n3. Deploy service\n\nProceed with migration?\n  ❯ 1. Yes\n    2. No\n\nEsc to cancel · Tab to amend";
        assert!(detect_interactive_prompt(text));
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (q, opts) = result.unwrap();
        assert_eq!(q, "Proceed with migration?");
        assert_eq!(opts.len(), 2);
    }

    // --- Edge cases ---

    #[test]
    fn test_edge_case_question_mark_then_plain_list() {
        // Question ending with ? followed by plain numbered list — no selector/footer
        let text = "What should we prioritize?\n\n1. Performance optimization\n2. Bug fixes\n3. New features\n4. Technical debt\n\nLet me know your preference.";
        assert!(
            parse_permission_prompt(text).is_none(),
            "Question + plain list = not a prompt"
        );
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_edge_case_esc_in_text_far_from_list() {
        // Text mentioning "Esc to cancel" far away from a numbered list — not a prompt
        let text = "Press Esc to cancel the operation.\n\nSome other output here.\nMore output.\nEven more output.\nStill going.\nAlmost done.\nKeep going.\nYet another line.\nMore lines here.\nFinal line.\n\nHere are the steps:\n1. Open the dialog\n2. Select the file\n3. Click confirm";
        assert!(!detect_interactive_prompt(text));
    }

    #[test]
    fn test_edge_case_numbers_in_output_not_list() {
        // Numbers that look like list items but are not
        let text = "2026-03-28 10:00:01 Server started\n2026-03-28 10:00:02 Connected to database\n2026-03-28 10:00:03 Ready to accept requests";
        assert!(!detect_interactive_prompt(text));
    }

    // ── Garbled UTF-8 selector (VT100 C1 byte stripping) ────────────

    #[test]
    fn test_garbled_selector_detect() {
        // VT100 parsers can mangle ❯ (E2 9D AF) → â¯ (E2 AF, 9D eaten as C1)
        let text = "Do you want to proceed?\n  \u{00e2}\u{00af} 1. Yes\n    2. No\n\nEsc to cancel \u{00b7} Tab to amend";
        assert!(
            detect_interactive_prompt(text),
            "Should detect garbled â¯ selector"
        );
    }

    #[test]
    fn test_garbled_selector_parse() {
        let text = "Do you want to proceed?\n  \u{00e2}\u{00af} 1. Yes\n    2. No\n\nEsc to cancel";
        let result = parse_permission_prompt(text);
        assert!(result.is_some(), "Should parse garbled selector prompt");
        let (q, opts) = result.unwrap();
        assert_eq!(q, "Do you want to proceed?");
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].0, "Yes");
        assert_eq!(opts[1].0, "No");
    }

    #[test]
    fn test_garbled_selector_3_options() {
        // Full 3-option prompt with garbled selector — all options parsed
        let text = "\nDo you want to proceed?\n  \u{00e2}\u{00af} 1. Yes\n    2. Yes, allow during session\n    3. No\n\nEsc to cancel \u{00b7} Tab to amend\n";
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (_, opts) = result.unwrap();
        assert_eq!(opts.len(), 3, "All 3 options should be parsed: {:?}", opts);
        assert_eq!(opts[0].0, "Yes");
        assert_eq!(opts[2].0, "No");
    }

    // ── Claude todo/task list — must NOT trigger ────────────────────

    #[test]
    fn test_claude_todo_not_prompt() {
        // Claude Code TUI shows todo lists with ✓/□ checkboxes — not a prompt
        let text = concat!(
            "* Seasoning... (32s · ↓ 96 tokens)\n",
            "  ✓ Step 1: Registration token on backend\n",
            "  ✓ Step 2: Plugin auto-discovery via Tailscale\n",
            "  ✓ Step 3: Auto-registration + Settings UI\n",
            "  □ Step 4: Backend peer scanning + dashboard\n",
            "  ✓ Backend: pending/approve flow + migration 003\n",
            "  ✓ Dashboard: pending clients UI\n",
        );
        assert!(
            !detect_interactive_prompt(text),
            "Todo list must not be a prompt"
        );
        assert!(parse_permission_prompt(text).is_none());
    }

    #[test]
    fn test_claude_todo_with_numbered_steps() {
        // Numbered steps without selector or footer — not a prompt
        let text = "Here's my plan:\n1. Fix the parser\n2. Add tests\n3. Update docs\n4. Deploy\n\nLet me start with step 1.";
        assert!(!detect_interactive_prompt(text));
        assert!(parse_permission_prompt(text).is_none());
    }

    // ── strip_selector / has_selector_prefix unit tests ─────────────

    #[test]
    fn test_strip_selector_variants() {
        assert_eq!(strip_selector("❯ 1. Yes"), "1. Yes");
        assert_eq!(strip_selector("> 1. Yes"), "1. Yes");
        assert_eq!(strip_selector("\u{00e2}\u{00af} 1. Yes"), "1. Yes");
        // Callers always pass .trim()'d input, so no leading spaces
        assert_eq!(strip_selector("1. Yes"), "1. Yes"); // no selector present
        assert_eq!(strip_selector("1. Yes"), "1. Yes");
    }

    #[test]
    fn test_has_selector_prefix_variants() {
        assert!(has_selector_prefix("❯ 1. Yes"));
        assert!(has_selector_prefix("> 1. Yes"));
        assert!(has_selector_prefix("\u{00e2}\u{00af} 1. Yes"));
        assert!(!has_selector_prefix("1. Yes"));
        assert!(!has_selector_prefix("  1. Yes"));
    }

    #[test]
    fn test_trust_directory_prompt_detected() {
        let text = "\
? Do you trust the contents of this directory?

  1. Yes, continue
  2. No, quit

❯ 1. Yes, continue

Press enter to continue";
        assert!(detect_interactive_prompt(text));
    }

    #[test]
    fn test_trust_directory_prompt_parsed() {
        let text = "\
? Do you trust the contents of this directory?

  1. Yes, continue
  2. No, quit

❯ 1. Yes, continue

Press enter to continue";
        let result = parse_permission_prompt(text);
        assert!(result.is_some());
        let (question, options) = result.unwrap();
        assert!(question.contains("trust"));
        // First option label starts with "Yes" — this is what auto-approve checks
        assert!(options[0].0.starts_with("Yes"));
        assert_eq!(options[0].1, "1");
    }
}
