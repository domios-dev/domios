//! Authorization for messages originating from tracked roleless agent panes.
//!
//! A roleless spawn has no orchestration hierarchy vouching for it, so it must
//! not freely message the well-known General Agent aliases (`general`/`main`),
//! which are the panes most likely to hold secrets/infra authority. This is a
//! pure access-control decision — it runs before any delivery and never touches
//! the PTY write path.

/// Well-known aliases for the local General Agent. A roleless pane reaching
/// these is exactly the incident class in the bug report.
const PROTECTED_ALIASES: [&str; 2] = ["general", "main"];

/// Whether a send target names a protected General Agent alias (case-insensitive).
pub fn target_is_protected_alias(raw_target: &str) -> bool {
    let target = raw_target.trim();
    PROTECTED_ALIASES
        .iter()
        .any(|alias| target.eq_ignore_ascii_case(alias))
}

/// Whether a send from a tracked roleless pane to `raw_target` must be blocked.
/// Only tracked-roleless senders are restricted, and only toward protected
/// aliases; every other combination passes through unchanged.
pub fn is_send_blocked(sender_roleless_tracked: bool, raw_target: &str) -> bool {
    sender_roleless_tracked && target_is_protected_alias(raw_target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_aliases_match_case_insensitively() {
        assert!(target_is_protected_alias("general"));
        assert!(target_is_protected_alias("General"));
        assert!(target_is_protected_alias("MAIN"));
        assert!(target_is_protected_alias("  main  "));
    }

    #[test]
    fn ordinary_tab_ids_are_not_protected() {
        assert!(!target_is_protected_alias("tab_abc123"));
        assert!(!target_is_protected_alias("worker-3"));
        assert!(!target_is_protected_alias(""));
    }

    #[test]
    fn roleless_pane_blocked_from_general_and_main() {
        assert!(is_send_blocked(true, "general"));
        assert!(is_send_blocked(true, "main"));
    }

    #[test]
    fn roleless_pane_may_still_message_ordinary_tabs() {
        assert!(!is_send_blocked(true, "tab_peer"));
    }

    #[test]
    fn tracked_pane_with_role_is_never_restricted() {
        // A pane that carries a real role is not `roleless_tracked`; it passes.
        assert!(!is_send_blocked(false, "general"));
        assert!(!is_send_blocked(false, "main"));
    }
}
