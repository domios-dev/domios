use super::*;

fn fresh_tracker() -> RestartTracker {
    new_restart_tracker()
}

fn check(tracker: &RestartTracker, tab_id: &str) -> Result<(), AgentRestartError> {
    let mut guard = tracker.lock();
    let now = Instant::now();
    if let Some(last) = guard.get(tab_id) {
        let elapsed = now.duration_since(*last);
        let limit = Duration::from_secs(RESTART_RATE_LIMIT_SECS);
        if elapsed < limit {
            let retry = (limit - elapsed).as_secs().max(1);
            return Err(AgentRestartError::RateLimited {
                retry_after_secs: retry,
            });
        }
    }
    guard.insert(tab_id.to_string(), now);
    Ok(())
}

#[test]
fn rate_limit_first_call_succeeds() {
    let tracker = fresh_tracker();
    assert!(check(&tracker, "tab-1").is_ok());
}

#[test]
fn rate_limit_second_call_blocked() {
    let tracker = fresh_tracker();
    check(&tracker, "tab-1").unwrap();
    match check(&tracker, "tab-1") {
        Err(AgentRestartError::RateLimited { retry_after_secs }) => {
            assert!(retry_after_secs > 0);
            assert!(retry_after_secs <= RESTART_RATE_LIMIT_SECS);
        }
        other => panic!("expected RateLimited, got {:?}", other),
    }
}

#[test]
fn rate_limit_independent_tabs() {
    let tracker = fresh_tracker();
    check(&tracker, "tab-a").unwrap();
    assert!(check(&tracker, "tab-b").is_ok());
}

#[test]
fn rate_limit_resets_after_window() {
    let tracker = fresh_tracker();
    let past = Instant::now() - Duration::from_secs(RESTART_RATE_LIMIT_SECS + 1);
    tracker.lock().insert("tab-1".to_string(), past);
    assert!(check(&tracker, "tab-1").is_ok());
}

fn registry_with_claude() -> crate::agent::registry::AgentRegistry {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    registry
}

#[test]
fn build_relaunch_falls_back_to_fresh_when_session_stale() {
    let registry = registry_with_claude();
    let command = build_relaunch_command_with_registry(
        &registry,
        "claude",
        Some("ghost-id"),
        true,
        false,
        None,
    )
    .expect("fallback launch command");
    assert!(!command.contains("--resume"));
    assert!(command.contains("claude"));
}

#[test]
fn build_relaunch_uses_resume_when_session_present() {
    let registry = registry_with_claude();
    let command = build_relaunch_command_with_registry(
        &registry,
        "claude",
        Some("abc-123"),
        true,
        true,
        None,
    )
    .expect("resume command");
    assert!(command.contains("--resume"));
    assert!(command.contains("abc-123"));
}

#[test]
fn build_relaunch_unknown_provider_returns_none() {
    let registry = registry_with_claude();
    assert!(build_relaunch_command_with_registry(
        &registry,
        "provider-fixture",
        None,
        true,
        false,
        None
    )
    .is_none());
}

#[test]
fn screen_shows_agent_tui_catches_provider_text_fingerprint() {
    let registry = registry_with_claude();
    let footer = "some assistant output\n⏵⏵ bypass permissions on (shift+tab to cycle)\n❯ ";
    assert!(!crate::pty::screen_state::screen_has_agent_tui(footer));
    assert!(crate::pty::screen_state::screen_has_shell_prompt(footer));
    assert!(screen_shows_agent_tui(&registry, footer));
}

#[test]
fn screen_shows_agent_tui_catches_universal_glyphs() {
    let registry = registry_with_claude();
    let boxed =
        "╭──────────────────────────╮\n│ ❯                        │\n╰──────────────────────────╯";
    assert!(screen_shows_agent_tui(&registry, boxed));
}

#[test]
fn screen_shows_agent_tui_false_for_bare_shell() {
    let registry = registry_with_claude();
    assert!(!screen_shows_agent_tui(&registry, "user@host:~/codeterm$ "));
    assert!(!screen_shows_agent_tui(&registry, "~/code on master\n❯ "));
}

fn outcome(session_id: Option<&str>, stale: bool, auto_approve: bool) -> RestartOutcome {
    RestartOutcome {
        tab_id: "tab-1".into(),
        provider: "provider-fixture".into(),
        session_id: session_id.map(str::to_string),
        session_was_stale: stale,
        force: true,
        auto_approve,
        restart_accepted: true,
        old_pty_torn_down: true,
        agent_ready: true,
        old_pty_id: Some("pty-old".into()),
        new_pty_id: Some("pty-new".into()),
    }
}

#[test]
fn session_action_resume_when_present_and_fresh() {
    assert_eq!(outcome(Some("sid"), false, true).session_action(), "resume");
}

#[test]
fn session_action_fresh_when_stale_or_missing() {
    assert_eq!(outcome(Some("sid"), true, true).session_action(), "fresh");
    assert_eq!(outcome(None, false, true).session_action(), "fresh");
}

#[test]
fn permission_profile_reflects_auto_approve() {
    assert_eq!(outcome(None, false, true).permission_profile(), "yolo");
    assert_eq!(
        outcome(None, false, false).permission_profile(),
        "sandboxed"
    );
}

#[test]
fn build_relaunch_preserves_permission_profile() {
    let registry = registry_with_claude();
    let resume =
        build_relaunch_command_with_registry(&registry, "claude", Some("sid"), true, true, None)
            .expect("resume command");
    assert!(resume.contains("--dangerously-skip-permissions"));

    let fresh =
        build_relaunch_command_with_registry(&registry, "claude", Some("sid"), true, false, None)
            .expect("fresh command");
    assert!(fresh.contains("--dangerously-skip-permissions"));

    let sandboxed =
        build_relaunch_command_with_registry(&registry, "claude", Some("sid"), false, true, None)
            .expect("sandboxed resume command");
    assert!(!sandboxed.contains("--dangerously-skip-permissions"));
}

#[test]
fn build_relaunch_drops_resume_without_session_id() {
    let registry = registry_with_claude();
    let command = build_relaunch_command_with_registry(&registry, "claude", None, true, true, None)
        .expect("fresh launch without session id");
    assert!(!command.contains("--resume"));
}

#[test]
fn stale_pre_restart_agent_ready_without_pty_turnover_is_not_ready() {
    let readiness = crate::service::boot_stage::classify_restart_readiness(
        Some("pty-old"),
        Some(codeterm_types::pty::BootStage::AgentReady),
        None,
        "pty-old",
    );

    assert_eq!(readiness, None);
}

#[test]
fn genuine_pty_turnover_reports_new_agent_ready() {
    let readiness = crate::service::boot_stage::classify_restart_readiness(
        Some("pty-new"),
        Some(codeterm_types::pty::BootStage::AgentReady),
        None,
        "pty-old",
    );

    assert_eq!(
        readiness,
        Some(crate::service::boot_stage::RestartReadiness::Ready {
            pty_id: "pty-new".into(),
        })
    );
}
