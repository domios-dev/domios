use super::schema::AppConfig;

const OLD_KEYS: [&str; 3] = [
    "resume_skip_permissions",
    "spawn_skip_permissions",
    "spawn_skip_permissions_remembered",
];

pub(super) fn migrate(config: &mut AppConfig, contents: &str) -> bool {
    let raw = toml::from_str::<toml::Value>(contents).ok();
    let global_enabled = raw
        .as_ref()
        .and_then(|value| value.get("agent"))
        .and_then(|value| value.get("resume_skip_permissions"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    let provider_enabled = config.agents.values().any(|provider| {
        let remembered_spawn = bool_extra(provider, "spawn_skip_permissions")
            && bool_extra(provider, "spawn_skip_permissions_remembered");
        remembered_spawn || bool_extra(provider, "resume_skip_permissions")
    });
    let had_old_keys = global_enabled
        || raw
            .as_ref()
            .and_then(|value| value.get("agent"))
            .and_then(toml::Value::as_table)
            .is_some_and(|agent| agent.contains_key("resume_skip_permissions"))
        || config
            .agents
            .values()
            .any(|provider| OLD_KEYS.iter().any(|key| provider.extra.contains_key(*key)));
    if config.agent.full_access.is_none() && (global_enabled || provider_enabled) {
        config.agent.full_access = Some(true);
    }
    for provider in config.agents.values_mut() {
        for key in OLD_KEYS {
            provider.extra.remove(key);
        }
    }
    had_old_keys
}

fn bool_extra(config: &super::schema::ProviderConfig, key: &str) -> bool {
    config.extra.get(key).and_then(toml::Value::as_bool) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_global_agent_access_migrates_and_removes_old_key() {
        let raw = "[agent]\nresume_skip_permissions = true\n";
        let mut config: AppConfig = toml::from_str(raw).unwrap();
        assert!(migrate(&mut config, raw));
        assert_eq!(config.agent.full_access, Some(true));
        assert!(!serialized(&config).contains("resume_skip_permissions"));
    }

    #[test]
    fn remembered_provider_access_migrates_once_and_cleans_provider_keys() {
        let raw = r#"
[agents.claude]
spawn_skip_permissions = true
spawn_skip_permissions_remembered = true
"#;
        let mut config: AppConfig = toml::from_str(raw).unwrap();
        assert!(migrate(&mut config, raw));
        assert_eq!(config.agent.full_access, Some(true));
        assert!(!serialized(&config).contains("spawn_skip_permissions"));
    }

    #[test]
    fn explicit_new_access_choice_wins_over_old_provider_value() {
        let raw = r#"
[agent]
full_access = false

[agents.claude]
spawn_skip_permissions = true
spawn_skip_permissions_remembered = true
"#;
        let mut config: AppConfig = toml::from_str(raw).unwrap();
        assert!(migrate(&mut config, raw));
        assert_eq!(config.agent.full_access, Some(false));
        assert!(!serialized(&config).contains("spawn_skip_permissions"));
    }

    fn serialized(config: &AppConfig) -> String {
        toml::to_string_pretty(config).unwrap()
    }
}
