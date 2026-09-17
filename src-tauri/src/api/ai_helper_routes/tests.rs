use super::AiHelperSpawnBody;

#[test]
fn ai_helper_spawn_body_carries_only_title_and_task() {
    let body: AiHelperSpawnBody =
        serde_json::from_str(r#"{"title":"ai-helper-title-7c1","task":"ai-helper-task-3e9"}"#)
            .unwrap();
    assert_eq!(body.title, "ai-helper-title-7c1");
    assert_eq!(body.task, "ai-helper-task-3e9");
}

#[test]
fn ai_helper_spawn_body_refuses_client_placement_or_provider() {
    for smuggled in [
        r#"{"title":"t","task":"x","workspace_id":"ws-active"}"#,
        r#"{"title":"t","task":"x","cwd":"project"}"#,
        r#"{"title":"t","task":"x","agent":"provider-fixture"}"#,
        r#"{"title":"t","task":"x","parent_tab_id":"tab"}"#,
    ] {
        assert!(serde_json::from_str::<AiHelperSpawnBody>(smuggled).is_err());
    }
}
