use super::*;

#[test]
fn heartbeat_popup_body_round_trips_the_scoped_emission() {
    let gate = Gate::default();
    assert_eq!(gate.last_body("tab-a", "plan-a"), None);
    let body = "opaque-delivery\nsecond-line\n界";
    gate.remember_body("tab-a", "plan-a", body);
    gate.remember_body("tab-b", "plan-a", "other-tab");
    gate.remember_body("tab-a", "plan-b", "other-plan");
    assert_eq!(gate.last_body("tab-a", "plan-a").as_deref(), Some(body));
    gate.remember_body("tab-a", "plan-a", "updated-body");
    assert_eq!(
        gate.last_body("tab-a", "plan-a").as_deref(),
        Some("updated-body")
    );
    for index in 0..256 {
        gate.remember_body(&format!("tab-{index}"), "plan-x", "bounded");
    }
    assert_eq!(gate.last_body("tab-b", "plan-a"), None);
}

fn healthy_snapshot() -> Snapshot {
    Snapshot {
        plan_id: "plan".into(),
        workers: vec![Worker {
            task_id: "task".into(),
            tab_id: "worker".into(),
            ack_received: true,
            session_key: Some("own-session:nonce".into()),
            process: "present".into(),
            activity: "working".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn every_hard_wake() -> Vec<HardWake> {
    vec![
        HardWake::UndeliveredReport("r".into()),
        HardWake::NewReport("r".into()),
        HardWake::TaskState("task".into()),
        HardWake::FailedGate("g".into()),
        HardWake::OwnerMessage("m".into()),
        HardWake::ChoiceAnswer("c".into()),
        HardWake::Incident("i".into()),
        HardWake::ModeChange,
    ]
}

fn changed() -> Snapshot {
    let mut snapshot = healthy_snapshot();
    snapshot.delta.insert("task:task".into(), "changed".into());
    snapshot
}

#[test]
fn heartbeat_rule_same_snapshot_never_wakes() {
    let decision = state::rule(&changed(), true, None, None, 100, 60);
    assert!(!decision.wake);
    assert_eq!(decision.reason, "same_snapshot");
}

#[test]
fn heartbeat_rule_changed_with_step_wakes_for_owner_and_worker() {
    for kind in [ReminderKind::Owner, ReminderKind::Worker] {
        let mut snapshot = changed();
        snapshot.scope.kind = kind;
        let decision = state::rule(&snapshot, false, None, None, 100, 60);
        assert!(decision.wake);
        assert!(!decision.next_steps.is_empty());
        assert_eq!(decision.decided_by, "rule");
    }
}

#[test]
fn heartbeat_rule_healthy_unchanged_and_changes_without_step_stay_quiet() {
    assert!(!state::rule(&healthy_snapshot(), false, None, None, 100, 60).wake);
    let mut snapshot = healthy_snapshot();
    snapshot
        .delta
        .insert("stride:noise".into(), "changed".into());
    assert!(!state::rule(&snapshot, false, None, None, 100, 60).wake);
}

#[test]
fn heartbeat_rule_repeated_soft_class_respects_cooldown() {
    let snapshot = changed();
    let class = cadence(&snapshot).soft_class();
    let decision = state::rule(&snapshot, false, Some(&class), Some(100), 159, 60);
    assert!(!decision.wake);
    assert_eq!(decision.reason, "soft_cooldown");
    assert!(state::rule(&snapshot, false, Some(&class), Some(100), 160, 60).wake);
}

#[test]
fn heartbeat_rule_hard_event_bypasses_soft_cooldown() {
    for event in every_hard_wake() {
        let mut snapshot = healthy_snapshot();
        snapshot.hard.push(event);
        let class = cadence(&snapshot).soft_class();
        assert!(state::rule(&snapshot, false, Some(&class), Some(100), 101, 60).wake);
    }
}

#[test]
fn heartbeat_rule_cache_and_failed_attempt_do_not_replay() {
    let gate = Gate::default();
    let snapshot = changed();
    let claim = gate.claim("key").unwrap();
    let decision = gate.decide("key", &snapshot, 100).unwrap();
    assert!(gate.attempt(&claim, &decision));
    assert!(!gate.attempt(&claim, &decision));
    gate.failed("key");
    let repeated = gate.decide("key", &snapshot, 10000).unwrap();
    assert!(!repeated.wake);
    assert_eq!(repeated.reason, "same_snapshot");
}

#[test]
fn heartbeat_rule_delivered_baseline_keeps_empty_delta_cached() {
    let gate = Gate::default();
    let mut snapshot = changed();
    let decision = gate.decide("key", &snapshot, 100).unwrap();
    gate.delivered("key", &snapshot, &decision, 100);
    snapshot.delta.clear();
    assert_eq!(
        gate.decide("key", &snapshot, 10000).unwrap().reason,
        "same_snapshot"
    );
}

#[test]
fn heartbeat_rule_journal_has_rule_and_no_model() {
    let decision = cadence(&changed());
    let event = decision.journal_event("tab", "plan", 100);
    assert_eq!(event["decided_by"], "rule");
    assert!(event["adapter"].is_null());
    assert!(event["model_id"].is_null());
}

#[test]
fn open_liveness_check_drops_evidence_already_held() {
    let checks = ["ACK", "bound session", "provider liveness"];
    let cases = [
        (
            Worker {
                task_id: "task_a".into(),
                ack_received: true,
                session_key: Some("sess".into()),
                process: "present".into(),
                ..Default::default()
            },
            Vec::<&str>::new(),
        ),
        (
            Worker {
                task_id: "task_a".into(),
                ack_received: false,
                session_key: Some("sess".into()),
                process: "present".into(),
                ..Default::default()
            },
            vec!["ACK"],
        ),
        (
            Worker {
                task_id: "task_a".into(),
                ack_received: true,
                session_key: None,
                process: "absent".into(),
                ..Default::default()
            },
            vec!["bound session", "provider liveness"],
        ),
        (
            Worker {
                task_id: "task_a".into(),
                process: "absent".into(),
                ..Default::default()
            },
            vec!["ACK", "bound session", "provider liveness"],
        ),
    ];
    for (worker, expected) in cases {
        assert_eq!(worker.missing_liveness(), expected);
        let text = worker.open_liveness_check();
        assert!(text.contains(&worker.task_id));
        for check in checks {
            assert_eq!(text.contains(check), expected.contains(&check));
        }
    }
}

#[test]
fn persisted_owner_message_wakes_are_dropped_and_others_kept() {
    let kept = super::actionable_pending(&every_hard_wake());
    assert!(!kept
        .iter()
        .any(|wake| matches!(wake, HardWake::OwnerMessage(_))));
    assert_eq!(kept.len(), every_hard_wake().len() - 1);
}

#[test]
fn fresh_owner_messages_need_a_cursor_for_this_session() {
    use super::snapshot::{fresh_owner_messages, had_session_cursor};

    let session = Some("sess".into());
    let other = Some("other".into());
    let watched = Worker {
        session_key: session.clone(),
        turn_cursor: Some("turn-1".into()),
        ..Default::default()
    };
    assert!(!had_session_cursor(None, &session));
    assert!(!had_session_cursor(
        Some(&Worker {
            session_key: session.clone(),
            ..Default::default()
        }),
        &session
    ));
    assert!(!had_session_cursor(
        Some(&Worker {
            session_key: other.clone(),
            turn_cursor: Some("turn-1".into()),
            ..Default::default()
        }),
        &session
    ));
    assert!(had_session_cursor(Some(&watched), &session));
    assert!(fresh_owner_messages(false, ["m1", "m2"]).is_empty());
    let fresh = fresh_owner_messages(true, ["m1", "m2"]);
    assert!(matches!(fresh[0], HardWake::OwnerMessage(ref id) if id == "m1"));
    assert!(matches!(fresh[1], HardWake::OwnerMessage(ref id) if id == "m2"));
}

#[test]
fn heartbeat_rule_soft_cooldown_applies_to_both_kinds_and_new_hard_event_wakes() {
    for kind in [ReminderKind::Owner, ReminderKind::Worker] {
        let gate = Gate::default();
        let mut snapshot = changed();
        snapshot.scope.kind = kind;
        let first = gate
            .decide_with_interval("key", &snapshot, 100, 60)
            .unwrap();
        assert!(first.wake);
        gate.delivered("key", &snapshot, &first, 100);
        snapshot
            .delta
            .insert("task:second".into(), "changed".into());
        let second = gate
            .decide_with_interval("key", &snapshot, 101, 60)
            .unwrap();
        assert!(!second.wake);
        assert_eq!(second.reason, "soft_cooldown");
        snapshot.hard.push(HardWake::NewReport("new-report".into()));
        assert!(
            gate.decide_with_interval("key", &snapshot, 102, 60)
                .unwrap()
                .wake
        );
    }
}

#[test]
fn heartbeat_rule_claims_are_exclusive_and_stale_claims_cannot_attempt() {
    let gate = Gate::default();
    let claim = gate.claim("key").unwrap();
    assert!(matches!(gate.claim("key"), Err(state::Refusal::InFlight)));
    let decision = gate.decide("key", &changed(), 100).unwrap();
    gate.retain(&HashSet::new());
    let replacement = gate.claim("key").unwrap();
    assert!(!gate.attempt(&claim, &decision));
    drop(claim);
    assert!(matches!(gate.claim("key"), Err(state::Refusal::InFlight)));
    drop(replacement);
    assert!(gate.claim("key").is_ok());
}

#[test]
fn heartbeat_rule_prompt_and_unhealthy_worker_have_concrete_steps() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].prompt = Some("opaque-prompt".into());
    assert!(cadence(&snapshot).wake);
    snapshot.workers[0].prompt = None;
    snapshot.workers[0].session_key = None;
    assert!(cadence(&snapshot).wake);
    snapshot.workers[0].held = true;
    assert!(cadence(&snapshot).wake);
}

#[test]
fn heartbeat_rule_hold_excuses_soft_drift() {
    let mut snapshot = healthy_snapshot();
    snapshot.workers[0].held = true;
    snapshot.workers[0].repeating = true;
    snapshot.workers[0].idle_overdue = true;
    assert!(!cadence(&snapshot).wake);
}
