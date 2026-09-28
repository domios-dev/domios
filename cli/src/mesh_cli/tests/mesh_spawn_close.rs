use super::cli_command_to_api;
use super::parse_spawn_command;

#[test]
fn mesh_spawn_sends_local_task_file_body_not_path() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let file = format!("mesh-task-{}.txt", uuid::Uuid::new_v4());
    let local_path = std::env::current_dir().unwrap().join(&file);
    let contents = "local task body with $(literal) and `ticks`";
    std::fs::write(&local_path, contents).unwrap();
    let command =
        parse_spawn_command(&["claude", "--cwd", "D:/peer/workspace", "--task-file", &file]);
    let (_, _, body) = cli_command_to_api(&command);
    let _ = std::fs::remove_file(&local_path);
    let body: serde_json::Value = serde_json::from_str(&body.unwrap()).unwrap();
    assert_eq!(body["task"], contents);
    assert_eq!(body["cwd"], "D:/peer/workspace");
    assert!(!body.to_string().contains(&file));
}

#[test]
fn mesh_spawn_unreadable_task_file_refuses() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let missing =
        std::env::temp_dir().join(format!("missing-mesh-task-{}.txt", uuid::Uuid::new_v4()));
    let command = parse_spawn_command(&[
        "claude",
        "--cwd",
        "D:/peer/workspace",
        "--task-file",
        missing.to_str().unwrap(),
    ]);
    let (method, _, body) = cli_command_to_api(&command);
    assert_eq!(method, super::super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(body.is_none());
}

#[test]
fn mesh_spawn_without_cwd_refuses() {
    for args in [
        vec!["claude", "--task", "work"],
        vec!["claude", "--cwd", " ", "--task", "work"],
    ] {
        let command = parse_spawn_command(&args);
        let (method, _, body) = cli_command_to_api(&command);
        assert_eq!(method, super::super::api_map::UNSUPPORTED_OVER_MESH);
        assert!(body.is_none());
    }
}

#[test]
fn mesh_tab_close_reports_success_when_tab_is_gone() {
    let result = super::super::relay::verified_close_result(
        Ok(serde_json::json!([{"id": "closing"}])),
        Err(super::super::relay::test_close_failure()),
        Ok(serde_json::json!([])),
        "closing",
    );
    assert_eq!(result.unwrap()["ok"], true);
}

#[test]
fn mesh_tab_close_keeps_failure_when_gone_check_fails() {
    let close_failure = super::super::relay::test_close_failure();
    let result = super::super::relay::verified_close_result(
        Ok(serde_json::json!([{"id": "closing"}])),
        Err(close_failure.clone()),
        Err(super::super::relay::test_close_failure()),
        "closing",
    );
    assert_eq!(result, Err(close_failure));
}

#[test]
fn mesh_tab_close_keeps_failure_when_gone_check_is_not_a_list() {
    let close_failure = super::super::relay::test_close_failure();
    let result = super::super::relay::verified_close_result(
        Ok(serde_json::json!([{"id": "closing"}])),
        Err(close_failure.clone()),
        Ok(serde_json::json!({"unexpected": true})),
        "closing",
    );
    assert_eq!(result, Err(close_failure));
}

#[test]
fn mesh_tab_close_refuses_id_missing_before_close() {
    let result = super::super::relay::verified_close_result(
        Ok(serde_json::json!([{"id": "other"}])),
        Ok(serde_json::json!({"ok": true})),
        Ok(serde_json::json!([])),
        "never-existed",
    );
    assert!(result.is_err());
}
