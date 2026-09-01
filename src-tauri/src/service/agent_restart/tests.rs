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

fn guard_snapshot(session_id: Option<&str>) -> TabRestartSnapshot {
    TabRestartSnapshot {
        tab_id: "tab-guard".into(),
        pty_id: "pty-guard".into(),
        provider: "claude".into(),
        session_id: session_id.map(str::to_string),
        cwd: Some("/tmp/project".into()),
        auto_approve: false,
        role: None,
        role_profile: None,
    }
}

#[test]
fn resume_guard_declares_failure_on_missing_evidenced_transcript_path() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        codeterm_plugin::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("store"),
    );

    let sid = "guard-sid-evidenced-missing";
    let gone = tmp.path().join("moved-away.jsonl");
    assert!(codeterm_plugin::session_evidence::record(
        sid,
        gone.to_str().unwrap()
    ));
    // The evidenced transcript never exists on disk — the guard must convert
    // that into a declared failure instead of letting a command be built.
    let reason = evidenced_transcript_block(&guard_snapshot(Some(sid)))
        .expect("evidenced-but-missing transcript must block the resume");
    assert!(
        reason.contains("resume unavailable: transcript missing"),
        "declared failure names the condition: {reason}"
    );
    assert!(
        reason.contains("relaunch fresh"),
        "declared failure carries the fresh-launch offer: {reason}"
    );
}

#[test]
fn resume_guard_passes_when_evidenced_transcript_path_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        codeterm_plugin::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("store"),
    );

    let sid = "guard-sid-evidenced-present";
    let alive = tmp.path().join("alive-transcript.jsonl");
    std::fs::write(&alive, "{}\n").unwrap();
    assert!(codeterm_plugin::session_evidence::record(
        sid,
        alive.to_str().unwrap()
    ));
    assert_eq!(
        evidenced_transcript_block(&guard_snapshot(Some(sid))),
        None,
        "an existing evidenced transcript lets the resume proceed"
    );
}

#[test]
fn resume_guard_without_transcript_path_evidence_keeps_todays_behavior() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        codeterm_plugin::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("empty-store"),
    );

    // No evidence recorded ⇒ the guard must not fire, for a bound session or none.
    assert_eq!(
        evidenced_transcript_block(&guard_snapshot(Some("unevidenced-sid"))),
        None
    );
    assert_eq!(evidenced_transcript_block(&guard_snapshot(None)), None);
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
        None,
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
        None,
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
        None,
        None,
        None,
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
        binding_restored: true,
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
fn build_relaunch_uses_resolved_full_access() {
    let registry = registry_with_claude();
    let resume = build_relaunch_command_with_registry(
        &registry,
        "claude",
        Some("sid"),
        true,
        true,
        None,
        None,
        None,
    )
    .expect("resume command");
    assert!(resume.contains("--dangerously-skip-permissions"));

    let fresh = build_relaunch_command_with_registry(
        &registry,
        "claude",
        Some("sid"),
        true,
        false,
        None,
        None,
        None,
    )
    .expect("fresh command");
    assert!(fresh.contains("--dangerously-skip-permissions"));

    let restricted = build_relaunch_command_with_registry(
        &registry,
        "claude",
        Some("sid"),
        false,
        true,
        None,
        None,
        None,
    )
    .expect("restricted resume command");
    assert!(!restricted.contains("--dangerously-skip-permissions"));
}

#[test]
fn agent_restart_access_does_not_depend_on_tab_class() {
    let registry = registry_with_claude();
    let visible =
        crate::service::permission_policy::authorize_permission_bypass(&registry, "claude", true)
            .is_some();
    let hidden =
        crate::service::permission_policy::authorize_permission_bypass(&registry, "claude", true)
            .is_some();
    let automation =
        crate::service::permission_policy::authorize_permission_bypass(&registry, "claude", true)
            .is_some();

    assert!(visible);
    assert!(hidden);
    assert!(automation);
    let command = build_relaunch_command_with_registry(
        &registry,
        "claude",
        Some("sid"),
        hidden,
        true,
        None,
        None,
        None,
    )
    .expect("resume command");
    assert!(command.contains("--dangerously-skip-permissions"));
}

#[test]
fn build_relaunch_drops_resume_without_session_id() {
    let registry = registry_with_claude();
    let command = build_relaunch_command_with_registry(
        &registry, "claude", None, true, true, None, None, None,
    )
    .expect("fresh launch without session id");
    assert!(!command.contains("--resume"));
}

#[test]
fn fresh_roleless_relaunch_keeps_system_context_without_a_user_turn() {
    let registry = registry_with_claude();
    let command = build_relaunch_command_with_registry(
        &registry,
        "claude",
        None,
        true,
        false,
        None,
        Some("system context"),
        None,
    )
    .expect("fresh roleless launch");

    assert!(command.contains("--append-system-prompt-file"));
    assert!(!command.contains("Execute the CodeTerm task"));
}

#[test]
fn restored_worker_never_replays_historical_task_or_wakes() {
    let registry = registry_with_claude();
    let command = build_relaunch_command_with_registry(
        &registry,
        "claude",
        Some("existing-session"),
        true,
        false,
        None,
        Some("restored worker system context"),
        Some("Worker"),
    )
    .expect("restored worker relaunch");

    assert!(command.contains("--append-system-prompt-file"));
    assert!(!command.contains("historical task metadata"));
    assert!(!command.contains("Execute the CodeTerm task"));
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

fn settle_snapshot_and_outcome(tab_id: &str) -> (TabRestartSnapshot, RestartOutcome) {
    let snap = TabRestartSnapshot {
        tab_id: tab_id.into(),
        pty_id: "pty-new".into(),
        provider: "claude".into(),
        session_id: Some("sess-a".into()),
        cwd: None,
        auto_approve: false,
        role: None,
        role_profile: None,
    };
    let outcome = RestartOutcome {
        tab_id: tab_id.into(),
        provider: "claude".into(),
        session_id: Some("sess-a".into()),
        session_was_stale: false,
        force: true,
        auto_approve: false,
        restart_accepted: true,
        old_pty_torn_down: true,
        agent_ready: true,
        binding_restored: false,
        old_pty_id: Some("pty-old".into()),
        new_pty_id: Some("pty-new".into()),
    };
    (snap, outcome)
}

/// codeterm-releases#55, through the real settle path: a relaunched tab
/// with no reported session yet settles successfully, not as foreign.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn settle_restored_binding_treats_unreported_session_as_success() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("restart".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_id = None;
    let (snap, outcome) = settle_snapshot_and_outcome(&tab_id);

    let result = fixture.service.settle_restored_binding(outcome, &snap);

    assert!(
        result.is_ok(),
        "an unreported session must settle successfully"
    );
}

/// Guard: a genuinely different reported session still fails the settle.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn settle_restored_binding_still_refuses_a_genuinely_foreign_session() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("restart".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_id = Some("foreign-session".into());
    let (snap, outcome) = settle_snapshot_and_outcome(&tab_id);

    let result = fixture.service.settle_restored_binding(outcome, &snap);

    assert!(
        result.is_err(),
        "a genuinely foreign session must still fail"
    );
}
