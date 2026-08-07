use super::{
    IntegrationsConfig, MemoryConfig, NotificationConfig, OrchestrationConfig, UiConfig,
    UsageConfig,
};

#[test]
fn telegram_push_is_opt_out_on_a_fresh_install() {
    let cfg = NotificationConfig::default();
    assert!(
        !cfg.telegram.enabled,
        "a fresh install must behave exactly as before"
    );
    assert!(
        cfg.telegram.recipients.is_empty(),
        "no audience is configured by default, so nobody receives anything"
    );
    assert!(cfg.telegram.per_scope_limit > 0);

    let from_empty_toml: NotificationConfig = toml::from_str("").unwrap();
    assert!(!from_empty_toml.telegram.enabled);
    assert!(from_empty_toml.telegram.recipients.is_empty());
}

#[test]
fn telegram_push_recipients_accept_bare_and_quoted_chat_ids() {
    let toml = r#"
        [telegram]
        enabled = true
        recipients = [100000001, "789012345"]
    "#;
    let cfg: NotificationConfig = toml::from_str(toml).unwrap();
    assert!(cfg.telegram.enabled);
    assert_eq!(
        cfg.telegram.recipients,
        vec!["100000001".to_string(), "789012345".to_string()]
    );
    assert_eq!(
        cfg.telegram.per_scope_limit,
        NotificationConfig::default().telegram.per_scope_limit,
        "an omitted ceiling falls back to the default rather than to zero"
    );
}

#[test]
fn ui_config_extra_keys_round_trip_over_json() {
    // talks JSON over IPC, so the JSON round-trip is the load-bearing path.
    let json = r#"{
        "usage_intro_hint_opens": 3
    }"#;
    let cfg: UiConfig = serde_json::from_str(json).unwrap();
    assert_eq!(
        cfg.extra.get("usage_intro_hint_opens"),
        Some(&toml::Value::Integer(3))
    );

    let back = serde_json::to_value(&cfg).unwrap();
    assert_eq!(back["usage_intro_hint_opens"], serde_json::json!(3));
}

#[test]
fn ui_config_extra_survives_toml_round_trip() {
    let toml_src = "keepawake_display = true\n";
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
    assert_eq!(cfg.nudge_interval_secs, 1800);
    assert_eq!(cfg.reflect.every_gates, 5);
    assert!(cfg.digests.enabled);
    assert_eq!(cfg.digests.rules.len(), 2);

    let serialized = toml::to_string(&cfg).unwrap();
    assert!(!serialized.contains(concat!("nudge_after_", "turns")));
    assert!(!serialized.contains(concat!("idle_", "poke")));
}

#[test]
fn usage_config_defaults_on() {
    let cfg: UsageConfig = toml::from_str("").unwrap();
    assert!(cfg.enabled);
    assert!(cfg.heartbeat);
    assert_eq!(cfg.freshness_secs, 180);
    assert_eq!(cfg.standalone_cadence_secs, 0);
}

#[test]
fn legacy_usage_threshold_is_ignored() {
    let cfg: UsageConfig = toml::from_str("high_pct_threshold = 75").unwrap();
    assert!(cfg.enabled);
}

#[test]
fn orchestration_reflect_every_gates_override() {
    let cfg: OrchestrationConfig = toml::from_str("[reflect]\nevery_gates = 25\n").unwrap();
    assert_eq!(cfg.reflect.every_gates, 25);
}

#[test]
fn orchestration_refresh_defaults_after_reflection_off() {
    let cfg: OrchestrationConfig = toml::from_str("").unwrap();
    assert!(
        !cfg.refresh.after_reflection,
        "after_reflection is OFF by default"
    );
    assert_eq!(cfg.refresh.handoff_timeout_secs, 60);
    assert_eq!(cfg.refresh.settle_timeout_secs, 120);
    assert_eq!(cfg.refresh.reorient_timeout_secs, 30);
    assert_eq!(cfg.refresh.verify_timeout_secs, 180);
}

#[test]
fn orchestration_refresh_overrides() {
    let cfg: OrchestrationConfig = toml::from_str(
        "[refresh]\nafter_reflection = true\nsettle_timeout_secs = 5\nverify_timeout_secs = 20\n",
    )
    .unwrap();
    assert!(cfg.refresh.after_reflection);
    assert_eq!(cfg.refresh.settle_timeout_secs, 5);
    assert_eq!(cfg.refresh.verify_timeout_secs, 20);
    assert_eq!(cfg.refresh.handoff_timeout_secs, 60);
}

#[test]
fn orchestration_plan_reminder_overrides() {
    let cfg: OrchestrationConfig =
        toml::from_str("plan_reminders = false\nnudge_interval_secs = 90").unwrap();
    assert!(!cfg.plan_reminders);
    assert_eq!(cfg.nudge_interval_secs, 90);
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
