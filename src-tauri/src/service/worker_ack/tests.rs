use super::*;
use crate::service::message_gateway::{DeliverySeam, KeyStroke, MessageGateway};
use crate::service::messenger::{Delivery, MessageEnvelope};
use crate::service::pty_io::SendBatchItem;
use std::cell::RefCell;
use std::sync::Arc;

fn ack_identity(attempt_id: &str) -> WorkerAckIdentity {
    WorkerAckIdentity {
        plan_id: "plan".into(),
        task_id: "task".into(),
        attempt_id: attempt_id.into(),
        tab_id: "tab".into(),
        session_id: Some("session".into()),
        binding_nonce: Some("nonce".into()),
    }
}

fn startup_receiver(
    registry: &WorkerAckRegistry,
    authority: StartupAuthority,
) -> Receiver<AgentStartupSignal> {
    match registry.arm_startup(authority) {
        AgentStartupArm::Armed(receiver) => receiver,
        _ => panic!("expected a new startup waiter"),
    }
}

#[derive(Default)]
struct ReminderDeliverySeam {
    targets: RefCell<Vec<String>>,
}

impl DeliverySeam for ReminderDeliverySeam {
    fn deliver_enveloped(
        &self,
        to_tab: &str,
        _envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        self.targets.borrow_mut().push(to_tab.to_string());
        Ok(Delivery::Pty {
            text: body.to_string(),
        })
    }

    fn deliver_text(&self, _tab: &str, _text: &str) -> Result<(), String> {
        panic!("reminder must use enveloped delivery")
    }

    fn deliver_batch(&self, _tab: &str, _batch: Vec<SendBatchItem>) -> Result<(), String> {
        panic!("reminder must use enveloped delivery")
    }

    fn deliver_keys(&self, _tab: &str, _text: &str, _strokes: &[KeyStroke]) -> Result<(), String> {
        panic!("reminder must use enveloped delivery")
    }
}

#[test]
fn worker_ack_reminder_uses_the_gateway_delivery_target() {
    let gateway = MessageGateway::new();
    let seam = ReminderDeliverySeam::default();

    gateway
        .submit(&seam, worker_ack_reminder_message("agent-tab", "attempt-a"))
        .unwrap();

    let targets = seam.targets.borrow();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0], "agent-tab");
}

#[test]
fn agent_ack_reminder_keeps_its_own_context_constant() {
    let message = worker_ack_reminder_message("agent-tab", "attempt-a");
    assert_eq!(
        message.envelope.and_then(|envelope| envelope.context),
        Some(WORKER_ACK_CONTEXT.to_string())
    );
}

#[test]
fn acknowledgement_resolves_only_the_matching_attempt() {
    let registry = WorkerAckRegistry::default();
    let identity = ack_identity("attempt-a");
    let receiver = registry.arm(identity.clone()).receiver.unwrap();

    assert!(!registry.acknowledge(&ack_identity("attempt-b")));
    assert!(registry.acknowledge(&identity));
    assert_eq!(receiver.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn different_attempt_supersedes_the_previous_deadline() {
    let registry = WorkerAckRegistry::default();
    let previous_identity = ack_identity("attempt-a");
    let previous = registry.arm(previous_identity.clone()).receiver.unwrap();
    let current_identity = ack_identity("attempt-b");
    let current_arm = registry.arm(current_identity.clone());
    let current = current_arm.receiver.unwrap();

    assert!(current_arm.starts_supervisor);
    assert!(!current_arm.identity_conflict);
    assert_eq!(previous.recv().unwrap(), WorkerAckSignal::Superseded);
    assert!(!registry.acknowledge(&previous_identity));
    assert!(registry.acknowledge(&current_identity));
    assert_eq!(current.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn double_arm_for_the_same_identity_keeps_one_supervisor_and_accepts_ack() {
    let registry = WorkerAckRegistry::default();
    let identity = ack_identity("attempt-a");
    let first = registry.arm(identity.clone());
    let starts_supervisor = first.starts_supervisor;
    let receiver = first.receiver.unwrap();
    let second = registry.arm(identity.clone());

    assert!(starts_supervisor);
    assert!(!second.starts_supervisor);
    assert!(second.receiver.is_none());
    assert!(!second.identity_conflict);
    assert!(registry.acknowledge(&identity));
    assert_eq!(receiver.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn reminder_delivery_and_same_attempt_arm_preserve_the_ack_waiter() {
    let registry = WorkerAckRegistry::default();
    let gateway = MessageGateway::new();
    let seam = ReminderDeliverySeam::default();
    let identity = ack_identity("attempt-a");
    let receiver = registry.arm(identity.clone()).receiver.unwrap();

    gateway
        .submit(&seam, worker_ack_reminder_message("tab", "attempt-a"))
        .unwrap();
    let rearm = registry.arm(identity.clone());

    assert_eq!(seam.targets.borrow().as_slice(), &[String::from("tab")]);
    assert!(!rearm.starts_supervisor);
    assert!(registry.acknowledge(&identity));
    assert_eq!(receiver.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn startup_hold_and_same_attempt_arm_keep_the_ack_waiter() {
    let registry = WorkerAckRegistry::default();
    let identity = ack_identity("attempt-a");
    let receiver = registry.arm(identity.clone()).receiver.unwrap();
    let startup_events = startup_receiver(&registry, creator_authority("tab", "launch-a"));

    assert!(registry
        .hold_startup_authorized(
            "creator",
            "tab",
            "launch-a",
            Some(&live_child("launch-a", None))
        )
        .is_ok());
    assert_eq!(startup_events.recv().unwrap(), AgentStartupSignal::Held);
    assert!(registry.extend_deadline(&identity));
    let rearm = registry.arm(identity.clone());
    assert!(!rearm.starts_supervisor);
    assert!(registry.acknowledge(&identity));
    assert_eq!(
        await_worker_ack(&receiver, Duration::from_secs(1)),
        WorkerAckWaitOutcome::Resolved
    );
}

#[test]
fn startup_hold_before_arm_is_applied_to_the_same_ack_identity() {
    let registry = WorkerAckRegistry::default();
    let identity = ack_identity("attempt-a");

    assert!(registry.extend_deadline(&identity));
    let arm = registry.arm(identity.clone());
    let starts_supervisor = arm.starts_supervisor;
    let receiver = arm.receiver.unwrap();

    assert!(starts_supervisor);
    assert!(registry.acknowledge(&identity));
    assert_eq!(
        await_worker_ack(&receiver, Duration::from_secs(1)),
        WorkerAckWaitOutcome::Resolved
    );
}

#[test]
fn startup_hold_survives_session_binding_rearm_for_the_same_launch_nonce() {
    let registry = WorkerAckRegistry::default();
    let old_identity = WorkerAckIdentity {
        session_id: None,
        ..ack_identity("attempt-a")
    };
    let old_receiver = registry.arm(old_identity.clone()).receiver.unwrap();
    let new_identity = WorkerAckIdentity {
        session_id: Some("session-bound".into()),
        ..old_identity.clone()
    };

    assert!(registry.extend_deadline(&new_identity));
    let new_receiver = registry.arm(new_identity.clone()).receiver.unwrap();

    assert_eq!(old_receiver.recv().unwrap(), WorkerAckSignal::StartupHeld);
    assert_eq!(old_receiver.recv().unwrap(), WorkerAckSignal::Superseded);
    assert!(registry.acknowledge(&new_identity));
    assert_eq!(
        await_worker_ack(&new_receiver, Duration::from_secs(1)),
        WorkerAckWaitOutcome::Resolved
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn bound_worker_ack_deadline_accepts_a_borrowed_service_context() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service: &ServiceContext = &fixture.service;

    assert!(!service
        .arm_bound_worker_ack_deadline_for_tab(&tab_id, "dispatch".into())
        .unwrap());
}

#[test]
fn a_new_binding_supersedes_the_old_identity_and_rejects_its_ack() {
    let registry = WorkerAckRegistry::default();
    let old_identity = ack_identity("attempt-a");
    let old_receiver = registry.arm(old_identity.clone()).receiver.unwrap();
    let new_identity = WorkerAckIdentity {
        binding_nonce: Some("nonce-new".into()),
        ..old_identity.clone()
    };
    let new_receiver = registry.arm(new_identity.clone()).receiver.unwrap();

    assert_eq!(old_receiver.recv().unwrap(), WorkerAckSignal::Superseded);
    assert!(!registry.acknowledge(&old_identity));
    assert!(registry.acknowledge(&new_identity));
    assert_eq!(new_receiver.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn a_subject_mismatch_for_the_same_attempt_and_binding_keeps_the_existing_waiter() {
    let registry = WorkerAckRegistry::default();
    let identity = ack_identity("attempt-a");
    let receiver = registry.arm(identity.clone()).receiver.unwrap();
    let conflicting = WorkerAckIdentity {
        task_id: "other-task".into(),
        ..identity.clone()
    };

    let conflict = registry.arm(conflicting);
    assert!(conflict.identity_conflict);
    assert!(!conflict.starts_supervisor);
    assert!(registry.acknowledge(&identity));
    assert_eq!(receiver.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn expiry_removes_only_the_matching_deadline() {
    let registry = WorkerAckRegistry::default();
    let identity = ack_identity("attempt-a");
    let receiver = registry.arm(identity.clone()).receiver.unwrap();

    assert!(!registry.expire(&ack_identity("attempt-b")));
    assert!(registry.expire(&identity));
    assert!(receiver.recv().is_err());
}

#[test]
fn startup_deadlines_exceed_measured_provider_cold_starts() {
    assert_eq!(WORKER_ACK_TIMEOUT.as_secs(), COLD_START_DEADLINE_SECS);
    assert_eq!(WORKER_ACK_FINAL_TIMEOUT.as_secs(), COLD_START_DEADLINE_SECS);
    assert_eq!(
        AGENT_STARTUP_NOTICE_TIMEOUT.as_secs(),
        COLD_START_DEADLINE_SECS
    );
    assert_eq!(
        AGENT_STARTUP_FINAL_TIMEOUT.as_secs(),
        COLD_START_DEADLINE_SECS
    );
    assert!(AGENT_STARTUP_NOTICE_TIMEOUT > Duration::from_secs(MEASURED_CLAUDE_COLD_START_SECS));
    assert!(AGENT_STARTUP_NOTICE_TIMEOUT > Duration::from_secs(MEASURED_CODEX_COLD_START_SECS));
}

#[test]
fn worker_ack_final_deadline_expires_once() {
    let registry = WorkerAckRegistry::default();
    let identity = ack_identity("attempt");
    let receiver = registry.arm(identity.clone()).receiver.unwrap();

    assert!(registry.expire(&identity));
    assert!(!registry.expire(&identity));
    assert!(receiver.recv().is_err());
}

#[test]
fn worker_ack_timeout_reports_a_surviving_finalized_attempt() {
    let event = worker_ack_timeout_event(
        "plan",
        "task",
        "tab",
        "attempt",
        "codeterm plan task dispatch plan task",
    );

    assert_eq!(event["kind"], "worker_ack_startup_failed");
    assert_eq!(event["tab_state"], TAB_STATE_ALIVE_AWAITING_INSPECTION);
    assert_eq!(event["attempt_state"], ATTEMPT_STATE_FINALIZED);
    assert_eq!(
        event["recovery"]["capability"],
        crate::prompts::INTERACTIVE_CHILD_PTY_CAPABILITY_ID
    );
    assert_eq!(event["recovery"]["target_tab_id"], "tab");
    assert_eq!(event["recovery"]["attempt_id"], "attempt");
    let primitives = event["recovery"]["primitives"].as_array().unwrap();
    for primitive in [
        "codeterm tab inspect --tab tab",
        "codeterm tab status --tab tab",
        "codeterm tab get-text --tab tab",
        "codeterm tab respond OPTION --tab tab",
    ] {
        assert!(primitives
            .iter()
            .any(|value| value.as_str() == Some(primitive)));
    }
    assert!(primitives.iter().any(|value| value
        .as_str()
        .is_some_and(|value| value.starts_with("codeterm send --tab tab --actions "))));
    assert!(event.get("cleanup").is_none());
    assert!(event.get("cleanup_error").is_none());
    assert_eq!(
        event["deadline_seconds"],
        (WORKER_ACK_TIMEOUT + WORKER_ACK_FINAL_TIMEOUT).as_secs()
    );
    assert_eq!(
        event["deadline_basis"]["measurement_method"],
        COLD_START_MEASUREMENT_METHOD
    );
    assert_eq!(
        event["deadline_basis"]["claude_cold_start_seconds"],
        MEASURED_CLAUDE_COLD_START_SECS
    );
    assert_eq!(
        event["deadline_basis"]["codex_cold_start_seconds"],
        MEASURED_CODEX_COLD_START_SECS
    );
    assert_eq!(
        event["deadline_basis"]["selected_phase_seconds"],
        COLD_START_DEADLINE_SECS
    );
}

#[test]
fn worker_ack_timeout_keeps_spawn_prose_as_opaque_json_data() {
    let spawn_command = "codeterm agent spawn --plan-task task; Write-Output 'opaque text'";
    let event = worker_ack_timeout_event("plan", "task", "tab", "attempt", spawn_command);

    assert_eq!(event["spawn_command"], spawn_command);
    assert!(event["spawn_command"].is_string());
    assert!(serde_json::to_string(&event)
        .unwrap()
        .contains("opaque text"));
}

fn creator_authority(child_tab_id: &str, attempt_id: &str) -> StartupAuthority {
    StartupAuthority::new("creator", child_tab_id, attempt_id)
}

fn live_child(attempt_id: &str, team_id: Option<&str>) -> ChildStartupState {
    ChildStartupState {
        attempt_id: Some(attempt_id.to_string()),
        team_id: team_id.map(str::to_string),
        session_id: None,
        binding_nonce: None,
    }
}

#[test]
fn startup_acknowledgement_and_hold_resolve_only_the_current_attempt() {
    let registry = WorkerAckRegistry::default();
    let ready = startup_receiver(&registry, creator_authority("tab-a", "launch-a"));
    assert!(!registry.mark_startup_ready("tab-a", "launch-old"));
    assert!(registry.mark_startup_ready("tab-a", "launch-a"));
    assert_eq!(ready.recv().unwrap(), AgentStartupSignal::Ready);

    let held = startup_receiver(&registry, creator_authority("tab-a", "launch-b"));
    assert!(registry
        .hold_startup_authorized(
            "creator",
            "tab-a",
            "launch-old",
            Some(&live_child("launch-old", None))
        )
        .is_err());
    assert!(registry
        .hold_startup_authorized(
            "creator",
            "tab-a",
            "launch-b",
            Some(&live_child("launch-b", None))
        )
        .is_ok());
    assert_eq!(held.recv().unwrap(), AgentStartupSignal::Held);

    let exited = startup_receiver(&registry, creator_authority("tab-a", "launch-c"));
    assert!(!registry.mark_startup_launch_command_finished("tab-a", "launch-old", Some(9)));
    assert!(registry.mark_startup_launch_command_finished("tab-a", "launch-c", Some(9)));
    assert_eq!(
        exited.recv().unwrap(),
        AgentStartupSignal::LaunchCommandFinished(Some(9))
    );
    assert!(registry
        .hold_startup_authorized(
            "creator",
            "tab-a",
            "launch-c",
            Some(&live_child("launch-c", None))
        )
        .is_ok());
    assert_eq!(exited.recv().unwrap(), AgentStartupSignal::Held);
}

#[test]
fn repeated_startup_ack_is_an_explicit_no_op_for_the_same_attempt() {
    let registry = WorkerAckRegistry::default();
    let ready = startup_receiver(&registry, creator_authority("tab-a", "launch-a"));

    assert_eq!(
        registry.acknowledge_startup("tab-a", "launch-a"),
        Some(AgentStartupAcknowledgement::Acknowledged)
    );
    assert_eq!(ready.recv().unwrap(), AgentStartupSignal::Ready);
    assert_eq!(
        registry.acknowledge_startup("tab-a", "launch-a"),
        Some(AgentStartupAcknowledgement::AlreadyAcknowledged)
    );
    assert_eq!(registry.acknowledge_startup("tab-a", "launch-b"), None);

    let replacement = startup_receiver(&registry, creator_authority("tab-a", "launch-b"));
    assert_eq!(registry.acknowledge_startup("tab-a", "launch-a"), None);
    assert_eq!(
        registry.acknowledge_startup("tab-a", "launch-b"),
        Some(AgentStartupAcknowledgement::Acknowledged)
    );
    assert_eq!(replacement.recv().unwrap(), AgentStartupSignal::Ready);
}

#[test]
fn launch_command_finish_cannot_outrank_readiness() {
    let registry = WorkerAckRegistry::default();
    let receiver = startup_receiver(&registry, creator_authority("tab-a", "launch-a"));

    assert!(registry.mark_startup_launch_command_finished("tab-a", "launch-a", Some(1)));
    assert!(registry.mark_startup_ready("tab-a", "launch-a"));
    assert_eq!(
        await_agent_startup_notice(&receiver, Duration::from_secs(1)),
        AgentStartupNoticeOutcome::Resolved
    );
}

#[test]
fn creator_hold_is_refused_for_every_other_caller() {
    let registry = WorkerAckRegistry::default();
    let _armed = startup_receiver(
        &registry,
        creator_authority("tab-a", "launch-a").in_team(Some("team_pair".into())),
    );

    for caller in ["general-agent", "sibling", "tab-a"] {
        let (refusal, supervised) = registry
            .hold_startup_authorized(
                caller,
                "tab-a",
                "launch-a",
                Some(&live_child("launch-a", Some("team_pair"))),
            )
            .expect_err("only the recorded creator may hold");
        assert_eq!(refusal.reason(), "creator_mismatch");
        assert_eq!(
            supervised
                .expect("the refusal carries its provenance")
                .creator_tab_id,
            "creator"
        );
    }

    assert!(registry
        .hold_startup_authorized(
            "creator",
            "tab-a",
            "launch-a",
            Some(&live_child("launch-a", Some("team_pair")))
        )
        .is_ok());
}

#[test]
fn stale_startup_expiry_cannot_reap_replaced_attempt() {
    let registry = WorkerAckRegistry::default();
    let lease = StartupTeamLease {
        team_id: "team_pair".into(),
        creator_tab_id: "creator".into(),
        member_tab_ids: vec!["tab-a".into()],
    };
    registry.register_startup_lease(lease.clone());
    let stale = startup_receiver(
        &registry,
        creator_authority("tab-a", "launch-a").in_team(Some(lease.team_id.clone())),
    );
    let current = startup_receiver(
        &registry,
        creator_authority("tab-a", "launch-b").in_team(Some(lease.team_id.clone())),
    );

    assert_eq!(stale.recv().unwrap(), AgentStartupSignal::Superseded);
    assert!(
        registry.expire_startup("tab-a", "launch-a").is_none(),
        "the replaced attempt owns no supervision record to retire"
    );
    assert_eq!(
        registry.active_startup_leases(),
        vec![lease.clone()],
        "a stale timer never releases the lease of a newer attempt"
    );

    let expired = registry
        .expire_startup("tab-a", "launch-b")
        .expect("the live attempt retires its own record");
    assert_eq!(expired.attempt_id, "launch-b");
    assert_eq!(expired.team_id.as_deref(), Some("team_pair"));
    assert!(current.recv().is_err());

    let held_after_expiry = registry.hold_startup_authorized(
        "creator",
        "tab-a",
        "launch-b",
        Some(&live_child("launch-b", Some("team_pair"))),
    );
    assert!(
        held_after_expiry.is_err(),
        "a retired attempt cannot be held again"
    );
}

#[test]
fn a_retired_lease_cannot_be_reaped_from_a_stale_candidate() {
    let registry = Arc::new(WorkerAckRegistry::default());
    let (mut mux, creator) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&creator).unwrap();
    let child = mux.create_tab(&workspace_id).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&child).unwrap().team_id;
    let lease = StartupTeamLease {
        team_id: team_id.clone(),
        creator_tab_id: creator.clone(),
        member_tab_ids: vec![child.clone()],
    };
    registry.register_startup_lease(lease.clone());
    let _armed = startup_receiver(
        &registry,
        StartupAuthority::new(&creator, &child, "launch-a").in_team(Some(team_id.clone())),
    );
    // What the snapshot-then-mutate shape captured before the acknowledgement.
    let stale_candidates = registry.active_startup_leases();
    assert_eq!(stale_candidates, vec![lease]);
    mux.close_tab(&child).unwrap();

    let mux = Arc::new(Mutex::new(mux));
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let (reaped_tx, reaped_rx) = mpsc::channel::<Vec<String>>();
    let sweeper = {
        let registry = Arc::clone(&registry);
        let mux = Arc::clone(&mux);
        std::thread::spawn(move || {
            gate_rx.recv().unwrap();
            reaped_tx
                .send(registry.reap_startup_leases(&mut mux.lock()))
                .unwrap();
        })
    };

    assert!(
        registry.mark_startup_ready(&child, "launch-a"),
        "the acknowledgement retires the lease"
    );
    gate_tx.send(()).unwrap();
    let reaped = reaped_rx.recv().unwrap();
    sweeper.join().unwrap();

    assert!(
        reaped.is_empty(),
        "a lease retired before the mutation is not a reaping candidate"
    );
    assert!(
        mux.lock().get_team(&team_id).is_some(),
        "the Team the acknowledgement made normal survives the sweep"
    );
    assert_eq!(
        crate::service::startup_lease::reap_owned_forming_team(
            &mut mux.lock(),
            &stale_candidates[0].team_id,
            &stale_candidates[0].member_tab_ids,
        ),
        crate::service::startup_lease::TeamReapOutcome::Reaped,
        "the stale candidate still satisfies forming+empty: only the live record saves the Team"
    );
}

#[test]
fn a_started_attempt_releases_the_pair_lease() {
    let registry = WorkerAckRegistry::default();
    let lease = StartupTeamLease {
        team_id: "team_pair".into(),
        creator_tab_id: "creator".into(),
        member_tab_ids: vec!["tab-a".into(), "tab-b".into()],
    };
    registry.register_startup_lease(lease.clone());
    let _ready = startup_receiver(
        &registry,
        creator_authority("tab-a", "launch-a").in_team(Some(lease.team_id.clone())),
    );

    assert!(registry.mark_startup_ready("tab-a", "launch-a"));
    assert!(
        registry.active_startup_leases().is_empty(),
        "a started attempt makes its Team a normal Team"
    );
}

#[test]
fn startup_ack_requires_the_current_pty_and_launch_nonce() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-current".into());
    tab.session_binding_nonce = Some("nonce-current".into());

    assert!(startup_ack_owns_attempt(
        tab,
        "pty-current",
        "nonce-current"
    ));
    assert!(!startup_ack_owns_attempt(tab, "pty-stale", "nonce-current"));
    assert!(!startup_ack_owns_attempt(tab, "pty-current", "nonce-stale"));
}

#[test]
fn startup_supervisor_registration_is_idempotent_for_the_same_task_identity() {
    let registry = WorkerAckRegistry::default();
    let task_identity = StartupTaskIdentity {
        plan_id: "plan".into(),
        task_id: "task".into(),
        attempt_id: "task-attempt".into(),
    };
    registry.register_startup_supervisor(
        "tab-a",
        "parent",
        "Planner",
        Some("team_pair"),
        Some(task_identity.clone()),
    );

    let supervisor = registry.startup_supervisor("tab-a").unwrap();
    registry.register_startup_supervisor(
        "tab-a",
        "parent",
        "Planner",
        Some("team_pair"),
        Some(task_identity),
    );
    assert_eq!(supervisor.supervisor_tab_id, "parent");
    assert_eq!(supervisor.agent_title, "Planner");
    assert_eq!(supervisor.team_id.as_deref(), Some("team_pair"));
    assert_eq!(registry.startup_supervisor("tab-a"), Some(supervisor));
}

#[test]
fn task_bound_startup_fails_without_a_supervisor_and_accepts_hold_when_registered() {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_tab_id).unwrap();
    let child_tab_id = mux.create_tab(&workspace_id).unwrap();
    let task_identity = StartupTaskIdentity {
        plan_id: "plan-a".into(),
        task_id: "task-a".into(),
        attempt_id: "attempt-a".into(),
    };
    mux.get_tab_mut(&parent_tab_id).unwrap().agent_type = Some("claude".into());
    let child = mux.get_tab_mut(&child_tab_id).unwrap();
    child.parent_tab_id = Some(parent_tab_id.clone());
    child.agent_type = Some("claude".into());
    child.pty_id = Some("pty-a".into());
    child.agent_session_id = Some("session-a".into());
    child.session_binding_nonce = Some("nonce-a".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    let missing = fixture
        .service
        .activate_agent_startup_supervision(&child_tab_id, "pty-a", "nonce-a", Some(&task_identity))
        .expect_err("task-bound launch cannot continue without a supervisor");
    assert!(
        missing.contains("no supervised startup waiter"),
        "{missing}"
    );

    assert!(fixture.service.prepare_agent_startup_supervision(
        &child_tab_id,
        Some(&parent_tab_id),
        Some(task_identity.clone()),
    ));
    fixture
        .service
        .activate_agent_startup_supervision(&child_tab_id, "pty-a", "nonce-a", Some(&task_identity))
        .unwrap();
    fixture
        .service
        .activate_agent_startup_supervision(&child_tab_id, "pty-a", "nonce-a", Some(&task_identity))
        .unwrap();
    assert_eq!(fixture.service.worker_ack.startup_waiters.lock().len(), 1);
    let held = fixture
        .service
        .hold_agent_startup(&parent_tab_id, &child_tab_id, "pty-a")
        .expect("the registered parent can hold the exact live startup binding");

    assert_eq!(held["held"], true);
    assert_eq!(
        held["authority"]["task_identity"]["attempt_id"],
        "attempt-a"
    );
    assert!(fixture.service.worker_ack.startup_waiters.lock().is_empty());
}

#[test]
fn task_startup_waiter_uses_task_attempt_session_and_binding_identity() {
    let registry = WorkerAckRegistry::default();
    let task_identity = StartupTaskIdentity {
        plan_id: "plan-a".into(),
        task_id: "task-a".into(),
        attempt_id: "attempt-a".into(),
    };
    let authority = StartupAuthority::new("parent", "child", "pty-a")
        .for_task(Some(task_identity.clone()))
        .for_binding(Some("session-a"), "nonce-a");
    let first = startup_receiver(&registry, authority.clone());

    assert!(matches!(
        registry.arm_startup(authority.clone()),
        AgentStartupArm::Extended
    ));
    assert_eq!(first.recv().unwrap(), AgentStartupSignal::DeadlineExtended);
    assert_eq!(
        registry.expire_startup_generation(&authority, 0),
        WaiterExpiry::Extended
    );

    let changed_binding = StartupAuthority::new("parent", "child", "pty-a")
        .for_task(Some(task_identity.clone()))
        .for_binding(Some("session-a"), "nonce-b");
    let current = startup_receiver(&registry, changed_binding.clone());
    assert_eq!(first.recv().unwrap(), AgentStartupSignal::Superseded);
    assert_eq!(
        registry.acknowledge_startup_for_binding(
            "child",
            "pty-a",
            Some("session-a"),
            Some("nonce-a")
        ),
        None
    );
    let newer_task_identity = StartupTaskIdentity {
        attempt_id: "attempt-b".into(),
        ..task_identity
    };
    let newer_task_attempt = StartupAuthority::new("parent", "child", "pty-a")
        .for_task(Some(newer_task_identity.clone()))
        .for_binding(Some("session-a"), "nonce-b");
    let replacement = startup_receiver(&registry, newer_task_attempt.clone());
    assert_eq!(current.recv().unwrap(), AgentStartupSignal::Superseded);
    assert_eq!(
        registry.acknowledge_startup_for_binding(
            "child",
            "pty-a",
            Some("session-b"),
            Some("nonce-b")
        ),
        None
    );
    assert_eq!(
        registry.acknowledge_startup_for_binding(
            "child",
            "pty-a",
            Some("session-a"),
            Some("nonce-b")
        ),
        Some(AgentStartupAcknowledgement::Acknowledged)
    );
    assert_eq!(replacement.recv().unwrap(), AgentStartupSignal::Ready);
    assert_eq!(
        registry.acknowledge_startup_for_binding(
            "child",
            "pty-a",
            Some("session-a"),
            Some("nonce-b")
        ),
        Some(AgentStartupAcknowledgement::AlreadyAcknowledged)
    );
    assert!(matches!(
        registry.arm_startup(newer_task_attempt),
        AgentStartupArm::AlreadyAcknowledged
    ));
}

#[test]
fn startup_notice_exposes_inspect_respond_and_hold_without_task_resend() {
    let event = agent_startup_notice_event("child", "Planner", "launch-a");
    assert_eq!(event["kind"], "agent_startup_overdue");
    assert_eq!(
        event["deadline_seconds"],
        AGENT_STARTUP_NOTICE_TIMEOUT.as_secs()
    );
    assert_eq!(event["tab_state"], TAB_STATE_ALIVE_AWAITING_INSPECTION);
    assert_eq!(
        event["recovery"]["capability"],
        crate::prompts::INTERACTIVE_CHILD_PTY_CAPABILITY_ID
    );
    assert_eq!(event["recovery"]["target_tab_id"], "child");
    assert_eq!(event["recovery"]["attempt_id"], "launch-a");
    assert_eq!(
        event["hold"],
        "codeterm agent startup-hold --tab child --attempt launch-a"
    );
    assert!(event["inspect"].as_array().unwrap().iter().any(|command| {
        command
            .as_str()
            .is_some_and(|command| command.contains("tab get-text"))
    }));
    assert!(event.get("deadline_basis").is_some());
}

#[test]
fn expired_startup_reports_a_surviving_tab_without_cleanup_schema() {
    let event = agent_startup_failed_event(
        "child",
        "Planner",
        "launch-a",
        Some(17),
        Some("synthetic provider rejection"),
    );
    assert_eq!(event["kind"], "agent_startup_failed");
    assert_eq!(
        event["deadline_seconds"],
        (AGENT_STARTUP_NOTICE_TIMEOUT + AGENT_STARTUP_FINAL_TIMEOUT).as_secs()
    );
    assert_eq!(event["tab_state"], TAB_STATE_ALIVE_AWAITING_INSPECTION);
    assert_eq!(
        event["recovery"]["capability"],
        crate::prompts::INTERACTIVE_CHILD_PTY_CAPABILITY_ID
    );
    assert!(event.get("cleanup").is_none());
    assert!(event.get("cleanup_error").is_none());
    assert_eq!(event["provider"]["exit_code"], 17);
    assert_eq!(event["provider"]["output"], "synthetic provider rejection");
}

#[test]
fn worker_ack_notice_is_a_short_single_line_with_agent_provider_and_attempt() {
    let notice = worker_lifecycle_notice(
        "Otto",
        Some("opencode"),
        None,
        &json!({
            "kind": "worker_ack",
            "task_id": "task_g94uu5",
            "attempt_id": "attempt_rjd7ab",
        }),
    );

    assert_eq!(notice.lines().count(), 1);
    assert!(!notice.starts_with('{'));
    assert!(notice.contains("Otto"));
    assert!(notice.contains("opencode"));
    assert!(notice.contains("task_g94uu5"));
    assert!(notice.contains("attempt_rjd7ab"));
    assert!(notice.len() < 180);
}

#[test]
fn provider_startup_failure_is_in_the_single_line_notice() {
    let notice = worker_lifecycle_notice(
        "Nora",
        Some("opencode"),
        None,
        &json!({
            "kind": "agent_startup_failed",
            "attempt_id": "attempt-launch",
            "provider": {
                "exit_code": 1,
                "output": "provider dialog\nInsufficient account funds",
            },
        }),
    );

    assert_eq!(notice.lines().count(), 1);
    assert!(!notice.starts_with('{'));
    assert!(notice.contains("Nora"));
    assert!(notice.contains("opencode"));
    assert!(notice.contains("attempt-launch"));
    assert!(notice.contains("Insufficient account funds"));
    assert!(notice.len() < 280);
}

#[test]
fn task_startup_failure_detail_is_in_the_short_worker_notice() {
    let notice = worker_lifecycle_notice(
        "Otto",
        Some("opencode"),
        Some("Insufficient account funds"),
        &json!({
            "kind": "worker_ack_startup_failed",
            "task_id": "task_g94uu5",
            "attempt_id": "attempt_rjd7ab",
        }),
    );

    assert_eq!(notice.lines().count(), 1);
    assert!(notice.contains("Otto"));
    assert!(notice.contains("opencode"));
    assert!(notice.contains("task_g94uu5"));
    assert!(notice.contains("attempt_rjd7ab"));
    assert!(notice.contains("Insufficient account funds"));
    assert!(notice.len() < 280);
}
