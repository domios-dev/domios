use super::*;
use crate::persisted_state::incidents_for;

const GARBAGE: &str = "\nthis is = = not toml [[[\n";

const VALID: &str = r#"
[api]
enabled = true
token = "tok-original"

[agent]
full_access = true

[theme]
name = "nord"

[general_agent]
agent_provider = "codex"

[memory]
dreaming_agent = "opencode"

[agents.claude]
default_model = "opus"

[kpi]
enabled = false
default_applied = true
"#;

fn bump_mtime(path: &Path) {
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(later)
        .unwrap();
}

#[test]
fn config_parse_error_preserves_file_and_token() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    config_file(&path).save(VALID).unwrap();
    let broken = format!("{VALID}{GARBAGE}");
    std::fs::write(&path, &broken).unwrap();

    let cache: ConfigCache = Mutex::new(None);
    let config = load_config_cached(&path, &cache);

    assert_eq!(config.api.token.as_deref(), Some("tok-original"));
    assert_eq!(config.agent.full_access, Some(true));
    let file = config_file(&path);
    let copies = file.corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), broken);
    let restored: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored.api.token.as_deref(), Some("tok-original"));
    assert_eq!(restored.agent.full_access, Some(true));
    assert!(file.held().is_none());
    assert_eq!(incidents_for(&path).len(), 1);
}

#[test]
fn config_parse_error_without_backup_writes_marked_salvage_and_keeps_hold() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let broken = format!("{VALID}{GARBAGE}");
    std::fs::write(&path, &broken).unwrap();

    let cache: ConfigCache = Mutex::new(None);
    let config = load_config_cached(&path, &cache);

    assert_eq!(
        config.api.token.as_deref(),
        Some("tok-original"),
        "the configured token is salvaged, never rotated"
    );
    assert_eq!(config.agent.full_access, Some(true));
    assert!(
        path.exists(),
        "a readable recovery config stays at config.toml"
    );
    let salvaged = std::fs::read_to_string(&path).unwrap();
    assert!(is_salvaged_config(&salvaged));
    assert!(
        !salvaged.contains("[font]"),
        "unsalvaged defaults stay implicit"
    );
    let restored: AppConfig = toml::from_str(&salvaged).unwrap();
    assert_eq!(restored.api.token.as_deref(), Some("tok-original"));
    assert_eq!(restored.agent.full_access, Some(true));
    assert_eq!(restored.theme.name, "nord");
    assert_eq!(restored.general_agent.agent_provider, "codex");
    assert_eq!(restored.memory.dreaming_agent, "opencode");
    assert_eq!(
        restored.agents["claude"].default_model.as_deref(),
        Some("opus")
    );
    assert!(
        restored.kpi.enabled,
        "the damaged final table falls back to defaults"
    );
    let file = config_file(&path);
    let copies = file.corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(
        std::fs::read(&copies[0]).unwrap(),
        broken.as_bytes().to_vec()
    );
    assert!(file.held().is_some());
    assert!(file.save("[api]\nenabled = true\n").is_err());
    assert!(file.backups().is_empty());
    assert!(incidents_for(&path)[0]
        .outcome()
        .contains("no valid backup"));
    assert!(!incidents_for(&path)[0]
        .outcome()
        .contains("running on defaults"));

    let restarted_cache: ConfigCache = Mutex::new(None);
    let again = load_config_cached(&path, &restarted_cache);
    assert_eq!(again.api.token.as_deref(), Some("tok-original"));
    assert!(
        file.held().is_some(),
        "the salvage marker restores the hold"
    );

    let mut consented = again;
    consented
        .agents
        .entry("claude".into())
        .or_default()
        .extra
        .insert(
            crate::config::schema::INTEGRATION_CONSENT_GRANTED.into(),
            toml::Value::Boolean(true),
        );
    crate::config::save_hook_reconciliation_config_at(&path, &consented).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(is_salvaged_config(&saved));
    let saved: AppConfig = toml::from_str(&saved).unwrap();
    assert_eq!(
        saved.agents["claude"].extra[crate::config::schema::INTEGRATION_CONSENT_GRANTED].as_bool(),
        Some(true)
    );
    assert!(
        file.held().is_some(),
        "internal consent does not release user-write hold"
    );
    assert!(file.save("[api]\nenabled = true\n").is_err());
    assert_eq!(
        std::fs::read(&copies[0]).unwrap(),
        broken.as_bytes().to_vec()
    );
    assert_eq!(
        incidents_for(&path).len(),
        1,
        "one incident for one broken file"
    );
}

#[test]
fn config_salvage_keeps_intact_tables_after_a_damaged_table() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let broken = "[api]\ntoken = \"tok-original\"\nthis is = = not toml [[[\n[agent]\nfull_access = false\n[general_agent]\nagent_provider = \"codex\"\n[memory]\ndreaming_agent = \"opencode\"\n";
    std::fs::write(&path, broken).unwrap();

    let cache: ConfigCache = Mutex::new(None);
    let config = load_config_cached(&path, &cache);
    let salvaged = std::fs::read_to_string(&path).unwrap();
    let restored: AppConfig = toml::from_str(&salvaged).unwrap();

    assert_eq!(config.api.token.as_deref(), Some("tok-original"));
    assert_eq!(restored.agent.full_access, Some(false));
    assert_eq!(restored.general_agent.agent_provider, "codex");
    assert_eq!(restored.memory.dreaming_agent, "opencode");
    assert_eq!(
        std::fs::read(&config_file(&path).corrupt_copies()[0]).unwrap(),
        broken.as_bytes().to_vec()
    );
}

#[test]
fn unreadable_token_is_never_generated_after_a_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        "[api]\ntoken = \"unterminated\n[agent]\nfull_access = false\n",
    )
    .unwrap();

    let cache: ConfigCache = Mutex::new(None);
    let config = load_config_cached(&path, &cache);

    assert!(config.api.token.is_none());
    assert_eq!(config.agent.full_access, Some(false));
    assert!(path.exists());
    assert!(toml::from_str::<AppConfig>(&std::fs::read_to_string(&path).unwrap()).is_ok());
}

#[test]
fn runtime_parse_error_keeps_last_known_config_and_holds_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(&path, VALID).unwrap();
    let cache: ConfigCache = Mutex::new(None);
    assert_eq!(
        load_config_cached(&path, &cache).api.token.as_deref(),
        Some("tok-original")
    );

    let broken = "[api]\ntoken = \"tok-other\"\n[[[";
    std::fs::write(&path, broken).unwrap();
    bump_mtime(&path);
    let config = load_config_cached(&path, &cache);

    assert_eq!(config.api.token.as_deref(), Some("tok-original"));
    assert_eq!(config.theme.name, "nord");
    let file = config_file(&path);
    assert_eq!(
        std::fs::read_to_string(&file.corrupt_copies()[0]).unwrap(),
        broken
    );
    assert!(
        file.held().is_some(),
        "no backup was ever written, so writes are held"
    );
    let held: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(held.api.token.as_deref(), Some("tok-original"));
    assert!(file.backups().is_empty());

    std::fs::write(&path, VALID.replace("tok-original", "tok-fixed")).unwrap();
    bump_mtime(&path);
    let fixed = load_config_cached(&path, &cache);
    assert_eq!(fixed.api.token.as_deref(), Some("tok-fixed"));
    assert!(file.held().is_none(), "a fixed file releases the hold");
}

#[test]
fn salvage_reads_only_intact_lines_of_the_right_tables() {
    let raw = "token = \"root-level\"\n[api]\nport = 7686\ntoken = \"t1\"\n[mesh]\nfull_access = true\n[agent]\nfull_access = false\n";
    let (token, full_access) = salvaged_critical_values(raw);
    assert_eq!(token.as_deref(), Some("t1"));
    assert_eq!(full_access, Some(false));
}
