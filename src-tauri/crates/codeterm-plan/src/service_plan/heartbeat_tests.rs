use super::task_attempts::TaskCompeteRequest;
use super::{NewTask, PlanStore};
use crate::plan::{AgentRef, OrchestratorRef, PlanMode, TaskStatus};

pub(super) fn store_with_plan(owner: &str) -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Heartbeat".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab(owner),
            vec![],
            10,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 11).unwrap();
    (dir, store, plan.id)
}

#[test]
fn heartbeat_learning_persists_reloads_and_dedups() {
    let (dir, store, plan_id) = store_with_plan("orch-a");
    let first = store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "Always verify delayed reseed text at send time.",
            Some("reseed"),
            20,
        )
        .unwrap();
    assert!(!first.deduped);

    let duplicate = store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "always verify delayed reseed text at send time",
            Some("reseed"),
            30,
        )
        .unwrap();
    assert!(duplicate.deduped);
    assert_eq!(duplicate.learning.id, first.learning.id);

    store.plans.write().clear();
    store.reload_from_disk(&plan_id).unwrap();
    assert_eq!(store.get(&plan_id).unwrap().heartbeat.learnings.len(), 1);

    let reloaded = PlanStore::new_for_test(dir.path());
    reloaded.scan_project(dir.path()).unwrap();
    let shown = reloaded.show_heartbeat(Some(&plan_id), None, None).unwrap();
    assert_eq!(shown.heartbeat.learnings[0].text, first.learning.text);
    assert!(shown.reinjection.contains(&first.learning.text));
}

#[test]
fn heartbeat_keeps_pending_group_records_advisory_without_reflection_block() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    for (index, name) in ["one", "two"].into_iter().enumerate() {
        let task_id = store
            .add_task(
                &plan_id,
                NewTask {
                    name: name.into(),
                    kind: "implement".into(),
                    group: Some("phase-1".into()),
                    ..Default::default()
                },
                20 + index as u64,
            )
            .unwrap();
        store.start_task_for_test(&plan_id, &task_id, 30 + index as u64);
        store
            .close_task(
                &plan_id,
                &task_id,
                None,
                AgentRef::from_tab("orch-a"),
                40 + index as u64,
            )
            .unwrap();
    }

    let shown = store.show_heartbeat(Some(&plan_id), None, None).unwrap();

    assert!(shown.reinjection.contains("phase-1"));
    assert!(!shown
        .reinjection
        .contains(super::prompts_plan::REFLECTION_HEARTBEAT_TITLE));
}

#[test]
fn heartbeat_lists_only_skills_referenced_by_actionable_work() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let slug = "opaque-skill-token";
    let created = super::plan_skills::create_plan_skill(
        &store,
        &plan_id,
        super::plan_skills::NewPlanSkill {
            slug: slug.into(),
            title: None,
            area: Some("testing".into()),
            check: None,
            rationale: Some("fixture".into()),
        },
    )
    .unwrap();
    std::fs::write(created.skill_md, "---\ndescription: opaque purpose\n---\n").unwrap();
    assert!(!store
        .show_heartbeat(Some(&plan_id), None, None)
        .unwrap()
        .reinjection
        .contains(slug));
    let id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "fixture".into(),
                kind: "implement".into(),
                body: format!("Use `{slug}`"),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    assert!(store
        .prepend_heartbeat_for_plan(&plan_id, "")
        .contains(slug));
    store
        .with_plan(&plan_id, 21, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == id)
                .unwrap()
                .status = TaskStatus::InProgress;
            Ok(())
        })
        .unwrap();
    assert!(!store
        .prepend_heartbeat_for_plan(&plan_id, "")
        .contains(slug));
}

#[test]
fn heartbeat_renders_live_competitive_attempt_summary() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "competitive lane".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    // Sequential assignment is a replacement and closes the outgoing attempt;
    // only the compete path leaves two attempts open at once.
    let drafts = store
        .build_task_compete_drafts(&plan_id, &task_id, TaskCompeteRequest::default())
        .unwrap();
    store
        .record_task_compete_attempt(
            &plan_id,
            &task_id,
            "worker-a",
            &drafts[0],
            Some("worker"),
            21,
        )
        .unwrap();
    store
        .record_task_compete_attempt(
            &plan_id,
            &task_id,
            "worker-b",
            &drafts[1],
            Some("worker"),
            22,
        )
        .unwrap();

    assert!(!store
        .show_heartbeat(Some(&plan_id), None, None)
        .unwrap()
        .reinjection
        .contains(super::prompts_plan::HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE));
    store
        .with_plan(&plan_id, 23, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap()
                .status = TaskStatus::InReview;
            Ok(())
        })
        .unwrap();
    let shown = store.show_heartbeat(Some(&plan_id), None, None).unwrap();
    assert!(shown
        .reinjection
        .contains(super::prompts_plan::HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE));
    assert!(shown
        .reinjection
        .contains(&super::heartbeat::competitive_attempt_summary_line(
            &task_id,
            "competitive lane",
            2
        )));
}

#[test]
fn heartbeat_resolution_prefers_execution_plan_with_open_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let stale = store
        .create_plan(
            "Stale owner plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-a"),
            vec![],
            10,
        )
        .unwrap();
    store
        .with_plan(&stale.id, 11, |plan| {
            plan.heartbeat.current_focus = Some("old focus".into());
            Ok(())
        })
        .unwrap();
    let live = store
        .create_plan(
            "Live execution plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch-a"),
            vec![],
            20,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &live.id,
            NewTask {
                name: "open work".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            21,
        )
        .unwrap();
    store.set_mode(&live.id, PlanMode::Execution, 22).unwrap();
    store
        .with_plan(&live.id, 23, |plan| {
            plan.heartbeat.current_focus = Some("live focus".into());
            Ok(())
        })
        .unwrap();

    let shown = store.show_heartbeat(None, Some("orch-a"), None).unwrap();
    assert_eq!(shown.plan_id, live.id);
    assert_eq!(shown.heartbeat.current_focus, None);

    let text = store.prepend_heartbeat_for_orchestrator_tab("orch-a", "continue");
    assert!(text.contains(&format!("plan=\"{}\"", live.id)));
    assert!(!text.contains("old focus"));

    store.start_task_for_test(&live.id, &task_id, 23);
    store
        .close_task(&live.id, &task_id, Some("done"), Default::default(), 24)
        .unwrap();
    let shown_after_close = store.show_heartbeat(None, Some("orch-a"), None).unwrap();
    assert_eq!(shown_after_close.plan_id, live.id);
}

#[test]
fn paused_plan_is_not_resolved_or_prepended_for_heartbeat() {
    let (_dir, store, plan_id) = store_with_plan("orch-paused");
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "open work".into(),
                kind: "implement".into(),
                competition: None,
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, "worker", None, 21)
        .unwrap();
    store
        .pause_plan(
            &plan_id,
            "waiting on quota",
            AgentRef::from_tab("orch-paused"),
            22,
        )
        .unwrap();

    assert_eq!(
        store.prepend_heartbeat_for_orchestrator_tab("orch-paused", "continue"),
        "continue"
    );
    assert_eq!(
        store.prepend_heartbeat_for_plan(&plan_id, "continue"),
        "continue"
    );
    assert!(store
        .resolve_active_plan_for_orchestrator(None, Some("orch-paused"), None)
        .is_err());
}

#[test]
fn heartbeat_resolves_for_owner_tab_at_send_time() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let payload = "opaque-trigger";
    let first = store.prepend_heartbeat_for_orchestrator_tab("orch-a", payload);
    assert!(first.contains(super::heartbeat::HEARTBEAT_BLOCK_OPEN));
    assert!(!first.contains(super::heartbeat::STATE_RECONCILIATION_PROMPT));
    let fragment = "opaque-learning-token";
    store
        .add_heartbeat_learning(Some(&plan_id), None, None, fragment, None, 20)
        .unwrap();
    let after = store.prepend_heartbeat_for_orchestrator_tab("orch-a", payload);
    assert_eq!(after.matches(fragment).count(), 1);
    assert!(after.ends_with(payload));
    assert_eq!(
        store.prepend_heartbeat_for_orchestrator_tab("other", payload),
        payload
    );
}

#[test]
fn notifier_and_owner_reminder_share_compact_delivery() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    for payload in ["opaque-notifier", "opaque-reminder"] {
        let text = store.prepend_heartbeat_for_plan(&plan_id, payload);
        assert!(text.contains(super::heartbeat::HEARTBEAT_BLOCK_OPEN));
        assert!(text.contains(super::heartbeat::HEARTBEAT_MORE));
        assert!(!text.contains(super::heartbeat::STATE_RECONCILIATION_PROMPT));
        assert_eq!(text.matches(payload).count(), 1);
        assert!(text.ends_with(payload));
    }
}

#[test]
fn refreshed_context_gets_reconciliation_once_at_snapshot_boundary() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let plan = store.get(&plan_id).unwrap();
    let payload = "opaque-refresh-trigger";
    let text =
        super::heartbeat::prepend_heartbeat_snapshot(&plan_id, &plan.heartbeat, &[], payload);
    assert!(text.contains(super::heartbeat::STATE_RECONCILIATION_PROMPT));
    assert!(!text.contains("{plan_id}"));
    assert!(!text.contains("{status}"));
    assert!(text.ends_with(payload));
    assert!(!store
        .prepend_heartbeat_for_plan(&plan_id, payload)
        .contains(super::heartbeat::STATE_RECONCILIATION_PROMPT));
}

#[test]
fn all_tasks_done_plan_produces_no_reconciliation_prepend() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let task_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "the only task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store.start_task_for_test(&plan_id, &task_id, 29);
    store
        .close_task(
            &plan_id,
            &task_id,
            Some("done"),
            AgentRef::from_tab("orch-a"),
            30,
        )
        .unwrap();

    let nudge_body = "Plan reminder: this tab owns unfinished execution-plan work.";
    let result = store.prepend_heartbeat_for_plan(&plan_id, nudge_body);
    assert_eq!(
        result, nudge_body,
        "complete plan must not inject reconciliation"
    );
    assert!(!result.contains(super::heartbeat::STATE_RECONCILIATION_PROMPT));

    let tab_result = store.prepend_heartbeat_for_orchestrator_tab("orch-a", nudge_body);
    assert_eq!(
        tab_result, nudge_body,
        "complete plan must be silent via orchestrator tab path"
    );
}

#[test]
fn explicit_complete_mode_plan_produces_no_reconciliation_prepend() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    store.set_mode(&plan_id, PlanMode::Complete, 20).unwrap();

    let nudge_body = "some message";
    let result = store.prepend_heartbeat_for_plan(&plan_id, nudge_body);
    assert_eq!(result, nudge_body);
    assert!(!result.contains(super::heartbeat::STATE_RECONCILIATION_PROMPT));
}

#[test]
fn open_work_plan_still_gets_compact_heartbeat() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    store
        .add_task(
            &plan_id,
            NewTask {
                name: "open task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();

    let result = store.prepend_heartbeat_for_plan(&plan_id, "text");
    assert!(
        result.contains(super::heartbeat::HEARTBEAT_BLOCK_OPEN),
        "open work must not be silenced"
    );
}

#[test]
fn is_plan_complete_predicate_covers_execution_all_done_and_explicit_complete() {
    use super::heartbeat::is_plan_complete;

    // Empty plan in Execution — not "complete" (no tasks done yet, just empty)
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "T".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 2).unwrap();
    let empty_plan = store.get(&plan.id).unwrap();
    assert!(
        !is_plan_complete(&empty_plan),
        "empty Execution plan is not complete"
    );

    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "t".into(),
                kind: "impl".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store.start_task_for_test(&plan.id, &task_id, 3);
    store
        .close_task(&plan.id, &task_id, None, AgentRef::from_tab("orch"), 4)
        .unwrap();
    let done_plan = store.get(&plan.id).unwrap();
    assert!(
        is_plan_complete(&done_plan),
        "all-done Execution plan is complete"
    );

    store.set_mode(&plan.id, PlanMode::Complete, 5).unwrap();
    let complete_plan = store.get(&plan.id).unwrap();
    assert!(
        is_plan_complete(&complete_plan),
        "Complete mode with terminal tasks is complete"
    );
}

#[test]
fn is_plan_complete_rejects_paused_and_blocked_tasks() {
    use super::heartbeat::is_plan_complete;

    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Deferred".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap();
    store.set_mode(&plan.id, PlanMode::Execution, 2).unwrap();
    let paused_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "paused".into(),
                kind: "impl".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == paused_id)
                .unwrap();
            task.status = TaskStatus::Paused;
            task.pause_reason = Some("waiting for quota reset".into());
            Ok(())
        })
        .unwrap();

    let paused_plan = store.get(&plan.id).unwrap();
    assert!(
        !is_plan_complete(&paused_plan),
        "paused work is deferred, not complete"
    );

    let blocked_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "blocked".into(),
                kind: "impl".into(),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    store
        .update_task(
            &plan.id,
            &blocked_id,
            None,
            None,
            None,
            None,
            Some("blocked"),
            Some(Some("waiting on external input")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            6,
        )
        .unwrap();

    let blocked_plan = store.get(&plan.id).unwrap();
    assert!(
        !is_plan_complete(&blocked_plan),
        "blocked work is waiting, not complete"
    );
}

#[test]
fn heartbeat_renders_digest_lines_without_other_state() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let digest_lines = vec![
        "5 reports await your verdict, oldest 2h — judge".into(),
        "claude: 62% weekly".into(),
    ];
    let text = store.prepend_heartbeat_for_plan_with_digests(&plan_id, "continue", &digest_lines);
    for fragment in &digest_lines {
        assert_eq!(text.matches(fragment.as_str()).count(), 1);
    }
}

/// codeterm#397: `plan next`, orchestrator status, and the heartbeat must
/// report the SAME outstanding-work count for the exact shape the report
/// named as under-counted — one task in_progress gating six pending tasks,
/// plus an ordinary explicit blocked task and an in_review task thrown in to
/// prove those statuses are not mistaken for gating. A fixture change (the
/// gate clearing) must move all three together, proving one derivation
/// rather than three that happen to agree today.
#[test]
fn outstanding_work_is_one_shared_derivation_across_next_status_and_heartbeat() {
    let (_dir, store, plan_id) = store_with_plan("orch-audit");

    let audit_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "audit".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            20,
        )
        .unwrap();
    store
        .with_plan(&plan_id, 21, |plan| {
            plan.tasks
                .iter_mut()
                .find(|t| t.id == audit_id)
                .unwrap()
                .status = TaskStatus::InProgress;
            Ok(())
        })
        .unwrap();

    for i in 0u64..6 {
        store
            .add_task(
                &plan_id,
                NewTask {
                    name: format!("gated-{i}"),
                    kind: "implement".into(),
                    depends_on: vec![audit_id.clone()],
                    ..Default::default()
                },
                22 + i,
            )
            .unwrap();
    }

    let review_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "in review".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            40,
        )
        .unwrap();
    store
        .with_plan(&plan_id, 41, |plan| {
            plan.tasks
                .iter_mut()
                .find(|t| t.id == review_id)
                .unwrap()
                .status = TaskStatus::InReview;
            Ok(())
        })
        .unwrap();

    let blocked_id = store
        .add_task(
            &plan_id,
            NewTask {
                name: "blocked".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            42,
        )
        .unwrap();
    store
        .with_plan(&plan_id, 43, |plan| {
            plan.tasks
                .iter_mut()
                .find(|t| t.id == blocked_id)
                .unwrap()
                .status = TaskStatus::Blocked;
            Ok(())
        })
        .unwrap();

    fn assert_outstanding(store: &PlanStore, plan_id: &str, expected: usize) {
        let plan = store.get(plan_id).unwrap();
        // The heartbeat's own derivation, called directly as heartbeat.rs
        // does — not re-derived here.
        let heartbeat_count = super::state::outstanding_blocked_count(&plan);
        assert_eq!(heartbeat_count, expected);

        // The orchestrator status / plan next projection.
        let coverage = crate::plan::coverage::compute_coverage_report(&plan);
        let snapshot = super::state::build_plan_state_snapshot(
            &plan,
            coverage,
            vec![],
            vec![],
            vec![],
            &std::collections::HashSet::new(),
            None,
            50,
        );
        assert_eq!(snapshot.blocked_or_gated_count, expected);
        assert_eq!(
            snapshot.blocked_or_gated_count, heartbeat_count,
            "orchestrator status and heartbeat must agree without re-deriving"
        );

        // The heartbeat RPC surface (`plan.heartbeat.show`), structured, not
        // rendered text.
        let shown = store.show_heartbeat(Some(plan_id), None, None).unwrap();
        assert_eq!(shown.blocked_or_gated_count, expected);
    }

    // 1 explicit blocked + 6 dependency-gated pending; the in_review task and
    // the in_progress audit task are neither.
    assert_outstanding(&store, &plan_id, 7);

    let plan = store.get(&plan_id).unwrap();
    let coverage = crate::plan::coverage::compute_coverage_report(&plan);
    let snapshot = super::state::build_plan_state_snapshot(
        &plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &std::collections::HashSet::new(),
        None,
        50,
    );
    let waiting = snapshot
        .next_actions
        .iter()
        .filter(|action| action.kind == super::next_actions::NextActionKind::WaitingOnDependency)
        .count();
    assert_eq!(waiting, 6, "plan next must name all six gated lanes");

    // Fixture change: clear the gate. All three surfaces must move together
    // down to just the one explicit blocked task.
    store
        .close_task(
            &plan_id,
            &audit_id,
            Some("done"),
            AgentRef::from_tab("orch-audit"),
            60,
        )
        .unwrap();
    assert_outstanding(&store, &plan_id, 1);
}

#[test]
fn heartbeat_gate_selected_learnings_render_at_most_three_and_fallback_preserves_baseline() {
    let (_dir, store, plan_id) = store_with_plan("owner");
    let fragments: Vec<_> = (0..5)
        .map(|index| format!("opaque learning token {index}"))
        .collect();
    let ids: Vec<_> = fragments
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            store
                .add_heartbeat_learning(
                    Some(&plan_id),
                    None,
                    None,
                    fragment,
                    None,
                    20 + index as u64,
                )
                .unwrap()
                .learning
                .id
        })
        .collect();
    let payload = "opaque delivered payload";
    let digests = vec!["opaque digest".into()];
    let selected =
        store.prepend_heartbeat_with_selected_learnings(&plan_id, payload, &digests, Some(&ids));
    for (index, fragment) in fragments.iter().enumerate() {
        assert_eq!(selected.contains(fragment), index < 3);
    }
    assert!(selected.contains(payload));
    let baseline = store.prepend_heartbeat_for_plan_with_digests(&plan_id, payload, &digests);
    assert_eq!(
        store.prepend_heartbeat_with_selected_learnings(&plan_id, payload, &digests, None),
        baseline
    );
    for (index, fragment) in fragments.iter().enumerate() {
        assert_eq!(baseline.contains(fragment), index >= 2);
    }
    assert_eq!(store.get(&plan_id).unwrap().heartbeat.learnings.len(), 5);
}
