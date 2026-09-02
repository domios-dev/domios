use super::*;

#[test]
fn acknowledgement_resolves_only_the_matching_attempt() {
    let registry = WorkerAckRegistry::default();
    let receiver = registry.arm("tab", "attempt-a");

    assert!(!registry.acknowledge("tab", "attempt-b"));
    assert!(registry.acknowledge("tab", "attempt-a"));
    assert_eq!(receiver.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn rearm_supersedes_the_previous_deadline() {
    let registry = WorkerAckRegistry::default();
    let previous = registry.arm("tab", "attempt-a");
    let current = registry.arm("tab", "attempt-b");

    assert_eq!(previous.recv().unwrap(), WorkerAckSignal::Superseded);
    assert!(registry.acknowledge("tab", "attempt-b"));
    assert_eq!(current.recv().unwrap(), WorkerAckSignal::Acknowledged);
}

#[test]
fn expiry_removes_only_the_matching_deadline() {
    let registry = WorkerAckRegistry::default();
    let receiver = registry.arm("tab", "attempt-a");

    assert!(!registry.expire("tab", "attempt-b"));
    assert!(registry.expire("tab", "attempt-a"));
    assert!(receiver.recv().is_err());
}

#[test]
fn bound_worker_missing_ack_gets_one_system_reminder() {
    assert_eq!(
        first_deadline_action(true),
        WorkerAckFirstDeadline::SystemReminder
    );
    assert_eq!(
        WORKER_ACK_REMINDER,
        "Acknowledge this Worker assignment with codeterm agent ack before doing any work. If acknowledgement is still absent at the final bounded startup deadline, this tab will be cleaned up."
    );
    assert!(!WORKER_ACK_REMINDER.contains("Task body"));
}

#[test]
fn unbound_worker_gets_no_ack_reminder_or_input() {
    assert_eq!(
        first_deadline_action(false),
        WorkerAckFirstDeadline::StartupUnbound
    );
}

#[test]
fn worker_ack_final_deadline_cleans_once() {
    let registry = WorkerAckRegistry::default();
    let receiver = registry.arm("tab", "attempt");

    assert!(registry.expire("tab", "attempt"));
    assert!(!registry.expire("tab", "attempt"));
    assert!(receiver.recv().is_err());
}

#[test]
fn worker_ack_close_tab_failure_is_a_coordination_incident() {
    let cleanup_error = close_worker_after_final_deadline(|| Err("tab remained open".into()));
    let event = worker_ack_timeout_event(
        "plan",
        "task",
        "tab",
        "attempt",
        "codeterm plan task dispatch plan task",
        cleanup_error.as_deref(),
    );

    assert_eq!(cleanup_error.as_deref(), Some("tab remained open"));
    assert_eq!(event["kind"], "worker_ack_cleanup_failed");
    assert_eq!(event["cleanup"]["ok"], false);
    assert!(event["impact"]
        .as_str()
        .unwrap()
        .contains("ORCHESTRATION PRODUCT INCIDENT"));
    assert!(event["fallback"].as_str().unwrap().contains("still live"));
}

fn creator_authority(child_tab_id: &str, attempt_id: &str) -> StartupAuthority {
    StartupAuthority::new("creator", child_tab_id, attempt_id)
}

fn live_child(attempt_id: &str, team_id: Option<&str>) -> ChildStartupState {
    ChildStartupState {
        attempt_id: Some(attempt_id.to_string()),
        team_id: team_id.map(str::to_string),
    }
}

#[test]
fn startup_acknowledgement_and_hold_resolve_only_the_current_attempt() {
    let registry = WorkerAckRegistry::default();
    let ready = registry.arm_startup(creator_authority("tab-a", "launch-a"));
    assert!(!registry.mark_startup_ready("tab-a", "launch-old"));
    assert!(registry.mark_startup_ready("tab-a", "launch-a"));
    assert_eq!(ready.recv().unwrap(), AgentStartupSignal::Ready);

    let held = registry.arm_startup(creator_authority("tab-a", "launch-b"));
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

    let exited = registry.arm_startup(creator_authority("tab-a", "launch-c"));
    assert!(!registry.mark_startup_launch_command_finished("tab-a", "launch-old"));
    assert!(registry.mark_startup_launch_command_finished("tab-a", "launch-c"));
    assert_eq!(
        exited.recv().unwrap(),
        AgentStartupSignal::LaunchCommandFinished
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
fn launch_command_finish_cannot_outrank_readiness() {
    let registry = WorkerAckRegistry::default();
    let receiver = registry.arm_startup(creator_authority("tab-a", "launch-a"));

    assert!(registry.mark_startup_launch_command_finished("tab-a", "launch-a"));
    assert!(registry.mark_startup_ready("tab-a", "launch-a"));
    assert_eq!(
        await_agent_startup_notice(&receiver, Duration::from_secs(1)),
        AgentStartupNoticeOutcome::Resolved
    );
}

#[test]
fn creator_hold_is_refused_for_every_other_caller() {
    let registry = WorkerAckRegistry::default();
    let _armed = registry
        .arm_startup(creator_authority("tab-a", "launch-a").in_team(Some("team_pair".into())));

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
fn stale_startup_cleanup_cannot_reap_replaced_attempt() {
    let registry = WorkerAckRegistry::default();
    let lease = StartupTeamLease {
        team_id: "team_pair".into(),
        creator_tab_id: "creator".into(),
        member_tab_ids: vec!["tab-a".into()],
    };
    registry.register_startup_lease(lease.clone());
    let stale = registry
        .arm_startup(creator_authority("tab-a", "launch-a").in_team(Some(lease.team_id.clone())));
    let current = registry
        .arm_startup(creator_authority("tab-a", "launch-b").in_team(Some(lease.team_id.clone())));

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
    let _armed = registry.arm_startup(
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
    let _ready = registry
        .arm_startup(creator_authority("tab-a", "launch-a").in_team(Some(lease.team_id.clone())));

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
fn startup_supervisor_registration_is_consumed_once() {
    let registry = WorkerAckRegistry::default();
    registry.register_startup_supervisor("tab-a", "parent", "Planner", Some("team_pair"));

    let supervisor = registry.take_startup_supervisor("tab-a").unwrap();
    assert_eq!(supervisor.supervisor_tab_id, "parent");
    assert_eq!(supervisor.agent_title, "Planner");
    assert_eq!(supervisor.team_id.as_deref(), Some("team_pair"));
    assert!(registry.take_startup_supervisor("tab-a").is_none());
}

#[test]
fn startup_notice_exposes_inspect_respond_and_hold_without_task_resend() {
    let event = agent_startup_notice_event("child", "Planner", "launch-a");
    assert_eq!(event["kind"], "agent_startup_overdue");
    assert_eq!(event["deadline_seconds"], 30);
    assert_eq!(
        event["hold"],
        "codeterm agent startup-hold --tab child --attempt launch-a"
    );
    assert!(event["inspect"].as_array().unwrap().iter().any(|command| {
        command
            .as_str()
            .is_some_and(|command| command.contains("tab get-text"))
    }));
    assert!(!event.to_string().contains("send the task"));
}

#[test]
fn exited_startup_is_cleaned_at_one_minute() {
    let event = agent_startup_failed_event("child", "Planner", "launch-a", None);
    assert_eq!(event["kind"], "agent_startup_failed");
    assert_eq!(event["deadline_seconds"], 60);
    assert_eq!(event["cleanup"]["ok"], true);
}

#[test]
fn startup_cleanup_holds_the_gate_across_tab_close() {
    let registry = Arc::new(WorkerAckRegistry::default());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let cleanup_registry = registry.clone();
    let cleanup = std::thread::spawn(move || {
        serialize_agent_startup_cleanup(&cleanup_registry, || {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
    });
    entered_rx.recv().unwrap();
    assert!(registry.startup_cleanup.try_lock().is_none());
    release_tx.send(()).unwrap();
    cleanup.join().unwrap();
    assert!(registry.startup_cleanup.try_lock().is_some());
}
