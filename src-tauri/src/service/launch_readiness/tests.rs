use super::*;
use crate::service::boot_stage::{
    apply_boot_stage, honest_boot_stage, late_readiness_retires_provisional_launch,
    provisional_launch_for,
};
use std::time::Duration;

/// A launch that wrote its command and has proved nothing yet.
fn launching_tab(pty_id: &str, nonce: &str) -> Tab {
    let (mut state, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&workspace_id).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some(pty_id.into());
    tab.agent_type = Some("claude".into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.reported_session_binding_nonce = None;
    tab.agent_session_id = None;
    tab.boot_stage = Some(BootStage::LaunchWritten);
    tab.clone()
}

fn attempt_of(tab: &Tab) -> LaunchAttempt {
    LaunchAttempt {
        tab_id: tab.id.clone(),
        pty_id: tab.pty_id.clone().expect("launching tab owns a PTY"),
        binding_nonce: tab
            .session_binding_nonce
            .clone()
            .expect("launching tab carries its launch nonce"),
    }
}

#[test]
fn provider_exit_evidence_ends_resume_first_turn_waiting() {
    let mut tab = launching_tab("pty-resume-exit", "nonce-resume-exit");
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_booting = false;
    let attempt = attempt_of(&tab);

    assert!(resume_attempt_is_waiting_for_first_turn(&tab, &attempt));

    tab.exit_suspected_at = Some(std::time::Instant::now());
    assert!(!resume_attempt_is_waiting_for_first_turn(&tab, &attempt));
}

/// The agent naming itself on this launch: its own accepted session plus this
/// attempt's nonce echoed back.
fn report_own_launch(tab: &mut Tab) {
    tab.agent_session_id = Some("01a090f2".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
}

/// A generous bound proves the wait is released by evidence, not by elapsed
/// time; nothing in these tests sleeps to reach its outcome.
const BOUND: Duration = Duration::from_secs(30);

/// Both exit sources claim before either publishes, so only the first claim
/// may win; otherwise one dead launch is reported twice.
#[test]
fn launch_readiness_only_the_first_observer_claims_one_exit() {
    let tab = launching_tab("pty-once", "nonce-once");
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    assert!(
        claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3)).is_some(),
        "the first observer of an exit claims it"
    );
    assert!(
        claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3)).is_none(),
        "a second observer must not claim the same exit before it is published"
    );
    assert!(
        claim_exit(&attempt.tab_id, None, None).is_none(),
        "nor may the funnel that reports no pty claim it again"
    );
}

#[test]
fn launch_readiness_local_exit_emits_bounded_failure() {
    let tab = launching_tab("pty-local", "nonce-local");
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    let (claimed, exit_code) = claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3))
        .expect("an armed launch claims the exit observed against it");
    assert_eq!(claimed, attempt);
    assert_eq!(exit_code, Some(3));
    // A claim leaves no evidence behind it, so the waiting observer finds none.
    assert_eq!(
        observe(&attempt, Instant::now()),
        LaunchObservation::Deadline
    );

    let settlement = settle_evidence(
        &tab,
        &attempt,
        LaunchEvidence::ProcessExited { exit_code: Some(3) },
    );
    let LaunchSettlement::Failed { reason, exit_code } = settlement else {
        panic!("a pre-readiness exit is a launch failure, not uncertainty");
    };
    assert_eq!(exit_code, Some(3));
    assert!(reason.contains(&attempt.tab_id));
    assert!(reason.contains(&format!("attempt={}", attempt.pty_id)));
    assert!(reason.contains(&format!("launch_nonce={}", attempt.binding_nonce)));
    assert!(reason.contains("exit_code=3"));
}

/// A relayed spawn is the same ordinary spawn on the peer, so the same evidence
/// settles it the same way. Nothing here branches on the origin peer.
#[test]
fn launch_readiness_mesh_exit_emits_bounded_failure() {
    let mut tab = launching_tab("pty-mesh", "nonce-mesh");
    tab.report_target_mesh_peer = Some("dev-peer".into());
    tab.spawned_by_tab_id = Some("origin-tab".into());
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(1)).is_some());

    let mut local = tab.clone();
    local.report_target_mesh_peer = None;
    local.spawned_by_tab_id = None;
    let relayed = settle_evidence(
        &tab,
        &attempt,
        LaunchEvidence::ProcessExited { exit_code: Some(1) },
    );
    let direct = settle_evidence(
        &local,
        &attempt,
        LaunchEvidence::ProcessExited { exit_code: Some(1) },
    );
    assert_eq!(relayed, direct);
    assert!(matches!(relayed, LaunchSettlement::Failed { .. }));
}

#[test]
fn launch_readiness_mesh_relay_reaches_the_same_spawn_contract() {
    let relay = include_str!("../../api/mesh_routes/relay.rs");
    let routes = include_str!("../../api/agent_routes.rs");

    assert!(relay.contains("/api/agents/spawn"));
    assert!(routes.contains(".spawn_agent(params, ready_tx)"));
    for source in [relay, routes] {
        assert!(!source.contains("launch_readiness"));
        assert!(!source.contains("BootStage"));
    }
}

#[test]
fn launch_readiness_healthy_ack_is_not_failure() {
    let tab = launching_tab("pty-healthy", "nonce-healthy");
    let attempt = attempt_of(&tab);

    assert_eq!(
        settle_evidence(&tab, &attempt, LaunchEvidence::Acknowledged),
        LaunchSettlement::Ready,
    );

    arm(attempt.clone());
    assert!(note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce
    ));
    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(0)).is_none());
    assert_eq!(
        observe(&attempt, Instant::now() + BOUND),
        LaunchObservation::Evidence(LaunchEvidence::Acknowledged),
    );

    let mut proven = tab.clone();
    proven.boot_stage = Some(BootStage::ProviderReady);
    report_own_launch(&mut proven);
    assert!(proven.launch_session_authenticated());
    assert_eq!(
        honest_boot_stage(
            BootStage::SpawnFailed,
            proven.boot_stage,
            proven.launch_session_authenticated()
        ),
        BootStage::AgentReady,
    );
}

#[test]
fn launch_readiness_slow_ack_before_deadline_is_not_failure() {
    let tab = launching_tab("pty-slow", "nonce-slow");
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    let released = {
        let attempt = attempt.clone();
        std::thread::spawn(move || {
            note_acknowledged(&attempt.tab_id, &attempt.pty_id, &attempt.binding_nonce)
        })
    };

    let observed = observe(&attempt, Instant::now() + BOUND);
    assert!(released.join().expect("the releasing thread finished"));
    assert_eq!(
        observed,
        LaunchObservation::Evidence(LaunchEvidence::Acknowledged),
    );
    assert_eq!(
        settle_evidence(&tab, &attempt, LaunchEvidence::Acknowledged),
        LaunchSettlement::Ready,
    );
}

#[test]
fn launch_readiness_deadline_is_provisional_not_a_verdict() {
    let mut tab = launching_tab("pty-quiet", "nonce-quiet");
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    assert_eq!(
        observe(&attempt, Instant::now()),
        LaunchObservation::Deadline
    );

    let settlement = settle_evidence(
        &tab,
        &attempt,
        LaunchEvidence::Unobserved {
            stage: BootStage::LaunchDeadlineElapsed,
            reason: SPAWN_READINESS_DEADLINE.to_string(),
        },
    );
    assert_eq!(
        settlement,
        LaunchSettlement::Provisional {
            stage: BootStage::LaunchDeadlineElapsed,
            reason: SPAWN_READINESS_DEADLINE.to_string(),
        },
    );
    assert!(BootStage::LaunchDeadlineElapsed.is_provisional());

    let latch = provisional_launch_for(&tab, &attempt.pty_id, SPAWN_READINESS_DEADLINE)
        .expect("an unresolved current launch is latchable");
    apply_boot_stage(
        &mut tab,
        BootStage::LaunchDeadlineElapsed,
        Some(SPAWN_READINESS_DEADLINE),
    );
    tab.provisional_launch = Some(latch);

    report_own_launch(&mut tab);
    let evidence = crate::service::ProviderSessionBinding {
        pty_id: attempt.pty_id.clone(),
        binding_nonce: attempt.binding_nonce.clone(),
    };
    assert!(late_readiness_retires_provisional_launch(&tab, &evidence));
}

#[test]
fn launch_readiness_stale_session_does_not_settle_current_attempt() {
    let tab = launching_tab("pty-current", "nonce-current");
    let attempt = attempt_of(&tab);

    let mut relinked = tab.clone();
    relinked.pty_id = Some("pty-prior".into());
    assert_eq!(
        settle_evidence(
            &relinked,
            &attempt,
            LaunchEvidence::ProcessExited { exit_code: Some(3) }
        ),
        LaunchSettlement::Superseded,
    );

    let mut rotated = tab.clone();
    rotated.session_binding_nonce = Some("nonce-next".into());
    assert_eq!(
        settle_evidence(&rotated, &attempt, LaunchEvidence::Acknowledged),
        LaunchSettlement::Superseded,
    );

    arm(attempt.clone());
    assert!(claim_exit(&attempt.tab_id, Some("pty-foreign"), Some(3)).is_none());
    assert!(claim_exit("tab-foreign", None, Some(3)).is_none());
    assert!(!note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        "nonce-foreign"
    ));
    assert!(!note_acknowledged(
        "tab-foreign",
        &attempt.pty_id,
        &attempt.binding_nonce
    ));
    assert_eq!(
        observe(&attempt, Instant::now()),
        LaunchObservation::Deadline
    );
}

/// The General Agent start keeps its own provisional vocabulary; only the owner
/// that applies it moved.
#[test]
fn launch_readiness_general_agent_start_keeps_its_provisional_contract() {
    assert_eq!(
        GENERAL_AGENT_OUTPUT_DEADLINE,
        "agent output deadline elapsed"
    );

    let lifecycle = include_str!("../general_agent/lifecycle.rs");
    assert!(lifecycle.contains("settle_launch_readiness"));
    assert!(lifecycle.contains("GENERAL_AGENT_OUTPUT_DEADLINE"));
    assert!(lifecycle.contains("BootStage::LaunchBindingUnconfirmed"));
    assert!(!lifecycle.contains("svc.record_launch_output_deadline"));
    assert!(!lifecycle.contains("svc.record_provisional_launch"));
}

#[test]
fn launch_readiness_shared_owner_is_not_duplicated() {
    let owner = include_str!("../launch_readiness.rs");
    let lifecycle = include_str!("../general_agent/lifecycle.rs");
    let launch = include_str!("../orchestrator/launch.rs");
    let startup = include_str!("../orchestrator/launch/startup_readiness.rs");

    assert!(launch.contains("launch_readiness::arm("));
    assert!(launch.contains("observe_launch_readiness"));
    assert!(launch.contains("startup_readiness::emit_agent_ready"));
    assert!(!launch.contains("take_published_failure"));
    assert_eq!(owner.matches("take_published_failure(&attempt)").count(), 1);
    assert!(startup.contains("LaunchSettlement::Failed"));

    assert_eq!(owner.matches("\"agent-launch-failed\"").count(), 1);
    for source in [lifecycle, launch] {
        assert!(!source.contains("agent-launch-failed"));
    }
}

/// The failure is built from launch identity alone: no screen text, no provider
/// wording, no sleep or poll anywhere in the owner.
#[test]
fn launch_readiness_failure_needs_no_terminal_text() {
    let tab = launching_tab("pty-quiet-exit", "nonce-quiet-exit");
    let attempt = attempt_of(&tab);
    let reason = exited_before_readiness_reason(&attempt, None);
    assert!(reason.starts_with(EXITED_BEFORE_READINESS));
    assert!(reason.contains("exit_code=unknown"));

    let owner = include_str!("../launch_readiness.rs");
    for scraped in [
        "get_screen_text",
        "detect_from_output",
        "screen_has_tui",
        "thread::sleep",
        "sleep(",
    ] {
        assert!(!owner.contains(scraped), "owner must not use {scraped}");
    }
}

/// The committed live oracle: both modes and the whole control contract are
/// self-evident in the fixture, and it reaches readiness on an event.
#[test]
fn launch_readiness_fixture_exposes_both_deterministic_modes() {
    let fixture = include_str!("../../../../scripts/fixtures/agent-launch-readiness.mjs");

    for declared in [
        "exit-before-readiness",
        "slow-ready",
        "CODETERM_STUB_AGENT_CMD",
        "CODETERM_READINESS_FIXTURE_CONTROL",
        "hook/session_start",
        "binding_nonce",
    ] {
        assert!(
            fixture.contains(declared),
            "fixture must document {declared}"
        );
    }
    for timed in ["setTimeout", "setInterval", "Atomics.wait"] {
        assert!(!fixture.contains(timed), "fixture must not use {timed}");
    }
    assert!(fixture.contains("watch("));
}

/// The live failure this round fixes. The shell reports a finished command with
/// an EMPTY command line (`ESC ] 633 ; E ; ; <shell-nonce> BEL`), so an exit
/// detector that asks the command line to name the launch rejects the only
/// evidence there is. Nothing here may depend on that text.
#[test]
fn launch_readiness_exit_settles_without_a_shell_reported_command_line() {
    let tab = launching_tab("pty-marker-less", "nonce-marker-less");
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    let (claimed, exit_code) = claim_exit(&attempt.tab_id, Some(&attempt.pty_id), None)
        .expect("an exit with no shell-reported detail still claims the launch");
    assert_eq!(claimed, attempt);
    assert_eq!(exit_code, None);
    assert!(matches!(
        settle_evidence(
            &tab,
            &attempt,
            LaunchEvidence::ProcessExited { exit_code: None }
        ),
        LaunchSettlement::Failed { .. }
    ));

    let owner = include_str!("../launch_readiness.rs");
    assert!(!owner.contains("command_line"));
}

/// The second half of the same live failure: every observer that sees an exit
/// also tears the tab's agent identity down, and the teardown clears the launch
/// nonce. A settlement attempted afterwards can only be Superseded, so the
/// settle must come first at every call site.
#[test]
fn launch_readiness_exit_settles_before_identity_teardown() {
    let tab = launching_tab("pty-teardown", "nonce-teardown");
    let attempt = attempt_of(&tab);
    let exited = LaunchEvidence::ProcessExited { exit_code: Some(3) };

    assert!(matches!(
        settle_evidence(&tab, &attempt, exited.clone()),
        LaunchSettlement::Failed { .. }
    ));

    let mut torn_down = tab.clone();
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(&mut torn_down));
    assert!(torn_down.session_binding_nonce.is_none());
    assert_eq!(
        settle_evidence(&torn_down, &attempt, exited),
        LaunchSettlement::Superseded,
    );

    // Claim, tear down, then publish: the verdict has to be decided while the
    // tab still carries the launch, and recorded after the teardown resets it.
    for (source, teardown_anchor) in [
        (
            include_str!("../output.rs"),
            "note_provider_exit_in_surviving_shell",
        ),
        (
            include_str!("../tab.rs"),
            "is_some_and(clear_agent_identity_for_lifecycle_exit)",
        ),
    ] {
        let claim = source
            .find("claim_launch_exit")
            .expect("the exit source claims the launch");
        let teardown = source
            .find(teardown_anchor)
            .expect("the exit source tears identity down");
        let publish = source
            .find("publish_launch_exit")
            .expect("the exit source publishes the verdict");
        assert!(claim < teardown, "the claim must precede the teardown");
        assert!(teardown < publish, "the verdict must be published after it");
    }
}

/// A launch verdict is published after the teardown, so it is the stage that
/// stands even though the teardown resets the tab to `shell_ready`.
#[test]
fn launch_readiness_failed_launch_is_published_after_identity_teardown() {
    let mut torn_down = launching_tab("pty-verdict", "nonce-verdict");
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(&mut torn_down));
    assert_eq!(torn_down.boot_stage, Some(BootStage::ShellReady));

    let tab_source = include_str!("../tab.rs");
    let clear = tab_source
        .find("is_some_and(clear_agent_identity_for_lifecycle_exit)")
        .expect("the funnel clears identity");
    let publish = tab_source
        .find("publish_launch_exit")
        .expect("the funnel publishes the claimed verdict");
    assert!(
        clear < publish,
        "the verdict must be published after the clear"
    );

    let output = include_str!("../output.rs");
    let teardown = output
        .find("note_provider_exit_in_surviving_shell")
        .expect("the shell exit source tears identity down");
    let published = output
        .find("publish_launch_exit")
        .expect("the shell exit source publishes its verdict");
    assert!(teardown < published, "publish must follow the teardown");
}

/// The exit is evidence about the launch whether or not the launch already
/// claimed `AgentReady`, so nothing here reads `agent_booting`.
#[test]
fn launch_readiness_exit_is_evidence_whether_or_not_agent_ready_was_claimed() {
    for (stage, booting) in [
        (BootStage::LaunchWritten, true),
        (BootStage::ProviderReady, false),
        (BootStage::AgentReady, false),
    ] {
        let mut tab = launching_tab("pty-window", "nonce-window");
        tab.boot_stage = Some(stage);
        tab.agent_booting = booting;
        let attempt = attempt_of(&tab);
        assert!(
            matches!(
                settle_evidence(
                    &tab,
                    &attempt,
                    LaunchEvidence::ProcessExited { exit_code: Some(3) }
                ),
                LaunchSettlement::Failed { .. }
            ),
            "stage {stage:?} must still settle as a launch failure"
        );
    }

    let owner = include_str!("../launch_readiness.rs");
    assert!(!owner.contains("agent_booting"));
}

/// End to end on a real ServiceContext, through the exact byte shape the live
/// shell emits. This is the case the pure tests could not reach: nothing notes
/// the exit by hand, the daemon has to observe it.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn launch_readiness_shell_exit_marker_emits_the_bounded_failure() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab_id, "pty-launch-readiness".into());
    let tab = mux.get_tab_mut(&tab_id).expect("seeded tab");
    tab.agent_type = Some("claude".into());
    tab.session_binding_nonce = Some("nonce-live".into());
    tab.shell_integration_nonce = Some("shell-nonce".into());
    tab.boot_stage = Some(BootStage::AgentReady);

    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some("pty-launch-readiness".into()),
            None,
            None,
            80,
            24,
            Default::default(),
        )
        .expect("spawn live backing PTY");
    arm(LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "pty-launch-readiness".into(),
        binding_nonce: "nonce-live".into(),
    });

    // Empty command line, shell nonce, then command end: what the live shell sends.
    harness
        .service
        .process_pty_output("pty-launch-readiness", b"]633;E;;shell-nonce]633;D;3");

    let mux = harness.service.mux.read();
    let tab = mux
        .get_tab(&tab_id)
        .expect("tab survives its launch failure");
    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    let reason = tab
        .boot_failure_reason
        .as_deref()
        .expect("the failure carries its reason");
    assert!(reason.starts_with(EXITED_BEFORE_READINESS));
    assert!(reason.contains(&tab_id));
    assert!(reason.contains("attempt=pty-launch-readiness"));
    assert!(reason.contains("launch_nonce=nonce-live"));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn unbound_resume_with_live_pty_is_not_spawn_failed_at_deadline() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "resume-live-pty-deadline";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("resume-fixture".into());
        tab.agent_session_id = Some("saved-session".into());
        tab.session_binding_nonce = Some("resume-nonce".into());
        tab.pty_id = Some(pty_id.into());
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness
        .service
        .pty
        .lock()
        .spawn_with_id(Some(pty_id.into()), None, None, 80, 24, Default::default())
        .expect("spawn live backing PTY");
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: pty_id.into(),
        binding_nonce: "resume-nonce".into(),
    };
    arm(attempt.clone());

    let settlement = harness
        .service
        .observe_launch_readiness(attempt, Duration::ZERO, true)
        .recv()
        .expect("deadline produces a waiting settlement");
    assert_eq!(settlement, LaunchSettlement::AwaitingFirstTurn);
    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).expect("restored tab stays present");
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
    drop(mux);
    harness
        .service
        .pty
        .lock()
        .kill(pty_id)
        .expect("stop live backing PTY");
}

/// The launch thread must not claim ready over a verdict already published, so
/// a published failure stays readable until that thread takes it.
#[test]
fn launch_readiness_published_failure_stops_the_launch_thread() {
    let tab = launching_tab("pty-tombstone", "nonce-tombstone");
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    let (claimed, _) = claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3))
        .expect("the armed launch claims its exit");
    let reason = exited_before_readiness_reason(&claimed, Some(3));
    record_published_failure(&claimed, &reason);

    // A second observer of the same exit finds nothing left to claim.
    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3)).is_none());
    assert_eq!(
        observe(&attempt, Instant::now() + BOUND),
        LaunchObservation::Retired
    );

    assert_eq!(
        take_published_failure(&attempt).as_deref(),
        Some(reason.as_str())
    );
    assert_eq!(take_published_failure(&attempt), None);

    let launch = include_str!("../orchestrator/launch.rs");
    let observed = launch
        .find("readiness_receiver.recv()")
        .expect("the launch thread waits for readiness evidence");
    let ready = launch
        .find("startup_readiness::emit_agent_ready")
        .expect("the launch thread delegates ready to the binding gate");
    let gate = include_str!("../orchestrator/launch/startup_readiness.rs");
    assert!(gate.contains("settlement.authorizes_agent_ready(session_authenticated)"));
    assert!(gate.contains("launch.emit(svc, BootStage::AgentReady)"));
    assert!(
        observed < ready,
        "session binding evidence is observed before ready is claimed"
    );
}
