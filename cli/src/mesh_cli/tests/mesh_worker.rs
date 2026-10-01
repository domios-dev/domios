use super::*;

/// A generic task-less mesh Worker still relies on `role` for its Worker
/// doctrine; plan-bound dispatch uses the separate origin-owned protocol.
#[test]
fn a_task_less_mesh_worker_relays_its_role_to_the_peer() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let (method, path, body) = cli_command_to_api(&parse_spawn_command(&[
        "claude",
        "--cwd",
        "D:/peer/workspace",
        "--role",
        "Worker",
        "--task",
        "check out the repo, build it, serve it",
    ]));

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/agents/spawn");
    let body: serde_json::Value =
        serde_json::from_str(&body.expect("agent spawn carries a body")).unwrap();
    assert_eq!(body["role"], "Worker");
    assert!(
        body.get("plan_task").is_none() && body.get("task_id").is_none(),
        "the mesh Worker owns no binding: {body}"
    );
    assert!(
        body.get("execution_intent").is_none(),
        "no declared intent is its own shape: {body}"
    );
}
