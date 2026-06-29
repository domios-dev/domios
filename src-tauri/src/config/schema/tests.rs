use super::{IntegrationsConfig, MemoryConfig, OrchestrationConfig, UiConfig};

#[test]
fn ui_config_extra_keys_round_trip_over_json() {
    // Centralized client settings ride the flatten `extra` map. The frontend
    // talks JSON over IPC, so the JSON round-trip is the load-bearing path.
    let json = r#"{
        "default_new_agent_pane_chat": true,
        "chat_ui_default_prompted": true,
        "usage_intro_hint_opens": 3
    }"#;
    let cfg: UiConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.default_new_agent_pane_chat, "typed field still parses");
    assert_eq!(
        cfg.extra.get("chat_ui_default_prompted"),
        Some(&toml::Value::Boolean(true))
    );
    assert_eq!(
        cfg.extra.get("usage_intro_hint_opens"),
        Some(&toml::Value::Integer(3))
    );

    let back = serde_json::to_value(&cfg).unwrap();
    assert_eq!(
        back["chat_ui_default_prompted"],
        serde_json::json!(true),
        "extra keys re-emit flat"
    );
    assert_eq!(back["usage_intro_hint_opens"], serde_json::json!(3));
}

#[test]
fn ui_config_extra_survives_toml_round_trip() {
    let toml_src = "default_new_agent_pane_chat = true\nkeepawake_display = true\n";
    let cfg: UiConfig = toml::from_str(toml_src).unwrap();
    assert_eq!(
        cfg.extra.get("keepawake_display"),
        Some(&toml::Value::Boolean(true))
    );
    let serialized = toml::to_string(&cfg).unwrap();
    assert!(
        serialized.contains("keepawake_display = true"),
        "extra persists to disk: {serialized}"
    );
}

#[test]
fn memory_l1_budget_default_is_300() {
    let cfg: MemoryConfig = toml::from_str("").unwrap();
    assert_eq!(cfg.l1_budget_tokens, 300);
}

#[test]
fn memory_l1_budget_user_override() {
    let cfg: MemoryConfig = toml::from_str("l1_budget_tokens = 1200").unwrap();
    assert_eq!(cfg.l1_budget_tokens, 1200);
}

#[test]
fn orchestration_plan_reminder_defaults() {
    let cfg: OrchestrationConfig = toml::from_str("").unwrap();
    assert!(cfg.plan_reminders);
    assert_eq!(cfg.nudge_after_turns, 12);
}

#[test]
fn orchestration_plan_reminder_overrides() {
    let cfg: OrchestrationConfig =
        toml::from_str("plan_reminders = false\nnudge_after_turns = 3").unwrap();
    assert!(!cfg.plan_reminders);
    assert_eq!(cfg.nudge_after_turns, 3);
}

#[test]
fn telegram_allowed_users_accepts_quoted_strings() {
    let toml = r#"
        telegram_enabled = true
        telegram_allowed_users = ["100000001"]
    "#;
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.telegram_allowed_users, vec!["100000001".to_string()]);
}

#[test]
fn telegram_allowed_users_accepts_bare_integers() {
    let toml = r#"
        telegram_enabled = true
        telegram_allowed_users = [100000001]
    "#;
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        cfg.telegram_allowed_users,
        vec!["100000001".to_string()],
        "bare integer chat ids must coerce to strings — was silent rejection"
    );
}

#[test]
fn telegram_allowed_users_accepts_mixed() {
    let toml = r#"
        telegram_allowed_users = [100000001, "789012345"]
    "#;
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        cfg.telegram_allowed_users,
        vec!["100000001".to_string(), "789012345".to_string()]
    );
}

#[test]
fn discord_allowed_users_also_tolerant() {
    let toml = "discord_allowed_users = [555, \"666\"]";
    let cfg: IntegrationsConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        cfg.discord_allowed_users,
        vec!["555".to_string(), "666".to_string()]
    );
}
