use super::*;

fn terminal(trust: &TrustDecision, modal_on_screen: bool) -> Option<AgentTerminal<'_>> {
    Some(AgentTerminal {
        trust,
        modal_on_screen,
    })
}

#[test]
fn free_agent_terminal_takes_the_reminder() {
    assert_eq!(
        nudge_for(true, true, terminal(&TrustDecision::NoDialog, false)),
        StartupNudge::Deliver
    );
    assert_eq!(
        nudge_for(true, true, terminal(&TrustDecision::NotApplicable, false)),
        StartupNudge::Deliver
    );
}

#[test]
fn a_tab_without_an_agent_is_never_written_to() {
    assert_eq!(
        nudge_for(false, true, terminal(&TrustDecision::NoDialog, false)),
        StartupNudge::NoAgent
    );
    assert_eq!(nudge_for(false, false, None), StartupNudge::NoAgent);
}

#[test]
fn a_chat_backed_agent_has_no_terminal_to_collide_with() {
    assert_eq!(nudge_for(true, true, None), StartupNudge::Deliver);
}

#[test]
fn an_unbound_session_never_receives_reminders_on_clear_or_modal_screens() {
    let no_dialog = TrustDecision::NoDialog;
    let held = TrustDecision::Held;
    for terminal in [terminal(&no_dialog, false), terminal(&held, true)] {
        let nudge = nudge_for(true, false, terminal);
        assert_eq!(nudge, StartupNudge::UnboundSession);
        assert!(!nudge.delivers());
    }
}

#[test]
fn an_owned_trust_dialog_is_answered_instead_of_written_to() {
    assert_eq!(
        nudge_for(
            true,
            true,
            terminal(&TrustDecision::Answer { step: b"\r".into() }, false)
        ),
        StartupNudge::AnsweredTrustDialog
    );
    assert_eq!(
        nudge_for(true, true, terminal(&TrustDecision::Waiting, false)),
        StartupNudge::AnsweredTrustDialog
    );
}

#[test]
fn a_prompt_the_host_may_not_answer_blocks_the_reminder() {
    assert_eq!(
        nudge_for(true, true, terminal(&TrustDecision::Held, false)),
        StartupNudge::BlockedOnPrompt
    );
    // No provenance, so nothing to answer — and still nowhere to paste prose.
    assert_eq!(
        nudge_for(true, true, terminal(&TrustDecision::NotApplicable, true)),
        StartupNudge::BlockedOnPrompt
    );
    assert_eq!(
        nudge_for(true, true, terminal(&TrustDecision::NoDialog, true)),
        StartupNudge::BlockedOnPrompt
    );
}

#[test]
fn a_codex_update_dialog_blocks_startup_reminders() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../plugins/provider-codex/tests/fixtures/directory-trust/codex-update-available.txt",
    );
    let screen = std::fs::read_to_string(path).expect("Codex update fixture");
    let modal_on_screen = crate::agent::detector::detect_interactive_prompt(&screen);
    assert!(modal_on_screen);
    assert_eq!(
        nudge_for(
            true,
            true,
            terminal(&TrustDecision::NoDialog, modal_on_screen)
        ),
        StartupNudge::BlockedOnPrompt
    );
}

#[test]
fn only_deliver_writes() {
    assert!(StartupNudge::Deliver.delivers());
    for nudge in [
        StartupNudge::AnsweredTrustDialog,
        StartupNudge::BlockedOnPrompt,
        StartupNudge::UnboundSession,
        StartupNudge::NoAgent,
    ] {
        assert!(!nudge.delivers(), "{nudge:?} must not write into a tab");
    }
}
