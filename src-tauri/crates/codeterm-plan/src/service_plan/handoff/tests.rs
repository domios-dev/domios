use super::*;
use crate::plan::{MilestoneKind, OrchestratorRef};
use crate::service_plan::NewTask;

fn store_with_task() -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Canvas Handoff".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
            vec![MilestoneKind {
                key: "decision".into(),
                label: "decision".into(),
                color: None,
            }],
            10,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Backend".into(),
                kind: "implement".into(),
                handle: Some("backend".into()),
                competition: None,
                ..Default::default()
            },
            20,
        )
        .unwrap();
    (dir, store, plan.id, task_id)
}

#[test]
fn sidecar_status_lines_do_not_count_as_dirty() {
    assert!(is_sidecar_status_line("?? .codeterm/plans/INDEX.md"));
    assert!(is_sidecar_status_line(
        "R  .codeterm/plans/a -> .codeterm/plans/b"
    ));
    assert!(!is_sidecar_status_line(" M src-tauri/src/main.rs"));
    assert!(!is_sidecar_status_line(
        "R  .codeterm/plans/a -> src/main.rs"
    ));
}

#[test]
fn create_handoff_adds_element_milestone_history_and_sidecar() {
    let (dir, store, plan_id, task_id) = store_with_task();
    let result = store
        .create_handoff(
            &plan_id,
            HandoffCreateRequest {
                mode: Some("record".into()),
                task: Some(task_id.clone()),
                goal: Some("Land backend handoff".into()),
                next_actions: vec!["Run handoff show first".into()],
                foundational_rules: vec!["Never reseed parent tab".into()],
                open_decisions: vec!["Choose reseed ack source".into()],
                narrative: Some("Overlay body\n".into()),
                from: Some(AgentRef::from_tab("worker")),
            },
            30,
        )
        .unwrap();

    let plan = store.get(&plan_id).unwrap();
    let element = plan
        .elements
        .iter()
        .find(|element| element.id == result.handoff_id)
        .unwrap();
    assert_eq!(element.kind, "handoff");
    assert_eq!(element.content["seq"], 1);
    assert_eq!(element.content["goal"], "Land backend handoff");
    assert_eq!(element.content["reseed_state"], "created");
    assert!(element.links.iter().any(|link| link == &task_id));
    assert!(plan
        .milestone_kinds
        .iter()
        .any(|kind| kind.key == "handoff"));
    assert_eq!(plan.owner_history.last().unwrap().reason, "handoff #1");
    let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
    assert!(task.milestones.iter().any(|ms| {
        ms.kind == "handoff" && ms.links.iter().any(|link| link == &result.handoff_id)
    }));

    let sidecar = dir.path().join(&result.body_path);
    assert_eq!(std::fs::read_to_string(sidecar).unwrap(), "Overlay body\n");
    store.plans.write().clear();
    store.reload_from_disk(&plan_id).unwrap();
    let reloaded = store.get(&plan_id).unwrap();
    let element = reloaded
        .elements
        .iter()
        .find(|element| element.id == result.handoff_id)
        .unwrap();
    assert_eq!(element.kind, "handoff");
    assert_eq!(element.content["goal"], "Land backend handoff");
}

#[test]
fn show_latest_handoff_returns_markdown_and_list_is_ordered() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    store
        .create_handoff(
            &plan_id,
            HandoffCreateRequest {
                task: Some(task_id.clone()),
                goal: Some("First".into()),
                narrative: Some("First body".into()),
                ..HandoffCreateRequest::default()
            },
            30,
        )
        .unwrap();
    let second = store
        .create_handoff(
            &plan_id,
            HandoffCreateRequest {
                task: Some(task_id),
                goal: Some("Second".into()),
                next_actions: vec!["Continue from next action".into()],
                narrative: Some("Second body".into()),
                ..HandoffCreateRequest::default()
            },
            40,
        )
        .unwrap();

    let list = store.list_handoffs(&plan_id).unwrap();
    assert_eq!(
        list.iter().map(|item| item.seq).collect::<Vec<_>>(),
        vec![1, 2]
    );
    let detail = store.show_handoff(&plan_id, None).unwrap();
    assert_eq!(detail.id, second.handoff_id);
    assert!(detail.markdown.contains("# Handoff #2"));
    assert!(detail.markdown.contains("- Continue from next action"));
    assert!(detail.markdown.contains("Second body"));
    assert_eq!(store.show_handoff(&plan_id, Some("1")).unwrap().seq, 1);
}

#[test]
fn reseed_records_pending_row_and_session_start_sends_once() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    let created = store
        .create_handoff(
            &plan_id,
            HandoffCreateRequest {
                task: Some(task_id),
                goal: Some("Reseed".into()),
                ..HandoffCreateRequest::default()
            },
            30,
        )
        .unwrap();

    let result = store
        .prepare_handoff_reseed(
            &plan_id,
            &HandoffReseedRequest {
                selector: Some(created.handoff_id.clone()),
                tab_id: "tab-a".into(),
                source_session_id: "old-session".into(),
                mode: "new".into(),
                force: false,
                dry_run: false,
            },
            "/new",
            "resume seed",
            40,
        )
        .unwrap();
    assert_eq!(result.state, "new_sent");
    assert_eq!(result.sends[0].text, "/new");

    let resume = store
        .observe_handoff_session_start("tab-a", "new-session", 50)
        .unwrap()
        .unwrap();
    // always injected); the authored seed remains the trailing payload.
    assert!(resume.text.contains("codeterm-state-reconciliation"));
    assert!(resume.text.trim_end().ends_with("resume seed"));
    assert_eq!(resume.handoff_id, created.handoff_id);
    assert!(store
        .observe_handoff_session_start("tab-a", "new-session-2", 60)
        .unwrap()
        .is_none());
    let detail = store.show_handoff(&plan_id, None).unwrap();
    assert_eq!(detail.content["reseed_state"], "session_seen");
    assert_eq!(
        detail.content["reseed_pending"]["target_session_id"],
        "new-session"
    );
    store
        .mark_handoff_resume_sent(&plan_id, &created.handoff_id, 70)
        .unwrap();
    let detail = store.show_handoff(&plan_id, None).unwrap();
    assert_eq!(detail.content["reseed_state"], "resume_sent");
    store
        .ack_handoff_resume(&plan_id, &created.handoff_id, 80)
        .unwrap();
    let detail = store.show_handoff(&plan_id, None).unwrap();
    assert_eq!(detail.content["reseed_state"], "acked");
    assert_eq!(detail.content["reseed_pending"]["state"], "acked");
}

#[test]
fn reseed_rejects_active_pending_row_without_force() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    store
        .create_handoff(
            &plan_id,
            HandoffCreateRequest {
                task: Some(task_id),
                ..HandoffCreateRequest::default()
            },
            30,
        )
        .unwrap();
    let request = HandoffReseedRequest {
        selector: None,
        tab_id: "tab-a".into(),
        source_session_id: "old-session".into(),
        mode: "new".into(),
        force: false,
        dry_run: false,
    };
    store
        .prepare_handoff_reseed(&plan_id, &request, "/new", "resume seed", 40)
        .unwrap();
    let err = store
        .prepare_handoff_reseed(&plan_id, &request, "/new", "resume seed", 41)
        .unwrap_err();
    assert!(err.to_string().contains("already pending"));

    let forced = HandoffReseedRequest {
        force: true,
        ..request
    };
    store
        .prepare_handoff_reseed(&plan_id, &forced, "/new", "resume seed", 42)
        .unwrap();
}

#[test]
fn compact_reseed_sequence_is_seed_then_compact() {
    let (_dir, store, plan_id, task_id) = store_with_task();
    store
        .create_handoff(
            &plan_id,
            HandoffCreateRequest {
                task: Some(task_id),
                ..HandoffCreateRequest::default()
            },
            30,
        )
        .unwrap();
    let result = store
        .prepare_handoff_reseed(
            &plan_id,
            &HandoffReseedRequest {
                selector: None,
                tab_id: "tab-a".into(),
                source_session_id: "old-session".into(),
                mode: "compact".into(),
                force: false,
                dry_run: false,
            },
            "/compact",
            "resume seed",
            40,
        )
        .unwrap();
    assert_eq!(result.state, "resume_sent");
    assert_eq!(result.sends[0].reason, "resume_seed");
    assert_eq!(result.sends[1].text, "/compact");
}
