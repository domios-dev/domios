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

/// The agent naming itself on this launch: its own accepted session plus this
/// attempt's nonce echoed back.
fn report_own_launch(tab: &mut Tab) {
    tab.agent_session_id = Some("01a090f2".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
}

/// A generous bound proves the wait is released by evidence, not by elapsed
/// time; nothing in these tests sleeps to reach its outcome.
const BOUND: Duration = Duration::from_secs(30);

/// What the shell reports as the finished command line for this launch: the
/// prelude `command_with_binding_nonce_for_shell` stamps carries the nonce.
fn launch_command_line(attempt: &LaunchAttempt) -> String {
    format!(
        "export CODETERM_SESSION_BINDING_NONCE='{}'; claude --dangerously-skip-permissions",
        attempt.binding_nonce
    )
}

#[test]
fn launch_readiness_local_exit_emits_bounded_failure() {
    let tab = launching_tab("pty-local", "nonce-local");
    let attempt = attempt_of(&tab);

    arm(attempt.clone());
    assert!(note_launch_command_exit(
        &attempt.tab_id,
        &attempt.pty_id,
        Some(launch_command_line(&attempt).as_str()),
        Some(3),
    ));
    assert_eq!(
        observe(&attempt, Instant::now() + BOUND),
        LaunchObservation::Evidence(LaunchEvidence::ProcessExited { exit_code: Some(3) }),
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
    assert!(note_launch_command_exit(
        &attempt.tab_id,
        &attempt.pty_id,
        Some(launch_command_line(&attempt).as_str()),
        Some(1),
    ));
    assert_eq!(
        observe(&attempt, Instant::now() + BOUND),
        LaunchObservation::Evidence(LaunchEvidence::ProcessExited { exit_code: Some(1) }),
    );

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
    assert!(!note_launch_command_exit(
        &attempt.tab_id,
        &attempt.pty_id,
        Some(launch_command_line(&attempt).as_str()),
        Some(0),
    ));
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
    assert!(!note_launch_command_exit(
        &attempt.tab_id,
        "pty-foreign",
        Some(launch_command_line(&attempt).as_str()),
        Some(3),
    ));
    assert!(!note_launch_command_exit(
        &attempt.tab_id,
        &attempt.pty_id,
        Some("git status"),
        Some(1),
    ));
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

    assert!(launch.contains("launch_readiness::arm("));
    assert!(launch.contains("observe_launch_readiness"));
    assert!(launch.contains("settle_observed_launch_exit"));

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
