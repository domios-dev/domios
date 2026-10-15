use super::*;

#[test]
fn paused_task_dispatch_refuses_before_spawn_without_agent_resume_instruction() {
    let paused = task(json!({
        "id": "task_paused",
        "name": "waiting for user",
        "type": "task",
        "status": "paused"
    }));

    let error = preflight_dispatch_task_status("plan_1", &paused).unwrap_err();
    let data = error.data.expect("structured paused-task refusal");

    assert_eq!(error.code, -32602);
    assert_eq!(data["type"], "invalid_task_dispatch");
    assert_eq!(data["stage"], "task_status");
    assert_eq!(data["hold_kind"], "unclassified");
    assert!(data.get("fix").is_none());
}

#[test]
fn dispatch_refuses_statuses_that_cannot_reach_startup() {
    for status in ["paused", "done", "cancelled", "in_review"] {
        let candidate = task(json!({
            "id": format!("task_{status}"),
            "name": "status preflight",
            "type": "task",
            "status": status,
        }));
        let error = preflight_dispatch_task_status("plan_1", &candidate).unwrap_err();
        let data = error.data.expect("structured status refusal");
        if status == "paused" {
            assert_eq!(data["type"], "invalid_task_dispatch");
        } else {
            assert_eq!(data["type"], "transition_denied");
        }
        if status == "in_review" {
            assert_eq!(data["from"], "in_review");
            assert_eq!(data["to"], "pending");
        }
    }

    for status in ["pending", "in_progress", "blocked"] {
        let task = task(json!({
            "id": format!("task_{status}"),
            "name": "status preflight",
            "type": "task",
            "status": status,
        }));
        preflight_dispatch_task_status("plan_1", &task).unwrap();
    }
}
