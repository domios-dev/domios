use super::{IntegrationsConfig, MemoryConfig};

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
