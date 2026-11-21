use super::*;

#[test]
fn test_get_reports() {
    let mut store = ReportStore::new(100);
    store.add("cond-1", "w1", "Worker 1", "Done");
    store.add("cond-1", "w2", "Worker 2", "Done too");
    store.add("cond-2", "w3", "Worker 3", "Other orchestrator");

    let reports = store.get("cond-1", false);
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].from_tab_id, "w1");
    assert_eq!(reports[1].from_tab_id, "w2");
}

#[test]
fn test_get_reports_clear() {
    let mut store = ReportStore::new(100);
    store.add("cond-1", "w1", "Worker", "Done");
    let reports = store.get("cond-1", true);
    assert_eq!(reports.len(), 1);
    let reports2 = store.get("cond-1", false);
    assert_eq!(reports2.len(), 0);
}

#[test]
fn pending_reports_clear_only_after_delivery() {
    let mut store = ReportStore::new(100);
    let report = store.add("cond-1", "w1", "W1", "ready");
    assert_eq!(store.pending_for("cond-1").len(), 1);
    assert_eq!(store.claim_pending_for("cond-1").len(), 1);
    assert!(store.pending_for("cond-1").is_empty());
    assert!(store.claim_pending_for("cond-1").is_empty());
    store.release_delivery(&report.id);
    assert_eq!(store.pending_for("cond-1").len(), 1);
    assert_eq!(store.claim_pending_for("cond-1").len(), 1);
    store.mark_delivered(&report.id);
    assert!(store.pending_for("cond-1").is_empty());
    assert!(store.claim_pending_for("cond-1").is_empty());
}

#[test]
fn test_eviction() {
    let mut store = ReportStore::new(3);
    store.add("cond-1", "w1", "W1", "A");
    store.add("cond-1", "w2", "W2", "B");
    store.add("cond-1", "w3", "W3", "C");
    store.add("cond-1", "w4", "W4", "D");
    assert_eq!(store.count("cond-1"), 3);
    let reports = store.get("cond-1", false);
    assert_eq!(reports[0].message, "B"); // A was evicted
}

#[test]
fn test_has_report_from() {
    let mut store = ReportStore::new(100);
    assert!(!store.has_report_from("cond-1", "w1"));
    store.add("cond-1", "w1", "W1", "done");
    assert!(store.has_report_from("cond-1", "w1"));
    // Different reporter under the same target is not a match.
    assert!(!store.has_report_from("cond-1", "w2"));
    // Same reporter under a different target is not a match.
    assert!(!store.has_report_from("cond-2", "w1"));
}

#[test]
fn has_report_from_after_uses_the_worker_turn_boundary() {
    let mut store = ReportStore::new(100);
    let before_report = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(1);
    store.add("cond-1", "w1", "W1", "done");
    assert!(store.has_report_from_after("cond-1", "w1", before_report));

    let after_report = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_add(30);
    assert!(!store.has_report_from_after("cond-1", "w1", after_report));
}

#[test]
fn structured_report_round_trips_through_serde() {
    let details = ReportDetails {
        plan_id: Some("pl_structured".to_string()),
        task_id: Some("task_structured".to_string()),
        owner_epoch: Some(2),
        caller_tab_id: Some("w1".into()),
        caller_session_id: Some("session".into()),
        session_binding_nonce: Some("nonce".into()),
        attempt_id: None,
        status: Some(ReportStatus::Partial),
        branch: Some("orch-p2-structured-report".to_string()),
        base: None,
        sha: Some("deadbeef".to_string()),
        changed_files: Some(vec!["a.rs".to_string(), "b.rs".to_string()]),
        test_cmd: Some("cargo test".to_string()),
        test_result: Some("ok".to_string()),
        skipped: Some("e2e".to_string()),
        attachments: vec![],
    };
    let mut store = ReportStore::new(100);
    let stored = store.add_detailed("orch", "w1", "Worker", "all done", details, None);

    // Through the store, the typed fields survive.
    let listed = store.get("orch", false);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].plan_id.as_deref(), Some("pl_structured"));
    assert_eq!(listed[0].task_id.as_deref(), Some("task_structured"));
    assert_eq!(listed[0].status, Some(ReportStatus::Partial));
    assert_eq!(listed[0].changed_files.as_deref().unwrap().len(), 2);

    // Through serde JSON, every field round-trips byte-for-byte.
    let json = serde_json::to_string(&stored).unwrap();
    let back: AgentReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.plan_id.as_deref(), Some("pl_structured"));
    assert_eq!(back.task_id.as_deref(), Some("task_structured"));
    assert_eq!(back.status, Some(ReportStatus::Partial));
    assert_eq!(back.branch.as_deref(), Some("orch-p2-structured-report"));
    assert_eq!(back.sha.as_deref(), Some("deadbeef"));
    assert_eq!(
        back.changed_files,
        Some(vec!["a.rs".to_string(), "b.rs".to_string()])
    );
    assert_eq!(back.test_cmd.as_deref(), Some("cargo test"));
    assert_eq!(back.test_result.as_deref(), Some("ok"));
    assert_eq!(back.skipped.as_deref(), Some("e2e"));
}

#[test]
fn report_exposes_actual_requested_and_runtime_mismatch() {
    let mismatch = crate::multiplexer::types::ModelIdentityMismatch {
        planned_model: Some("codex/gpt-5.6-luna".into()),
        requested_model: "gpt-5.6-luna".into(),
        provider_resolved_model: Some("gpt-5.6-luna".into()),
        actual_model: "gpt-5.6-terra".into(),
        reason: "runtime_session_model_mismatch".into(),
    };
    let mut store = ReportStore::new(100);
    let report = store.add_detailed_with_model_identity(
        "orch",
        "w1",
        "Worker",
        "done",
        ReportDetails::default(),
        Some(ReportModelIdentity {
            actual_model: Some("gpt-5.6-terra".into()),
            planned_model: Some("codex/gpt-5.6-luna".into()),
            requested_model: Some("gpt-5.6-luna".into()),
            provider_resolved_model: Some("gpt-5.6-luna".into()),
            fallback: Some(crate::multiplexer::types::ModelFallbackNotice {
                planned_model: Some("codex/gpt-5.6-luna".into()),
                requested_model: "gpt-5.6-luna".into(),
                provider_resolved_model: "gpt-5.6-luna".into(),
                actual_model: Some("gpt-5.6-terra".into()),
                reason: "runtime_session_model_mismatch".into(),
            }),
            mismatch: Some(mismatch.clone()),
            reasoning_effort: crate::multiplexer::types::ReasoningEffortIdentity {
                planned: Some("high".into()),
                requested: Some("high".into()),
                resolved: Some("high".into()),
                actual: None,
                actual_source: Some("provider-default-unverified".into()),
            },
        }),
    );

    assert_eq!(report.model.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(report.actual_model.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(report.planned_model.as_deref(), Some("codex/gpt-5.6-luna"));
    assert_eq!(report.requested_model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(
        report.provider_resolved_model.as_deref(),
        Some("gpt-5.6-luna")
    );
    assert!(report.model_fallback.is_some());
    assert_eq!(report.model_identity_mismatch, Some(mismatch));
    assert_eq!(report.reasoning_effort.requested.as_deref(), Some("high"));
    assert_eq!(report.reasoning_effort.actual, None);
    assert_eq!(
        report.reasoning_effort.actual_source.as_deref(),
        Some("provider-default-unverified")
    );
}

#[test]
fn message_only_report_injects_just_the_message() {
    // proving no regression for P1's synthetic message-only reports.
    let mut store = ReportStore::new(100);
    let report = store.add("orch", "w1", "Worker", "just the body");
    assert_eq!(report.inject_body(), "just the body");
}

#[test]
fn structured_inject_block_renders_in_stable_order() {
    let details = ReportDetails {
        plan_id: Some("pl_1".to_string()),
        task_id: Some("task_1".to_string()),
        owner_epoch: Some(2),
        caller_tab_id: Some("w1".into()),
        caller_session_id: Some("session".into()),
        session_binding_nonce: Some("nonce".into()),
        attempt_id: None,
        status: Some(ReportStatus::Done),
        branch: Some("feature-x".to_string()),
        base: None,
        sha: Some("abc123".to_string()),
        changed_files: Some(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]),
        test_cmd: Some("cargo test".to_string()),
        test_result: Some("42 passed".to_string()),
        skipped: Some("manual UI step".to_string()),
        attachments: vec![],
    };
    let mut store = ReportStore::new(100);
    let report = store.add_detailed("orch", "w1", "Worker", "the freeform body", details, None);
    let expected = "task: pl_1/task_1\n\
         status: Done\n\
         branch: feature-x\n\
         sha: abc123\n\
         tests: cargo test -> 42 passed\n\
         skipped: manual UI step\n\
         the freeform body";
    let body = report.inject_body();
    assert_eq!(body, expected);
}

#[test]
fn blocked_report_injects_a_proposal_marker_and_keeps_its_evidence() {
    let details = ReportDetails {
        plan_id: Some("pl_1".into()),
        task_id: Some("task_1".into()),
        status: Some(ReportStatus::Blocked),
        test_cmd: Some("focused-check".into()),
        test_result: Some("blocked by dependency".into()),
        ..Default::default()
    };
    let report = ReportStore::new(100).add_detailed(
        "orch",
        "worker",
        "Worker",
        "dependency unavailable",
        details,
        None,
    );

    let body = report.inject_body();
    assert!(body.contains(BLOCK_PROPOSAL_NOTICE));
    assert!(body.contains("tests: focused-check -> blocked by dependency"));
    assert_eq!(
        body,
        format!(
            "task: pl_1/task_1\n{BLOCK_PROPOSAL_NOTICE}\ntests: focused-check -> blocked by dependency\ndependency unavailable"
        )
    );
    assert_eq!(report.status, Some(ReportStatus::Blocked));
}

#[test]
fn report_status_serializes_lowercase() {
    assert_eq!(
        serde_json::to_string(&ReportStatus::Done).unwrap(),
        "\"done\""
    );
    assert_eq!(
        serde_json::from_str::<ReportStatus>("\"blocked\"").unwrap(),
        ReportStatus::Blocked
    );
    assert_eq!(
        serde_json::to_string(&ReportStatus::InReview).unwrap(),
        "\"in_review\""
    );
}

#[test]
fn ledger_backed_report_persists_latest_status_across_reopen() {
    use crate::agent::ledger::OrchestrationLedger;

    let db = std::env::temp_dir().join(format!(
        "reports-ledger-{}/orchestration.db",
        ulid::Ulid::new()
    ));
    {
        let ledger = std::sync::Arc::new(OrchestrationLedger::open_at(&db).unwrap());
        let mut store = ReportStore::with_ledger(100, ledger);
        // Same worker reports twice with changing status + model.
        let mut d = ReportDetails::default();
        d.status = Some(ReportStatus::Partial);
        d.branch = Some("orch-p3-ledger".into());
        store.add_detailed("orch", "w1", "Worker", "wip", d.clone(), Some("opus"));
        d.status = Some(ReportStatus::Done);
        store.add_detailed("orch", "w1", "Worker", "done", d, Some("opus"));
    } // store + ledger Connection dropped — simulates restart.

    // latest status.
    let reopened = OrchestrationLedger::open_at(&db).unwrap();
    let rows = reopened.list_for_orchestrator("orch").unwrap();
    assert_eq!(rows.len(), 1, "one row per worker after re-report");
    assert_eq!(rows[0].status, Some(ReportStatus::Done));
    assert_eq!(rows[0].model.as_deref(), Some("opus"));
    assert_eq!(rows[0].branch.as_deref(), Some("orch-p3-ledger"));
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn spawn_audit_row_gives_roleless_pane_a_durable_trail() {
    use crate::agent::ledger::OrchestrationLedger;
    let db = std::env::temp_dir().join(format!(
        "reports-audit-{}/orchestration.db",
        ulid::Ulid::new()
    ));
    let ledger = std::sync::Arc::new(OrchestrationLedger::open_at(&db).unwrap());
    let store = ReportStore::with_ledger(100, ledger);
    store.record_spawn_audit("parent_tab", "roleless_tab", "install watchdog", 77);
    let rows = store.ledger_rows("parent_tab");
    assert_eq!(
        rows.len(),
        1,
        "the roleless spawn is now tracked under its parent"
    );
    assert_eq!(rows[0].worker_tab_id, "roleless_tab");
    assert_eq!(rows[0].title, "install watchdog");
    assert_eq!(rows[0].updated_at, 77);
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn completed_rows_surfaces_finished_lanes_only() {
    use crate::agent::ledger::OrchestrationLedger;
    let db = std::env::temp_dir().join(format!(
        "reports-done-{}/orchestration.db",
        ulid::Ulid::new()
    ));
    let ledger = std::sync::Arc::new(OrchestrationLedger::open_at(&db).unwrap());
    let mut store = ReportStore::with_ledger(100, ledger);
    let mut done = ReportDetails::default();
    done.status = Some(ReportStatus::Done);
    store.add_detailed("orch", "w_done", "Worker", "landed", done, None);
    let mut blocked = ReportDetails::default();
    blocked.status = Some(ReportStatus::Blocked);
    store.add_detailed("orch", "w_blocked", "Worker", "stuck", blocked, None);

    let completed = store.completed_rows(50);
    assert_eq!(
        completed.len(),
        1,
        "only finished lanes appear on the Done surface"
    );
    assert_eq!(completed[0].worker_tab_id, "w_done");
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn test_separate_orchestrators() {
    let mut store = ReportStore::new(100);
    store.add("cond-1", "w1", "W1", "msg1");
    store.add("cond-2", "w2", "W2", "msg2");
    assert_eq!(store.count("cond-1"), 1);
    assert_eq!(store.count("cond-2"), 1);
    store.remove("cond-1");
    assert_eq!(store.count("cond-1"), 0);
    assert_eq!(store.count("cond-2"), 1);
}

#[test]
fn host_report_metadata_is_not_deserialized_from_report_details() {
    let details: ReportDetails = serde_json::from_value(serde_json::json!({
        "kind": "forged_kind",
        "sender_position": {
            "source_session_id": "forged_session",
            "message_id": "forged_row"
        }
    }))
    .unwrap();
    let mut store = ReportStore::new(8);
    let report = store.add_detailed("target", "sender", "Sender", "opaque", details, None);

    assert!(report.sender_position.is_none());
    assert!(report.kind.is_none());
    let serialized = serde_json::to_value(report).unwrap();
    assert!(serialized.get("sender_position").is_none());
    assert!(serialized.get("kind").is_none());
}

#[test]
fn outbound_snapshot_keeps_original_metadata_and_is_bounded_independent_of_receipts() {
    let mut store = ReportStore::new(2);
    let first = store.add_detailed_with_host_metadata(
        "recipient-one",
        "sender",
        "Sender",
        "ready one",
        ReportDetails::default(),
        None,
        AgentReportHostMetadata {
            sender_position: Some(ReportSenderPosition {
                source_session_id: "session-one".into(),
                message_id: "ack-row".into(),
            }),
            kind: Some("dispatch_ready".into()),
        },
    );
    let second = store.add("recipient-two", "sender", "Sender", "ready two");
    let third = store.add("recipient-three", "sender", "Sender", "ready three");

    let snapshot = store.outbound_for_sender("sender");
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0].target_tab_id, "recipient-two");
    assert_eq!(snapshot[1].target_tab_id, "recipient-three");
    assert_ne!(
        snapshot[0].report.id, first.id,
        "sender retention evicted only its oldest row"
    );
    assert_eq!(snapshot[0].report.id, second.id);
    assert_eq!(snapshot[1].report.id, third.id);

    let mut retained_store = ReportStore::new(2);
    let retained = retained_store.add_detailed_with_host_metadata(
        "recipient",
        "sender",
        "Sender",
        "ready",
        ReportDetails::default(),
        None,
        AgentReportHostMetadata {
            sender_position: Some(ReportSenderPosition {
                source_session_id: "source-session".into(),
                message_id: "original-ack".into(),
            }),
            kind: Some("worker_ack".into()),
        },
    );
    let before_clear = retained_store.outbound_for_sender("sender");
    assert_eq!(before_clear[0].report.id, retained.id);
    assert_eq!(before_clear[0].report.timestamp, retained.timestamp);
    assert_eq!(
        before_clear[0]
            .report
            .sender_position
            .as_ref()
            .map(|position| position.message_id.as_str()),
        Some("original-ack")
    );
    assert_eq!(before_clear[0].report.kind.as_deref(), Some("worker_ack"));

    let received = retained_store.get("recipient", true);
    assert_eq!(received[0].id, retained.id);
    let after_clear = retained_store.outbound_for_sender("sender");
    assert_eq!(after_clear[0].report.id, retained.id);
    assert_eq!(
        after_clear[0].report.sender_position,
        retained.sender_position
    );
    assert_eq!(after_clear[0].report.kind, retained.kind);

    retained_store.remove("sender");
    assert!(retained_store.outbound_for_sender("sender").is_empty());

    let mut target_store = ReportStore::new(2);
    target_store.add("closed-target", "live-sender", "Sender", "notice");
    target_store.remove("closed-target");
    assert!(target_store.outbound_for_sender("live-sender").is_empty());
}
