use crate::ScreenStateHandle;

/// Number of trailing non-empty lines treated as the "live" prompt region.
/// A TUI menu is anchored at the bottom while active; once resolved it scrolls
/// up into history. Scoping detection here is what stops a stale/gone prompt
/// from triggering (#22).
const LIVE_PROMPT_LINES: usize = 12;

/// Surface a non-blocking notice when an account/rate-limit prompt is visible.
///
/// This NEVER blocks input — the user must always be able to send (#22). An
/// earlier version hard-refused injection (`Err`), which false-positived on a
/// stale/resolved rate-limit menu and locked the user out of sending. We now
/// only notify (log); the caller proceeds regardless.
pub(crate) fn notify_if_unsafe_prompt(screen_state: &ScreenStateHandle, pty_id: &str) {
    let Some(screen) = screen_state.lock().get_screen_text(pty_id) else {
        return;
    };
    let Some(reason) = classify_unsafe_prompt_for_injection(&screen) else {
        return;
    };
    let screen_tail = last_screen_chars(&screen, 600);
    log::warn!(
        "[inject/safety] unsafe prompt visible for pty {} — NOT blocking send (notify-only, #22): {}; tail={:?}",
        &pty_id[..pty_id.len().min(8)],
        reason,
        screen_tail
    );
}

/// Classify the LIVE prompt region only. Returns `Some(reason)` when an active
/// account/rate-limit menu is on screen, `None` otherwise (including a prompt
/// that has already scrolled out of the live region — the staleness fix).
pub(crate) fn classify_unsafe_prompt_for_injection(screen: &str) -> Option<String> {
    let live = live_tail_region(screen, LIVE_PROMPT_LINES);
    let lower = live.to_ascii_lowercase();
    let has_interactive_footer = lower.contains("enter to confirm")
        || lower.contains("esc to cancel")
        || lower.contains("what do you want to do?");
    if !has_interactive_footer {
        return None;
    }

    let unsafe_markers = [
        "/rate-limit-options",
        "rate limit",
        "session limit",
        "usage limit",
        "upgrade your plan",
        "upgrade to team plan",
        "stop and wait for limit",
        "sign out",
        "log out",
        "logout",
        "delete account",
    ];
    unsafe_markers
        .iter()
        .find(|marker| lower.contains(**marker))
        .map(|marker| format!("account/rate-limit prompt matched {}", marker))
}

/// The last `max_lines` non-empty lines of the screen, joined — the region a
/// live TUI prompt occupies. Blank lines are dropped first so wrapping/padding
/// doesn't push the menu out of the window.
fn live_tail_region(screen: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

pub(crate) fn last_screen_chars(screen: &str, max_chars: usize) -> String {
    let total = screen.chars().count();
    screen
        .chars()
        .skip(total.saturating_sub(max_chars))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_prompt_classifier_blocks_claude_rate_limit_menu() {
        let screen = "\
/rate-limit-options

What do you want to do?

❯ 1. Stop and wait for limit to reset
  2. Upgrade your plan
  3. Upgrade to Team plan

Enter to confirm · Esc to cancel";

        let reason = classify_unsafe_prompt_for_injection(screen).expect("rate limit blocked");
        assert!(reason.contains("rate-limit"));
    }

    #[test]
    fn unsafe_prompt_classifier_blocks_account_sign_out_menu() {
        let screen = "\
Account action

❯ 1. Sign out
  2. Cancel

Enter to confirm · Esc to cancel";

        let reason = classify_unsafe_prompt_for_injection(screen).expect("sign-out blocked");
        assert!(reason.contains("sign out"));
    }

    #[test]
    fn unsafe_prompt_classifier_allows_trust_directory_prompt() {
        let screen = "\
? Do you trust the contents of this directory?

❯ 1. Yes, continue
  2. No, quit

Enter to confirm · Esc to cancel";

        assert_eq!(classify_unsafe_prompt_for_injection(screen), None);
    }

    /// #22 staleness: a rate-limit menu that has already scrolled out of the
    /// live region (resolved, with fresh output below) must NOT trigger. This
    /// is the false-positive that locked the user out of sending.
    #[test]
    fn unsafe_prompt_classifier_ignores_stale_scrolled_prompt() {
        let mut screen = String::from(
            "\
/rate-limit-options

What do you want to do?

❯ 1. Stop and wait for limit to reset
  2. Upgrade your plan

Enter to confirm · Esc to cancel
",
        );
        // Limit reset; the menu is gone and the session resumed — plenty of
        // fresh output now sits below the old menu.
        for i in 0..30 {
            screen.push_str(&format!("resumed work line {i}\n"));
        }
        assert_eq!(
            classify_unsafe_prompt_for_injection(&screen),
            None,
            "a scrolled-away menu must not be treated as an active prompt"
        );
    }

    /// The live menu (footer still at the bottom) is still detected, so the
    /// non-blocking notify still fires for a genuinely active prompt.
    #[test]
    fn unsafe_prompt_classifier_still_detects_live_menu_at_bottom() {
        let screen = "\
some earlier output
/rate-limit-options
What do you want to do?
❯ 1. Stop and wait for limit to reset
  2. Upgrade your plan
Enter to confirm · Esc to cancel";
        let reason =
            classify_unsafe_prompt_for_injection(screen).expect("live menu must be detected");
        assert!(reason.contains("rate-limit"));
    }
}
