use super::*;
use crate::service::boot_stage::{
    apply_boot_stage, honest_boot_stage, late_readiness_retires_provisional_launch,
    provisional_launch_for,
};
use std::time::Duration;

#[test]
fn launch_waits_for_owner_prompt_without_spawn_failed() {
    owner_prompt_readiness_round_trip("owner-prompt-readiness");
}

#[test]
fn answered_owner_prompt_resumes_normal_readiness() {
    owner_prompt_readiness_round_trip("answered-owner-prompt");
}

fn owner_prompt_readiness_round_trip(prefix: &str) {
    let attempt = LaunchAttempt {
        tab_id: format!("{prefix}-tab"),
        pty_id: format!("{prefix}-pty"),
        binding_nonce: format!("{prefix}-nonce"),
    };
    owner_prompt_pending(&attempt.tab_id, "prompt-token-7");
    arm(attempt.clone());

    let (changed, events) = std::sync::mpsc::channel();
    let observer_attempt = attempt.clone();
    let observer = std::thread::spawn(move || {
        observe_with_owner_prompt(
            &observer_attempt,
            Instant::now() + Duration::from_secs(30),
            OWNER_PROMPT_DEADLINE,
            |prompt| {
                changed
                    .send(prompt.map(|_| BootStage::AwaitingOwnerPrompt))
                    .unwrap()
            },
        )
    });
    assert_eq!(events.recv().unwrap(), Some(BootStage::AwaitingOwnerPrompt));
    owner_prompt_cleared(&attempt.tab_id);
    assert_eq!(events.recv().unwrap(), None);
    assert!(note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce
    ));
    assert_eq!(
        observer.join().unwrap(),
        LaunchObservation::Evidence(LaunchEvidence::Acknowledged)
    );
}

#[test]
fn auto_answered_prompt_does_not_suspend_readiness() {
    let attempt = LaunchAttempt {
        tab_id: "auto-prompt-readiness-tab".into(),
        pty_id: "auto-prompt-readiness-pty".into(),
        binding_nonce: "auto-prompt-readiness-nonce".into(),
    };
    arm(attempt.clone());
    assert_eq!(
        observe_with_owner_prompt(&attempt, Instant::now(), OWNER_PROMPT_DEADLINE, |_| {}),
        LaunchObservation::Deadline
    );
}

#[test]
fn startup_supervision_resumes_when_owner_prompt_clears() {
    let tab_id = "startup-owner-prompt-tab";
    owner_prompt_pending(tab_id, "prompt-token-8");
    let waiter_tab = tab_id.to_string();
    let waiter = std::thread::spawn(move || {
        await_owner_prompt_resolution(&waiter_tab, Duration::from_secs(30))
    });
    owner_prompt_cleared(tab_id);
    assert!(waiter.join().unwrap());
}

#[test]
fn unanswered_owner_prompt_fails_at_owner_deadline_with_named_reason() {
    let attempt = LaunchAttempt {
        tab_id: "unanswered-owner-prompt-tab".into(),
        pty_id: "unanswered-owner-prompt-pty".into(),
        binding_nonce: "unanswered-owner-prompt-nonce".into(),
    };
    arm(attempt.clone());
    owner_prompt_pending(&attempt.tab_id, "prompt-token-7");
    assert_eq!(
        observe_with_owner_prompt(
            &attempt,
            Instant::now() + Duration::from_secs(30),
            Duration::ZERO,
            |_| {},
        ),
        LaunchObservation::OwnerDeadline {
            question: "prompt-token-7".into()
        }
    );
    assert_eq!(
        owner_prompt_deadline_reason("prompt-token-7"),
        "owner prompt was not answered before the 30 minute deadline: prompt-token-7"
    );
    owner_prompt_cleared(&attempt.tab_id);
}

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

fn exit_proof(attempt: &LaunchAttempt) -> LaunchExitProof {
    use crate::service::process_tree::{
        ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeWitnesses,
    };

    let witnesses = ProviderTreeWitnesses::default();
    let key = provider_tree_key(attempt, "claude");
    witnesses.record_for_test(
        &key,
        ProviderProcessTreeSnapshot::Present(
            [ProcessIdentity {
                pid: 900_001,
                start_token: "test-provider-start".into(),
            }]
            .into_iter()
            .collect(),
        ),
    );
    witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let observation = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    LaunchExitProof::from_lifecycle_clear(attempt, &observation)
        .expect("the attempt's tree evidence proves the provider is gone")
}

fn provider_tree_key(
    attempt: &LaunchAttempt,
    executable_name: &str,
) -> crate::service::process_tree::ProviderTreeKey {
    crate::service::process_tree::ProviderTreeKey {
        tab_id: attempt.tab_id.clone(),
        pty_id: attempt.pty_id.clone(),
        binding_nonce: Some(attempt.binding_nonce.clone()),
        executable_name: executable_name.into(),
    }
}

#[test]
fn provider_tree_witnesses_reject_live_restored_provider() {
    use crate::service::process_tree::{
        ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeWitnesses,
    };

    let attempt = LaunchAttempt {
        tab_id: "restored-opencode-tab".into(),
        pty_id: "restored-opencode-pty".into(),
        binding_nonce: "restored-opencode-nonce".into(),
    };
    let key = provider_tree_key(&attempt, "opencode");
    let witnesses = ProviderTreeWitnesses::default();
    for _prompt_frame in 0..2 {
        let prompt = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
        assert!(!prompt.proves_exit_from_lifecycle_clear());
        assert!(!prompt.proves_exit_from_finished_marker());
    }

    let live_child = witnesses.record_for_test(
        &key,
        ProviderProcessTreeSnapshot::Present(
            [ProcessIdentity {
                pid: 52_520,
                start_token: "opencode-start".into(),
            }]
            .into_iter()
            .collect(),
        ),
    );
    assert_eq!(
        live_child.state(),
        crate::service::process_tree::ProviderTreeState::Present
    );
    assert!(
        LaunchExitProof::from_finished_marker(&attempt, &live_child, Some(3), false, None)
            .is_none()
    );
    assert!(LaunchExitProof::from_lifecycle_clear(&attempt, &live_child).is_none());
}

#[test]
fn provider_absent_before_first_seen_is_not_exit_evidence() {
    use crate::service::process_tree::{
        ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeState, ProviderTreeWitnesses,
    };

    let attempt = LaunchAttempt {
        tab_id: "starting-provider-tab".into(),
        pty_id: "starting-provider-pty".into(),
        binding_nonce: "starting-provider-nonce".into(),
    };
    let key = provider_tree_key(&attempt, "opencode");
    let witnesses = ProviderTreeWitnesses::default();
    for _ in 0..3 {
        let absent = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
        assert_eq!(absent.state(), ProviderTreeState::AbsentBeforeSeen);
        assert!(LaunchExitProof::from_lifecycle_clear(&attempt, &absent).is_none());
        assert!(
            LaunchExitProof::from_finished_marker(&attempt, &absent, None, false, None).is_none()
        );
    }
    let absent = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let explicit_failure =
        LaunchExitProof::from_finished_marker(&attempt, &absent, Some(3), true, None);
    assert!(explicit_failure.is_some());
    assert!(
        LaunchExitProof::from_finished_marker(&attempt, &absent, Some(0), false, None).is_none()
    );
    assert!(
        LaunchExitProof::from_finished_marker(&attempt, &absent, Some(0), true, None).is_some()
    );
    assert!(
        LaunchExitProof::from_finished_marker(&attempt, &absent, None, false, Some(false))
            .is_some()
    );
    assert!(
        LaunchExitProof::from_finished_marker(&attempt, &absent, None, false, Some(true)).is_none()
    );

    witnesses.record_for_test(
        &key,
        ProviderProcessTreeSnapshot::Present(
            [ProcessIdentity {
                pid: 70_001,
                start_token: "starting-opencode".into(),
            }]
            .into_iter()
            .collect(),
        ),
    );
    let one_absence = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    assert_eq!(one_absence.state(), ProviderTreeState::GoneAfterSeen);
    assert!(LaunchExitProof::from_lifecycle_clear(&attempt, &one_absence).is_some());
    assert!(
        LaunchExitProof::from_finished_marker(&attempt, &one_absence, None, false, None).is_some()
    );
}

#[test]
fn provider_exit_evidence_ends_first_turn_waiting() {
    let mut tab = launching_tab("pty-resume-exit", "nonce-resume-exit");
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_booting = false;
    let attempt = attempt_of(&tab);

    assert!(launch_attempt_is_waiting_for_first_turn(&tab, &attempt));

    tab.exit_suspected_at = Some(std::time::Instant::now());
    assert!(!launch_attempt_is_waiting_for_first_turn(&tab, &attempt));
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
    let proof = exit_proof(&attempt);

    arm(attempt.clone());
    assert!(
        claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3), &proof).is_some(),
        "the first observer of an exit claims it"
    );
    assert!(
        claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3), &proof).is_none(),
        "a second observer must not claim the same exit before it is published"
    );
    assert!(
        claim_exit(&attempt.tab_id, None, None, &proof).is_none(),
        "nor may the funnel that reports no pty claim it again"
    );
}

#[test]
fn launch_readiness_local_exit_emits_bounded_failure() {
    let tab = launching_tab("pty-local", "nonce-local");
    let attempt = attempt_of(&tab);
    let proof = exit_proof(&attempt);

    arm(attempt.clone());
    let (claimed, exit_code, _, _) =
        claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3), &proof)
            .expect("an armed launch claims the exit observed against it");
    assert_eq!(claimed, attempt);
    assert_eq!(exit_code, Some(3));
    // A claim leaves no evidence behind it, so the waiting observer finds none.
    assert_eq!(
        observe(&attempt, Instant::now()),
        LaunchObservation::Deadline
    );

    let settlement = settle_proven_exit(&tab, &attempt, Some(3), &proof);
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
    let proof = exit_proof(&attempt);

    arm(attempt.clone());
    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(1), &proof).is_some());

    let mut local = tab.clone();
    local.report_target_mesh_peer = None;
    local.spawned_by_tab_id = None;
    let relayed = settle_proven_exit(&tab, &attempt, Some(1), &proof);
    let direct = settle_proven_exit(&local, &attempt, Some(1), &proof);
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
    let proof = exit_proof(&attempt);
    assert!(note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce
    ));
    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(0), &proof).is_none());
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

    let latch = provisional_launch_for(
        &tab,
        &attempt.pty_id,
        &attempt.binding_nonce,
        SPAWN_READINESS_DEADLINE,
    )
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
        settle_proven_exit(&relinked, &attempt, Some(3), &exit_proof(&attempt)),
        LaunchSettlement::Superseded,
    );

    let mut rotated = tab.clone();
    rotated.session_binding_nonce = Some("nonce-next".into());
    assert_eq!(
        settle_evidence(&rotated, &attempt, LaunchEvidence::Acknowledged),
        LaunchSettlement::Superseded,
    );

    arm(attempt.clone());
    let proof = exit_proof(&attempt);
    assert!(claim_exit(&attempt.tab_id, Some("pty-foreign"), Some(3), &proof).is_none());
    assert!(claim_exit("tab-foreign", None, Some(3), &proof).is_none());
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

    assert!(launch.contains("launch_readiness::arm_with_origin("));
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
    let proof = exit_proof(&attempt);

    arm(attempt.clone());
    let (claimed, exit_code, _, _) =
        claim_exit(&attempt.tab_id, Some(&attempt.pty_id), None, &proof)
            .expect("an exit with no shell-reported detail still claims the launch");
    assert_eq!(claimed, attempt);
    assert_eq!(exit_code, None);
    assert!(matches!(
        settle_proven_exit(&tab, &attempt, None, &proof),
        LaunchSettlement::Failed { .. }
    ));

    let owner = include_str!("../launch_readiness.rs");
    assert!(!owner.contains("command_line"));
}

#[test]
fn pre_ready_exit_in_restore_auto_resumes_once() {
    let tab = launching_tab("pty-auto-restore", "nonce-auto-restore");
    let attempt = attempt_of(&tab);
    arm_with_origin(
        attempt.clone(),
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    );

    let (_, _, _, origin) = claim_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        None,
        &exit_proof(&attempt),
    )
    .expect("the proven early exit claims its armed launch");
    assert!(matches!(
        origin,
        Some(crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot)
    ));
    let starts = std::cell::Cell::new(0);
    route_pre_ready_exit(
        origin,
        LaunchExitSource::LifecycleClear,
        || starts.set(starts.get() + 1),
        || panic!("restore must resume rather than notify"),
    );
    assert!(claim_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        None,
        &exit_proof(&attempt),
    )
    .is_none());
    assert_eq!(starts.get(), 1);
}

#[test]
fn provider_composer_exit_before_launch_ack_is_still_pre_ready() {
    let mut tab = launching_tab("pty-composer-before-ack", "nonce-composer-before-ack");
    tab.boot_stage = Some(BootStage::ProviderReady);
    tab.agent_booting = false;

    assert!(!surviving_shell_exit_is_after_launch_ack(&tab));

    report_own_launch(&mut tab);
    assert!(surviving_shell_exit_is_after_launch_ack(&tab));
}

#[test]
fn recover_pre_ready_exit_routes_only_restore_resume_and_exit_recovery() {
    use crate::service::launch_diagnostics::AgentLaunchOrigin as Origin;

    let recovery_count = std::cell::Cell::new(0);
    let notification_count = std::cell::Cell::new(0);
    for origin in [
        Origin::RestoredSnapshot,
        Origin::Resume,
        Origin::ExitRecovery,
    ] {
        route_pre_ready_exit(
            Some(origin),
            LaunchExitSource::LifecycleClear,
            || recovery_count.set(recovery_count.get() + 1),
            || notification_count.set(notification_count.get() + 1),
        );
    }
    assert_eq!(recovery_count.get(), 3);
    assert_eq!(notification_count.get(), 0);

    route_pre_ready_exit(
        Some(Origin::VisibleSpawn),
        LaunchExitSource::LifecycleClear,
        || recovery_count.set(recovery_count.get() + 1),
        || notification_count.set(notification_count.get() + 1),
    );
    route_pre_ready_exit(
        Some(Origin::RestoredSnapshot),
        LaunchExitSource::ConfirmedPtyExit,
        || recovery_count.set(recovery_count.get() + 1),
        || notification_count.set(notification_count.get() + 1),
    );
    assert_eq!(recovery_count.get(), 3);
    assert_eq!(notification_count.get(), 0);

    route_pre_ready_exit(
        Some(Origin::AutomaticExitRecovery),
        LaunchExitSource::LifecycleClear,
        || recovery_count.set(recovery_count.get() + 1),
        || notification_count.set(notification_count.get() + 1),
    );
    assert_eq!(recovery_count.get(), 3);
    assert_eq!(notification_count.get(), 1);
}

#[test]
fn recovery_failure_notifies_unless_the_exit_path_already_notifies() {
    assert!(recovery_failure_needs_notification(Some(
        "ShellReadyTimeout"
    )));
    assert!(recovery_failure_needs_notification(None));
    let attempt = LaunchAttempt {
        tab_id: "tab-exit-already-notified".into(),
        pty_id: "pty-exit-already-notified".into(),
        binding_nonce: "nonce-exit-already-notified".into(),
    };
    let reason = exited_before_readiness_reason(&attempt, Some(3));
    assert!(!recovery_failure_needs_notification(Some(&reason)));
}

#[test]
fn pre_ready_exit_in_resume_auto_resumes_once() {
    let tab = launching_tab("pty-auto-resume", "nonce-auto-resume");
    let attempt = attempt_of(&tab);
    arm_with_origin(
        attempt.clone(),
        crate::service::launch_diagnostics::AgentLaunchOrigin::Resume,
    );

    let (_, _, _, origin) = claim_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        None,
        &exit_proof(&attempt),
    )
    .expect("the proven early exit claims its resumed launch");
    assert_eq!(
        origin,
        Some(crate::service::launch_diagnostics::AgentLaunchOrigin::Resume)
    );
    let starts = std::cell::Cell::new(0);
    route_pre_ready_exit(
        origin,
        LaunchExitSource::LifecycleClear,
        || starts.set(starts.get() + 1),
        || panic!("resume must resume rather than notify"),
    );
    assert_eq!(starts.get(), 1);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn second_pre_ready_exit_notifies_and_stops() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("seeded tab");
    tab.pty_id = Some("pty-auto-retry".into());
    tab.boot_stage = Some(BootStage::ShellReady);
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "pty-auto-retry".into(),
        binding_nonce: "nonce-auto-retry".into(),
    };
    arm_with_origin(
        attempt.clone(),
        crate::service::launch_diagnostics::AgentLaunchOrigin::AutomaticExitRecovery,
    );
    let (_, _, proof, action) = claim_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        None,
        &exit_proof(&attempt),
    )
    .expect("the automatic resume exit claims its armed launch");
    assert!(matches!(
        action,
        Some(crate::service::launch_diagnostics::AgentLaunchOrigin::AutomaticExitRecovery)
    ));
    assert!(claim_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        None,
        &exit_proof(&attempt),
    )
    .is_none());

    let failure = LaunchExitFailure {
        attempt: attempt.clone(),
        origin: action,
        auto_recovery_claimed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        exit_code: None,
        reason: exited_before_readiness_reason(&attempt, None),
        source: LaunchExitSource::LifecycleClear,
        tree_evidence: proof.tree_evidence().to_string(),
    };
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.recover_pre_ready_exit(&failure);
    fixture.service.recover_pre_ready_exit(&failure);

    {
        let notification_store = fixture.service.notif.lock();
        let notifications = notification_store.list(false);
        assert_eq!(notifications.len(), 1);
        assert!(notifications[0]
            .body
            .as_deref()
            .is_some_and(|body| body.contains(&recovery_command(&tab_id))));
    }
    let mux = fixture.service.mux.read();
    let shell = mux.get_tab(&tab_id).expect("the surviving shell remains");
    assert_eq!(shell.pty_id.as_deref(), Some("pty-auto-retry"));
    assert_eq!(shell.boot_stage, Some(BootStage::ShellReady));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn failed_automatic_recovery_notifies_with_restart_command() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    fixture.service.notify_recovery_failure_if_needed(
        &tab_id,
        "recovery-launch",
        Some("ShellReadyTimeout"),
    );

    let notif_guard = fixture.service.notif.lock();

    let notifications = notif_guard.list(false);
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0]
        .body
        .as_deref()
        .is_some_and(|body| body.contains(&recovery_command(&tab_id))));
}

#[test]
fn exit_after_readiness_is_never_auto_restarted() {
    let tab = launching_tab("pty-ready-exit", "nonce-ready-exit");
    let attempt = attempt_of(&tab);
    let proof = exit_proof(&attempt);
    arm_with_origin(
        attempt.clone(),
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    );
    assert!(note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce,
    ));

    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), None, &proof).is_none());
}

/// The second half of the same live failure: every observer that sees an exit
/// also tears the tab's agent identity down, and the teardown clears the launch
/// nonce. A settlement attempted afterwards can only be Superseded, so the
/// settle must come first at every call site.
#[test]
fn launch_readiness_exit_settles_before_identity_teardown() {
    let tab = launching_tab("pty-teardown", "nonce-teardown");
    let attempt = attempt_of(&tab);
    let proof = exit_proof(&attempt);

    assert!(matches!(
        settle_proven_exit(&tab, &attempt, Some(3), &proof),
        LaunchSettlement::Failed { .. }
    ));

    let mut torn_down = tab.clone();
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(&mut torn_down));
    assert!(torn_down.session_binding_nonce.is_none());
    assert_eq!(
        settle_proven_exit(&torn_down, &attempt, Some(3), &proof),
        LaunchSettlement::Superseded,
    );

    // Claim, tear down, then publish: the verdict has to be decided while the
    // tab still carries the launch, and recorded after the teardown resets it.
    for (source, teardown_anchor) in [
        (
            include_str!("../output/shell_markers.rs"),
            "clear_tab_agent_identity_after_exit_with_proof",
        ),
        (
            include_str!("../tab/lifecycle_exit.rs"),
            "let finished = self.finish_agent_identity_clear",
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
        let handoff = source
            .find("handoff_launch_exit_after_teardown")
            .expect("recovery starts at the explicit post-teardown handoff");
        assert!(claim < teardown, "the claim must precede the teardown");
        assert!(teardown < publish, "the verdict must be published after it");
        assert!(
            publish < handoff,
            "recovery must follow verdict publication"
        );
    }
}

/// A launch verdict is published after the teardown, so it is the stage that
/// stands even though the teardown resets the tab to `shell_ready`.
#[test]
fn launch_readiness_failed_launch_is_published_after_identity_teardown() {
    let mut torn_down = launching_tab("pty-verdict", "nonce-verdict");
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(&mut torn_down));
    assert_eq!(torn_down.boot_stage, Some(BootStage::ShellReady));

    let tab_source = include_str!("../tab/lifecycle_exit.rs");
    let clear = tab_source
        .find("let finished = self.finish_agent_identity_clear")
        .expect("the funnel clears identity");
    let publish = tab_source
        .find("publish_launch_exit")
        .expect("the funnel publishes the claimed verdict");
    assert!(
        clear < publish,
        "the verdict must be published after the clear"
    );
    let handoff = tab_source
        .find("handoff_launch_exit_after_teardown")
        .expect("recovery starts at the explicit post-teardown handoff");
    assert!(publish < handoff, "recovery follows teardown publication");

    let output = include_str!("../output/shell_markers.rs");
    let teardown = output
        .find("clear_tab_agent_identity_after_exit_with_proof")
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
                settle_proven_exit(&tab, &attempt, Some(3), &exit_proof(&attempt)),
                LaunchSettlement::Failed { .. }
            ),
            "stage {stage:?} must still settle as a launch failure"
        );
    }

    let owner = include_str!("../launch_readiness.rs");
    let exit_settlement = owner
        .split("fn settle_proven_exit(")
        .nth(1)
        .and_then(|remaining| remaining.split("\nstruct Armed").next())
        .expect("the exit settlement function remains present");
    assert!(!exit_settlement.contains("agent_booting"));
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
        .agent_registry
        .register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
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
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "pty-launch-readiness".into(),
        binding_nonce: "nonce-live".into(),
    };
    let tree_key = harness
        .service
        .provider_tree_key_for_tab(&tab_id, &attempt.pty_id)
        .expect("the tab provider exposes its executable capability");
    harness.service.provider_tree_witnesses.record_for_test(
        &tree_key,
        crate::service::process_tree::ProviderProcessTreeSnapshot::Present(
            [crate::service::process_tree::ProcessIdentity {
                pid: 900_001,
                start_token: "synthetic-provider-start".into(),
            }]
            .into_iter()
            .collect(),
        ),
    );
    arm(attempt);

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
    let proof = exit_proof(&attempt);

    arm(attempt.clone());
    let (claimed, _, _, _) = claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3), &proof)
        .expect("the armed launch claims its exit");
    let reason = exited_before_readiness_reason(&claimed, Some(3));
    record_published_failure(&claimed, &reason);

    // A second observer of the same exit finds nothing left to claim.
    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), Some(3), &proof).is_none());
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

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn post_readiness_shell_exit_notifies_with_restart_command() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab_id, "pty-ready-exit-notification".into());
    {
        let tab = mux.get_tab_mut(&tab_id).expect("seeded tab");
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some("session-ready-exit".into());
        tab.session_binding_nonce = Some("nonce-ready-exit".into());
        tab.reported_session_binding_nonce = Some("nonce-ready-exit".into());
        tab.shell_integration_nonce = Some("shell-nonce".into());
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness
        .service
        .agent_registry
        .register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    harness
        .service
        .pty
        .lock()
        .spawn_with_id(
            Some("pty-ready-exit-notification".into()),
            None,
            None,
            80,
            24,
            Default::default(),
        )
        .expect("spawn live backing PTY");
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "pty-ready-exit-notification".into(),
        binding_nonce: "nonce-ready-exit".into(),
    };
    let tree_key = harness
        .service
        .provider_tree_key_for_tab(&tab_id, &attempt.pty_id)
        .expect("the tab provider exposes its executable capability");
    harness.service.provider_tree_witnesses.record_for_test(
        &tree_key,
        crate::service::process_tree::ProviderProcessTreeSnapshot::Present(
            [crate::service::process_tree::ProcessIdentity {
                pid: 900_002,
                start_token: "synthetic-ready-provider-start".into(),
            }]
            .into_iter()
            .collect(),
        ),
    );

    harness.service.process_pty_output(
        "pty-ready-exit-notification",
        b"\x1b]633;E;;shell-nonce\x07\x1b]633;D;3\x07",
    );

    let notif_guard = harness.service.notif.lock();

    let notifications = notif_guard.list(false);
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0]
        .body
        .as_deref()
        .is_some_and(|body| body.contains(&recovery_command(&tab_id))));
    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).expect("surviving shell remains");
    assert_eq!(tab.pty_id.as_deref(), Some(attempt.pty_id.as_str()));
    assert!(tab.agent_type.is_none());
}
