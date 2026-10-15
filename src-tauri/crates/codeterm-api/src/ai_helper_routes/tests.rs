use super::AiHelperSpawnBody;

#[test]
fn ai_helper_spawn_body_carries_title_description_and_task() {
    let body: AiHelperSpawnBody = serde_json::from_str(
        r#"{"title":"Configure","description":"Configure plugin fixture","task":"ai-helper-task-3e9"}"#,
    )
    .unwrap();
    assert_eq!(body.title, "Configure");
    assert_eq!(body.description, "Configure plugin fixture");
    assert_eq!(body.task, "ai-helper-task-3e9");
}

#[test]
fn ai_helper_spawn_body_refuses_client_placement_or_provider() {
    for smuggled in [
        r#"{"title":"t","description":"d","task":"x","workspace_id":"ws-active"}"#,
        r#"{"title":"t","description":"d","task":"x","cwd":"project"}"#,
        r#"{"title":"t","description":"d","task":"x","agent":"provider-fixture"}"#,
        r#"{"title":"t","description":"d","task":"x","parent_tab_id":"tab"}"#,
    ] {
        assert!(serde_json::from_str::<AiHelperSpawnBody>(smuggled).is_err());
    }
}
