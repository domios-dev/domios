use super::inject_verify::InjectionError;
use crate::ScreenStateHandle;

pub(crate) fn refuse_if_unsafe_prompt(
    screen_state: &ScreenStateHandle,
    pty_id: &str,
) -> Result<(), InjectionError> {
    let Some(screen) = screen_state.lock().get_screen_text(pty_id) else {
        return Ok(());
    };
    let Some(reason) = classify_unsafe_prompt_for_injection(&screen) else {
        return Ok(());
    };
    let screen_tail = last_screen_chars(&screen, 600);
    log::warn!(
        "[inject/safety] refusing input for pty {}: {}; tail={:?}",
        &pty_id[..pty_id.len().min(8)],
        reason,
        screen_tail
    );
    Err(InjectionError::UnsafePromptVisible {
        reason,
        screen_tail,
    })
}

pub(crate) fn classify_unsafe_prompt_for_injection(screen: &str) -> Option<String> {
    let lower = screen.to_ascii_lowercase();
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
}
