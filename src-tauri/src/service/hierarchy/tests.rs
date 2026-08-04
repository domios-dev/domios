use super::{
    is_explicit_plan_foreign, ledger_with_live_children, milestone_text, planner_rehydrate_payload,
    planner_request_payload, planner_specs_for_plan, report_task_status_transition,
    resolve_plan_ctx, resolve_report_target, worker_death_report, ReportTarget,
};
use crate::agent::ledger::LedgerRow;
use crate::agent::reports::{ReportStatus, ReportStore};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{
    Goal, Milestone, OrchestratorRef, Plan, PlannerRequest, PlannerRequestState, Spec,
};
use crate::service::hierarchy::team_plan_creation::create_team_plan_for_orchestrator;
use crate::service::plan::{NewTask, PlanError, PlanStore};
use crate::service::TeamPlanCreationError;
use crate::{lock_hold::TimedRwLock, multiplexer::commands::MuxState};
use std::path::Path;
use std::sync::{Arc, Barrier};

// Enforce report key symmetry: reporter writes and parent query must use the same key.
// This avoids "report succeeded" with empty parent results.

fn build_orchestrator_with_worker(worker_title: &str) -> (MultiplexerState, String, String) {
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orch))
        .expect("attach worker to orchestrator");
    mux.rename_tab(&worker, worker_title)
        .expect("rename worker Tab");
    (mux, orch, worker)
}

fn build_orchestrator_with_manager(manager_title: &str) -> (MultiplexerState, String, String) {
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let manager = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(&manager, AgentRole::Manager, Some("planner"), Some(&orch))
        .expect("attach manager to orchestrator");
    mux.rename_tab(&manager, manager_title)
        .expect("rename manager Tab");
    (mux, orch, manager)
}

#[test]
fn worker_report_is_listed_by_orchestrator() {
    let (mux, orch, worker) = build_orchestrator_with_worker("Worker-1");
    let mut store = ReportStore::new(100);

    let (from_title, target) = resolve_report_target(&mux, &worker).expect("resolve target");
    let target = target.expect("worker has a parent target");
    assert_eq!(
        target,
        ReportTarget::Local(orch.clone()),
        "worker reports to its direct parent"
    );
    assert_eq!(from_title, "Worker-1");

    store.add(&target.storage_key(), &worker, &from_title, "task done");

    let listed = store.get(&orch, false);
    assert_eq!(
        listed.len(),
        1,
        "orchestrator must see the worker's report after submit"
    );
    assert_eq!(listed[0].from_tab_id, worker);
    assert_eq!(listed[0].from_title, "Worker-1");
    assert_eq!(listed[0].message, "task done");
}

#[test]
fn worker_report_keys_match_storage_and_query() {
    let (mux, orch, worker) = build_orchestrator_with_worker("Worker-A");
    let mut store = ReportStore::new(100);

    let (from_title, target) = resolve_report_target(&mux, &worker).expect("resolve target");
    let storage_key = target.expect("worker has a parent target").storage_key();
    store.add(&storage_key, &worker, &from_title, "msg");

    let query_key = &orch;
    assert_eq!(
        &storage_key, query_key,
        "submit_report write key and report target key must agree"
    );
    assert_eq!(store.get(query_key, false).len(), 1);
}

#[test]
fn ledger_includes_live_child_before_first_report() {
    let (mut mux, orch, worker) = build_orchestrator_with_worker("ledger-smoke");
    mux.get_tab_mut(&worker).unwrap().agent_session_model = Some("gpt-5.5".into());

    let rows = ledger_with_live_children(&mux, &orch, vec![], 123);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].orchestrator_tab_id, orch);
    assert_eq!(rows[0].worker_tab_id, worker);
    assert_eq!(rows[0].title, "ledger-smoke");
    assert_eq!(rows[0].model.as_deref(), Some("gpt-5.5"));
    assert_eq!(rows[0].status, None);
    assert_eq!(rows[0].message, "");
    assert_eq!(rows[0].updated_at, 123);
}

#[test]
fn ledger_refreshes_existing_live_child_without_losing_report_details() {
    let (mut mux, orch, worker) = build_orchestrator_with_worker("fresh-title");
    mux.get_tab_mut(&worker).unwrap().agent_session_model = Some("sonnet".into());
    let rows = vec![LedgerRow {
        orchestrator_tab_id: orch.clone(),
        worker_tab_id: worker.clone(),
        title: "stale-title".into(),
        plan_id: Some("pl_1".into()),
        task_id: Some("task_1".into()),
        branch: Some("lane/agent-ledger".into()),
        model: Some("old-model".into()),
        status: Some(ReportStatus::Partial),
        message: "still working".into(),
        sha: Some("abc123".into()),
        changed_files: Some(vec!["src-tauri/src/service/hierarchy.rs".into()]),
        test_cmd: Some("cargo test hierarchy".into()),
        test_result: Some("ok".into()),
        updated_at: 10,
        ..Default::default()
    }];

    let rows = ledger_with_live_children(&mux, &orch, rows, 99);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "fresh-title");
    assert_eq!(rows[0].model.as_deref(), Some("sonnet"));
    assert_eq!(rows[0].branch.as_deref(), Some("lane/agent-ledger"));
    assert_eq!(rows[0].status, Some(ReportStatus::Partial));
    assert_eq!(rows[0].message, "still working");
    assert_eq!(rows[0].updated_at, 99);
}

#[test]
fn no_plan_routing_keeps_parent_priority() {
    let (mux, orch, worker) = build_orchestrator_with_worker("Worker-A");
    let (_from_title, target) = resolve_report_target(&mux, &worker).expect("resolve target");
    assert_eq!(target, Some(ReportTarget::Local(orch.clone())));
}

#[test]
fn report_title_uses_user_pinned_tab_identity_not_provider_default() {
    let (mut mux, orch) = MultiplexerState::new();
    let ws = mux
        .find_workspace_for_tab(&orch)
        .expect("orchestrator lives in a workspace");
    let worker = mux.create_tab(&ws).expect("create worker tab");
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orch))
        .expect("attach worker");
    mux.rename_tab(&worker, "Euler").expect("set Tab identity");
    mux.rename_tab_at_level(
        &worker,
        "claude",
        crate::multiplexer::types::TitleLevel::Agent,
    )
    .expect("agent title ignored below user pin");

    let (from_title, target) = resolve_report_target(&mux, &worker).expect("resolve");
    assert_eq!(target, Some(ReportTarget::Local(orch.clone())));
    assert_eq!(
        from_title, "Euler",
        "report must carry the tab identity, not the provider default"
    );
}

// ── resolve_plan_ctx tests ──────────────────────────────────────────────────

fn make_store_with_assigned_task(tab_id: &str) -> (PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut plan = Plan::new(
        "Test".into(),
        project.display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "my task".into(),
                kind: "implement".into(),
                body: "do stuff".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, tab_id, None, 3)
        .unwrap();
    (store, plan.id, task_id)
}

#[test]
fn resolve_plan_ctx_uses_provided_valid_plan() {
    let (store, plan_id, task_id) = make_store_with_assigned_task("tab-x");
    let mut notices = Vec::new();
    let (p, t) = resolve_plan_ctx(
        &store,
        "tab-x",
        Some(&plan_id),
        Some(&task_id),
        &mut notices,
    )
    .unwrap();
    assert_eq!(p.as_deref(), Some(plan_id.as_str()));
    assert_eq!(t.as_deref(), Some(task_id.as_str()));
    assert!(notices.is_empty());
}

#[test]
fn resolve_plan_ctx_stale_plan_falls_through_to_binding() {
    let (store, _plan_id, task_id) = make_store_with_assigned_task("tab-y");
    let mut notices = Vec::new();
    let (p, t) = resolve_plan_ctx(&store, "tab-y", Some("pl_stale"), None, &mut notices).unwrap();
    assert!(p.is_some(), "should auto-resolve to bound plan");
    assert!(t.is_some(), "should auto-resolve to bound task");
    assert!(
        !notices.is_empty(),
        "must emit a notice about the stale plan"
    );
    assert!(notices[0].contains("pl_stale"));
    assert_eq!(t.as_deref(), Some(task_id.as_str()));
}

#[test]
fn resolve_plan_ctx_no_plan_hint_uses_binding() {
    let (store, _plan_id, task_id) = make_store_with_assigned_task("tab-z");
    let mut notices = Vec::new();
    let (p, t) = resolve_plan_ctx(&store, "tab-z", None, None, &mut notices).unwrap();
    assert!(p.is_some());
    assert_eq!(t.as_deref(), Some(task_id.as_str()));
    assert!(notices.is_empty(), "no notice when silently auto-resolving");
}

#[test]
fn resolve_plan_ctx_unassigned_tab_returns_none() {
    let (store, _plan_id, _task_id) = make_store_with_assigned_task("tab-a");
    let mut notices = Vec::new();
    let (p, t) = resolve_plan_ctx(&store, "tab-not-assigned", None, None, &mut notices).unwrap();
    assert!(p.is_none());
    assert!(t.is_none());
    assert!(notices.is_empty());
}

#[test]
fn resolve_plan_ctx_ambiguous_assignment_errors_with_list() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut plan = Plan::new(
        "Multi".into(),
        project.display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let t1 = store
        .add_task(
            &plan.id,
            NewTask {
                name: "task alpha".into(),
                kind: "implement".into(),
                body: "".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let t2 = store
        .add_task(
            &plan.id,
            NewTask {
                name: "task beta".into(),
                kind: "implement".into(),
                body: "".into(),
                competition: None,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &t1, "tab-multi", None, 3)
        .unwrap();
    store
        .assign_task(&plan.id, &t2, "tab-multi", None, 3)
        .unwrap();

    let mut notices = Vec::new();
    let err = resolve_plan_ctx(&store, "tab-multi", None, None, &mut notices).unwrap_err();
    assert!(
        err.contains("ambiguous"),
        "error must say 'ambiguous': {err}"
    );
    assert!(err.contains("task alpha") || err.contains(t1.as_str()));
    assert!(err.contains("task beta") || err.contains(t2.as_str()));
}

#[test]
fn milestone_text_prefixes_structured_status_only() {
    assert_eq!(milestone_text("shipped", None), "shipped");
    assert_eq!(
        milestone_text("shipped", Some(ReportStatus::Done)),
        "DONE: shipped"
    );
    assert_eq!(
        milestone_text("needs help", Some(ReportStatus::Blocked)),
        "BLOCKED: needs help"
    );
    assert_eq!(
        milestone_text("ready", Some(ReportStatus::InReview)),
        "IN_REVIEW: ready"
    );
}

#[test]
fn planner_rehydrate_preserves_all_36_tasks_and_strides() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Windows recovery".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    for index in 0..36 {
        store
            .add_task(
                &plan.id,
                NewTask {
                    name: format!("task {index}"),
                    kind: "implement".into(),
                    body: format!("body {index}"),
                    competition: None,
                    ..Default::default()
                },
                index + 2,
            )
            .unwrap();
    }
    let mut hydrated = store.get(&plan.id).unwrap();
    hydrated.goal = Some(Goal {
        vector: "recover".into(),
        ..Default::default()
    });
    let task_stride = Milestone {
        id: "task-stride".into(),
        from: OrchestratorRef::from_tab("worker"),
        kind: "status_update".into(),
        text: "task evidence".into(),
        ts: 10,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    };
    hydrated.tasks[0].milestones.push(task_stride);
    hydrated.strides.push(Milestone {
        id: "plan-stride".into(),
        from: OrchestratorRef::from_tab("owner"),
        kind: "decision".into(),
        text: "plan evidence".into(),
        ts: 11,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    });
    let mut spec = Spec::new(
        "Authoritative spec".into(),
        dir.path().display().to_string(),
    );
    spec.plan_id = Some(plan.id.clone());
    store.save_spec(&mut spec, 12).unwrap();
    let mut linked_spec = Spec::new("Linked spec".into(), dir.path().display().to_string());
    store.save_spec(&mut linked_spec, 13).unwrap();
    hydrated.primary_spec_id = Some(spec.id.clone());
    hydrated.spec_refs = vec![spec.id.clone(), linked_spec.id.clone()];

    let payload = planner_rehydrate_payload(&hydrated, planner_specs_for_plan(&store, &hydrated));
    let snapshot: serde_json::Value =
        serde_json::from_str(payload.split_once('\n').unwrap().1).unwrap();
    let tasks = snapshot["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 36);
    assert_eq!(
        tasks
            .iter()
            .filter_map(|task| task["id"].as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        36
    );
    assert_eq!(tasks[0]["milestones"][0]["id"], "task-stride");
    assert_eq!(snapshot["strides"][0]["id"], "plan-stride");
    assert_eq!(snapshot["goal"]["vector"], "recover");
    assert_eq!(snapshot["specs"].as_array().unwrap().len(), 2);
    let spec_titles = snapshot["specs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|spec| spec["title"].as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(spec_titles, ["Authoritative spec", "Linked spec"].into());
}

#[test]
fn planner_request_envelope_preserves_shell_metacharacters_as_chat_data() {
    let payload = "- audit `model`\n- <nonexistent>\n$env:BAD='x'; echo $(whoami)\n$(false) && :";
    let request = PlannerRequest {
        request_id: "req-shell".into(),
        payload: payload.into(),
        payload_blob: None,
        caller_tab_id: Some("caller".into()),
        caller_session_id: Some("caller-session".into()),
        state: PlannerRequestState::Accepted,
        created_at: 1,
        accepted_at: Some(2),
        lease_expires_at: None,
        delivery_attempts: 1,
        completed_at: None,
        failed_at: None,
        delivered_to_tab_id: Some("planner".into()),
        delivered_to_session_id: Some("planner-session".into()),
        delivered_with_binding_nonce: Some("planner-nonce".into()),
    };

    let envelope = planner_request_payload("pl-shell", &request);
    assert!(envelope.contains(payload));
    assert!(envelope.contains("ack-request pl-shell req-shell"));
}

#[test]
fn report_status_policy_maps_worker_outcomes_to_task_transitions() {
    assert_eq!(report_task_status_transition(None).unwrap(), None);
    assert_eq!(
        report_task_status_transition(Some(ReportStatus::Partial)).unwrap(),
        None
    );
    assert_eq!(
        report_task_status_transition(Some(ReportStatus::InReview)).unwrap(),
        Some("in_review")
    );
    assert_eq!(
        report_task_status_transition(Some(ReportStatus::Blocked)).unwrap(),
        Some("blocked")
    );
}

#[test]
fn report_status_policy_rejects_worker_done() {
    let err = report_task_status_transition(Some(ReportStatus::Done)).unwrap_err();
    assert!(err.contains("must not report --status done"), "got: {err}");
    assert!(err.contains("--status in_review"), "got: {err}");
}

#[test]
fn cron_report_target_is_used_when_no_parent() {
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let cron_tab = mux.create_tab(&workspace_id).unwrap();
    // Cron-spawned tabs carry an explicit report_target_tab_id but no role.
    if let Some(tab) = mux.get_tab_mut(&cron_tab) {
        tab.report_target_tab_id = Some(orch.clone());
    }
    mux.rename_tab(&cron_tab, "cron-task").unwrap();

    let (from_title, target) = resolve_report_target(&mux, &cron_tab).unwrap();
    assert_eq!(target, Some(ReportTarget::Local(orch.clone())));
    assert_eq!(from_title, "cron-task");
}

#[test]
fn parent_tab_id_takes_priority_over_other_targets() {
    let (mut mux, orch, worker) = build_orchestrator_with_worker("Worker-Pri");
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let other = mux.create_tab(&workspace_id).unwrap();
    let spawner = mux.create_tab(&workspace_id).unwrap();
    if let Some(tab) = mux.get_tab_mut(&worker) {
        tab.report_target_tab_id = Some(other.clone());
        tab.spawned_by_tab_id = Some(spawner.clone());
    }

    let (_, target) = resolve_report_target(&mux, &worker).unwrap();
    assert_eq!(
        target,
        Some(ReportTarget::Local(orch.clone())),
        "explicit parent_tab_id wins over report_target and spawned_by"
    );
}

#[test]
fn orchestrator_falls_back_to_spawned_by_when_no_parent() {
    let (mut mux, orch) = MultiplexerState::new();
    // Simulate the spawning agent — it lives in its own tab elsewhere.
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let spawner = mux.create_tab(&workspace_id).unwrap();
    // Top-level orchestrator has no parent and no report_target, but `agent spawn` recorded who asked for it.
    if let Some(tab) = mux.get_tab_mut(&orch) {
        tab.spawned_by_tab_id = Some(spawner.clone());
    }

    let (_, target) = resolve_report_target(&mux, &orch).unwrap();
    assert_eq!(
        target,
        Some(ReportTarget::Local(spawner.clone())),
        "top-level orchestrator routes its report to whoever spawned it"
    );
}

#[test]
fn never_routes_back_to_self_when_all_targets_unset() {
    // The reporter's own id would inject the report back into its PTY instead of its parent.
    let (mux, orch) = MultiplexerState::new();
    let (_, target) = resolve_report_target(&mux, &orch).unwrap();
    assert!(
        target.is_none(),
        "orphan orchestrator must NOT resolve to itself; got {:?}",
        target
    );
}

#[test]
fn never_routes_back_to_self_even_when_targets_point_at_self() {
    // Defensive: if any field accidentally points at the reporter itself, that field is skipped — reports should never bounce.
    let (mut mux, orch) = MultiplexerState::new();
    if let Some(tab) = mux.get_tab_mut(&orch) {
        tab.parent_tab_id = Some(orch.clone());
        tab.report_target_tab_id = Some(orch.clone());
        tab.spawned_by_tab_id = Some(orch.clone());
    }
    let (_, target) = resolve_report_target(&mux, &orch).unwrap();
    assert!(
        target.is_none(),
        "self-referential targets must be filtered out; got {:?}",
        target
    );
}

#[test]
fn spawned_by_used_only_when_parent_and_report_target_missing() {
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let spawner = mux.create_tab(&workspace_id).unwrap();
    let report_target = mux.create_tab(&workspace_id).unwrap();
    if let Some(tab) = mux.get_tab_mut(&orch) {
        tab.report_target_tab_id = Some(report_target.clone());
        tab.spawned_by_tab_id = Some(spawner.clone());
    }
    let (_, target) = resolve_report_target(&mux, &orch).unwrap();
    assert_eq!(
        target,
        Some(ReportTarget::Local(report_target.clone())),
        "report_target_tab_id wins over spawned_by_tab_id"
    );
}

#[test]
fn unknown_tab_id_is_rejected() {
    let (mux, _orch) = MultiplexerState::new();
    let err = resolve_report_target(&mux, "nonexistent").unwrap_err();
    assert!(err.contains("Tab not found"), "got: {}", err);
}

// ── Worker-death watchdog ──

#[test]
fn dead_worker_without_report_routes_synthetic_to_parent() {
    let (mux, orch, worker) = build_orchestrator_with_worker("Worker-Dead");
    let store = ReportStore::new(100);

    let (target, msg) = worker_death_report(&mux, &store, &worker)
        .expect("dead unreported worker must synthesize a report");
    assert_eq!(target, orch, "synthetic dead-report routes to the parent");
    assert_eq!(
        msg,
        format!("WORKER {} EXITED without reporting (dead)", worker)
    );
}

#[test]
fn dead_manager_without_report_uses_manager_label() {
    let (mux, orch, manager) = build_orchestrator_with_manager("Manager-Dead");
    let store = ReportStore::new(100);

    let (target, msg) = worker_death_report(&mux, &store, &manager)
        .expect("dead unreported manager must synthesize a report");
    assert_eq!(target, orch, "synthetic dead-report routes to the parent");
    assert_eq!(
        msg,
        format!("MANAGER {} EXITED without reporting (dead)", manager)
    );
}

#[test]
fn dead_worker_that_already_reported_gets_no_synthetic() {
    let (mux, orch, worker) = build_orchestrator_with_worker("Worker-Reported");
    let mut store = ReportStore::new(100);
    // Worker already spoke to its parent before dying.
    store.add(&orch, &worker, "Worker-Reported", "task done");

    assert!(
        worker_death_report(&mux, &store, &worker).is_none(),
        "a worker that already reported must NOT get a synthetic dead-report"
    );
}

#[test]
fn dead_non_hierarchy_tab_gets_no_synthetic() {
    // Plain tab: no parent/report_target/spawner → no routing target → no synthetic report.
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let plain = mux.create_tab(&workspace_id).unwrap();
    let store = ReportStore::new(100);

    assert!(
        worker_death_report(&mux, &store, &plain).is_none(),
        "a non-hierarchy Tab must NOT get a synthetic dead-report"
    );
}

#[test]
fn dead_worker_routes_via_spawned_by_when_no_parent() {
    // A top-level tab spawned by another agent still routes its death to whoever spawned it.
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let spawner = mux.create_tab(&workspace_id).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    if let Some(tab) = mux.get_tab_mut(&worker) {
        tab.spawned_by_tab_id = Some(spawner.clone());
    }
    let store = ReportStore::new(100);

    let (target, msg) =
        worker_death_report(&mux, &store, &worker).expect("spawned_by gives a target");
    assert_eq!(target, spawner);
    assert_eq!(
        msg,
        format!("WORKER {} EXITED without reporting (dead)", worker)
    );
}

// ── Report message format ── submit_report builds the notify body without a trailing CR — Enter is sent as a separate delayed raw write by send_text's bracket-paste path.

fn format_report(from_title: &str, message: &str) -> String {
    format!("[Report from {}] {}", from_title, message)
}

#[test]
fn report_format_basic() {
    let msg = format_report("Scout", "Task completed successfully");
    assert_eq!(msg, "[Report from Scout] Task completed successfully");
}

#[test]
fn report_format_no_trailing_cr() {
    // the bracket-paste end marker is processed.
    let msg = format_report("Worker-1", "done");
    assert!(!msg.ends_with('\r'), "Report body must not end with \\r");
}

#[test]
fn report_format_message_with_newlines() {
    // Internal newlines pass through raw — bracket paste protects them.
    let msg = format_report("Worker", "line1\nline2\nline3");
    assert_eq!(msg, "[Report from Worker] line1\nline2\nline3");
    assert!(msg.contains('\n'), "Internal newlines are preserved");
}

#[test]
fn report_format_empty_message() {
    let msg = format_report("Agent", "");
    assert_eq!(msg, "[Report from Agent] ");
}

#[test]
fn report_format_title_with_spaces() {
    let msg = format_report("My Agent Title", "ok");
    assert_eq!(msg, "[Report from My Agent Title] ok");
}

#[test]
fn report_format_message_with_special_chars() {
    let msg = format_report("Bot", "found 3 files: a.rs, b.rs, c.rs");
    assert!(msg.starts_with("[Report from Bot] "));
}

// ── Remote (mesh) report target ──

#[test]
fn mesh_target_resolves_when_peer_and_tab_set() {
    // both fields; it must resolve to a Mesh target, never a local tab.
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    if let Some(tab) = mux.get_tab_mut(&worker) {
        tab.report_target_mesh_peer = Some("Mac-Air".into());
        tab.report_target_tab_id = Some("main".into());
    }
    mux.rename_tab(&worker, "Remote-Worker").unwrap();

    let (from_title, target) = resolve_report_target(&mux, &worker).unwrap();
    assert_eq!(from_title, "Remote-Worker");
    assert_eq!(
        target,
        Some(ReportTarget::Mesh {
            peer: "Mac-Air".into(),
            tab: "main".into()
        })
    );
}

#[test]
fn mesh_target_wins_over_local_parent() {
    // so the report crosses the mesh instead of landing in a stale local tab.
    let (mux, _orch, worker) = build_orchestrator_with_worker("Remote-Pri");
    let mut mux = mux;
    if let Some(tab) = mux.get_tab_mut(&worker) {
        tab.report_target_mesh_peer = Some("eight".into());
        tab.report_target_tab_id = Some("orch-42".into());
    }
    let (_, target) = resolve_report_target(&mux, &worker).unwrap();
    assert_eq!(
        target,
        Some(ReportTarget::Mesh {
            peer: "eight".into(),
            tab: "orch-42".into()
        }),
        "remote target must win over a local parent_tab_id"
    );
}

#[test]
fn mesh_peer_without_target_tab_falls_back_to_local() {
    // half-addressed remote target; fall through to the local chain.
    let (mux, orch, worker) = build_orchestrator_with_worker("Half-Remote");
    let mut mux = mux;
    if let Some(tab) = mux.get_tab_mut(&worker) {
        tab.report_target_mesh_peer = Some("eight".into());
        tab.report_target_tab_id = None;
    }
    let (_, target) = resolve_report_target(&mux, &worker).unwrap();
    assert_eq!(target, Some(ReportTarget::Local(orch)));
}

#[test]
fn storage_keys_distinguish_local_and_mesh() {
    assert_eq!(ReportTarget::Local("p1".into()).storage_key(), "p1");
    assert_eq!(
        ReportTarget::Mesh {
            peer: "Mac-Air".into(),
            tab: "main".into()
        }
        .storage_key(),
        "mesh:Mac-Air/main"
    );
}

// ── Mesh homing: foreign plan detection ──────────────────────────────────────

#[test]
fn resolve_plan_ctx_foreign_plan_with_no_binding_returns_none_with_notice() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut notices = Vec::new();
    let (p, t) = resolve_plan_ctx(
        &store,
        "tab-remote",
        Some("pl_foreign"),
        Some("task_yyy"),
        &mut notices,
    )
    .unwrap();
    assert!(p.is_none(), "foreign plan → eff_plan_id must be None");
    assert!(t.is_none(), "no local binding → eff_task_id must be None");
    assert!(
        !notices.is_empty(),
        "must emit a notice that the plan was not found"
    );
    assert!(
        notices[0].contains("pl_foreign"),
        "notice must name the missing plan"
    );
}

#[test]
fn mesh_peer_extracted_from_default_target_before_or_consumes_it() {
    // homing — this is the detection step that decides whether to relay.
    let default_target: Option<ReportTarget> = Some(ReportTarget::Mesh {
        peer: "eight".into(),
        tab: "orch-99".into(),
    });
    let default_mesh_peer = match &default_target {
        Some(ReportTarget::Mesh { peer, .. }) => Some(peer.clone()),
        _ => None,
    };
    // Simulates the .or(default_target_opt) path in submit_report.
    let _target_opt: Option<ReportTarget> = None::<ReportTarget>.or(default_target);

    assert_eq!(
        default_mesh_peer.as_deref(),
        Some("eight"),
        "mesh peer must be captured before default_target is consumed"
    );
}

#[test]
fn local_default_target_yields_no_mesh_peer_for_homing() {
    // A local worker (no mesh peer) must not trigger homing even if plan not found.
    let default_target: Option<ReportTarget> = Some(ReportTarget::Local("orch-local".into()));
    let default_mesh_peer = match &default_target {
        Some(ReportTarget::Mesh { peer, .. }) => Some(peer.clone()),
        _ => None,
    };
    assert!(
        default_mesh_peer.is_none(),
        "local target must not produce a mesh peer"
    );
}

// ── is_explicit_plan_foreign routing gate ────────────────────────────────────

#[test]
fn foreign_when_explicit_not_in_store() {
    // plan_id provided but resolve_plan_ctx couldn't find it — eff is None.
    assert!(
        is_explicit_plan_foreign(Some("pl_remote"), None),
        "explicit plan not found locally must be foreign"
    );
}

#[test]
fn foreign_when_explicit_overridden_by_local_binding() {
    // resolve_plan_ctx fell through to a local binding — explicit ref must still win.
    assert!(
        is_explicit_plan_foreign(Some("pl_remote"), Some("pl_local_binding")),
        "explicit plan overridden by a different local plan must be foreign"
    );
}

#[test]
fn not_foreign_when_explicit_found_locally() {
    // resolve_plan_ctx matched the explicit plan id in the local store.
    assert!(
        !is_explicit_plan_foreign(Some("pl_local"), Some("pl_local")),
        "explicit plan found locally is NOT foreign"
    );
}

#[test]
fn not_foreign_when_no_explicit_plan() {
    // No --plan flag provided — tab-binding auto-resolution, always local.
    assert!(
        !is_explicit_plan_foreign(None, Some("pl_auto")),
        "no explicit plan means no homing needed"
    );
}

#[test]
fn not_foreign_for_empty_explicit_plan() {
    // Empty/whitespace --plan should be treated as absent.
    assert!(
        !is_explicit_plan_foreign(Some("  "), Some("pl_local")),
        "whitespace-only plan id is not an explicit ref"
    );
    assert!(
        !is_explicit_plan_foreign(Some(""), None),
        "empty plan id is not an explicit ref"
    );
}

#[test]
fn resolve_plan_ctx_explicit_plan_overridden_by_binding_triggers_foreign() {
    // the explicit plan isn't found. is_explicit_plan_foreign must detect this.
    let (store, _local_plan, _local_task) = make_store_with_assigned_task("tab-q");
    let mut notices = Vec::new();
    let (eff, _) =
        resolve_plan_ctx(&store, "tab-q", Some("pl_foreign_x"), None, &mut notices).unwrap();
    // is_explicit_plan_foreign must report true since "pl_foreign_x" != eff.
    assert!(
        is_explicit_plan_foreign(Some("pl_foreign_x"), eff.as_deref()),
        "explicit plan overridden by local binding must be detected as foreign"
    );
}

struct TeamPlanHarness {
    mux: MuxState,
    plan_store: Arc<PlanStore>,
}

impl TeamPlanHarness {
    fn new(root: &Path) -> Self {
        let (state, _) = MultiplexerState::new();
        Self {
            mux: Arc::new(TimedRwLock::new("mux", state)),
            plan_store: Arc::new(PlanStore::new_for_test(root)),
        }
    }
}

fn teamless_orchestrator(mux: &MuxState) -> (String, String) {
    let mut mux = mux.write();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let owner_tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&owner_tab_id)
        .unwrap();
    (workspace_id, owner_tab_id)
}

#[test]
fn first_plan_mints_team_enrolls_manager_and_binds() {
    let dir = tempfile::tempdir().unwrap();
    let harness = TeamPlanHarness::new(dir.path());
    let (_workspace_id, owner_tab_id) = teamless_orchestrator(&harness.mux);

    let outcome = create_team_plan_for_orchestrator(
        &harness.mux,
        &harness.plan_store,
        "First effort".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner_tab_id),
        vec![],
        None,
        1,
    )
    .unwrap();

    assert!(outcome.team_minted);
    assert_eq!(harness.plan_store.list_index().len(), 1);
    assert_eq!(
        outcome.plan.team_id.as_deref(),
        Some(outcome.team_id.as_str())
    );
    let mux = harness.mux.read();
    assert_eq!(mux.team_count(), 1);
    let owner = mux.get_tab(&owner_tab_id).unwrap();
    assert_eq!(owner.role, Some(AgentRole::Manager));
    assert!(owner.anchors_team(), "orchestrator kind anchors its Team");
    assert_eq!(owner.team_id.as_deref(), Some(outcome.team_id.as_str()));
}

#[test]
fn concurrent_first_plan_and_first_child_converge() {
    let dir = tempfile::tempdir().unwrap();
    let harness = TeamPlanHarness::new(dir.path());
    let (workspace_id, owner_tab_id) = teamless_orchestrator(&harness.mux);
    let child_tab_id = harness.mux.write().create_tab(&workspace_id).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let plan_mux = harness.mux.clone();
    let plan_store = harness.plan_store.clone();
    let plan_owner = owner_tab_id.clone();
    let plan_root = dir.path().display().to_string();
    let plan_barrier = barrier.clone();
    let plan_thread = std::thread::spawn(move || {
        plan_barrier.wait();
        create_team_plan_for_orchestrator(
            &plan_mux,
            &plan_store,
            "Concurrent effort".into(),
            plan_root,
            OrchestratorRef::from_tab(&plan_owner),
            vec![],
            None,
            1,
        )
    });
    let child_mux = harness.mux.clone();
    let child_owner = owner_tab_id.clone();
    let child_tab = child_tab_id.clone();
    let child_barrier = barrier.clone();
    let child_thread = std::thread::spawn(move || {
        child_barrier.wait();
        child_mux
            .write()
            .set_tab_role(&child_tab, AgentRole::Worker, Some(&child_owner))
    });
    barrier.wait();
    let outcome = plan_thread.join().unwrap().unwrap();
    child_thread.join().unwrap().unwrap();

    let mux = harness.mux.read();
    assert_eq!(mux.team_count(), 1);
    assert_eq!(harness.plan_store.list_index().len(), 1);
    assert_eq!(
        mux.get_tab(&owner_tab_id).unwrap().team_id.as_deref(),
        Some(outcome.team_id.as_str())
    );
    assert_eq!(
        mux.get_tab(&child_tab_id).unwrap().team_id.as_deref(),
        Some(outcome.team_id.as_str())
    );
    assert_eq!(
        outcome.plan.team_id.as_deref(),
        Some(outcome.team_id.as_str())
    );
}

#[test]
fn first_plan_failure_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let harness = TeamPlanHarness::new(dir.path());
    let (_workspace_id, owner_tab_id) = teamless_orchestrator(&harness.mux);
    let invalid_project = dir.path().join("not-a-directory");
    std::fs::write(&invalid_project, "file").unwrap();

    let error = create_team_plan_for_orchestrator(
        &harness.mux,
        &harness.plan_store,
        "Failure".into(),
        invalid_project.display().to_string(),
        OrchestratorRef::from_tab(&owner_tab_id),
        vec![],
        None,
        1,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        TeamPlanCreationError::Plan(PlanError::Io(_))
    ));
    assert!(harness.plan_store.list_index().is_empty());
    let mux = harness.mux.read();
    assert_eq!(mux.team_count(), 0);
    assert!(mux.get_tab(&owner_tab_id).unwrap().team_id.is_none());
}
