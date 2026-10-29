use super::*;
use codeterm_util::persisted_state::incidents_for;

#[test]
fn permission_tightening_failure_is_only_a_warning_with_the_path() {
    let path = Path::new("isolated/config.toml");
    assert!(permission_warning(path, Ok(())).is_none());
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::InvalidInput,
    ] {
        let warning = permission_warning(path, Err(std::io::Error::from(kind))).unwrap();
        assert!(warning.contains(&path.display().to_string()));
        assert!(warning.contains("continuing"));
    }
}

#[test]
fn credential_failure_requires_recoverable_authority_to_continue() {
    for token in [None, Some(""), Some(" ")] {
        assert_eq!(
            credential_failure(token, "unreadable .secrets".into()),
            Err("unreadable .secrets".into())
        );
    }
    assert!(credential_failure(Some("retained"), "write failed".into()).is_ok());
}

#[test]
fn unreadable_or_corrupt_secrets_return_a_startup_error_without_replacement() {
    for existing_config in [false, true] {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("config.toml");
            if existing_config {
                std::fs::write(&path, "[theme]\nname = 'nord'\n").unwrap();
            }
            let secrets = root.path().join(".secrets");
            if directory {
                std::fs::create_dir(&secrets).unwrap();
            } else {
                std::fs::write(&secrets, "corrupt secrets").unwrap();
            }
            let cache = Mutex::new(None);
            let error = load_config_cached_at(&path, root.path(), &cache).unwrap_err();
            assert!(error.contains(&secrets.display().to_string()));
            assert!(cache.lock().is_none());
            if directory {
                assert!(secrets.is_dir());
            } else {
                assert_eq!(std::fs::read_to_string(secrets).unwrap(), "corrupt secrets");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn symlinked_config_and_secrets_load_the_existing_token() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let token = codeterm_util::instance_token::initialize_at(target.path(), None, true).unwrap();
    let target_config = target.path().join("settings.toml");
    std::fs::write(
        &target_config,
        "[theme]\nname = 'nord'\n[kpi]\ndefault_applied = true\n",
    )
    .unwrap();
    let path = root.path().join("config.toml");
    symlink(&target_config, &path).unwrap();
    symlink(target.path().join(".secrets"), root.path().join(".secrets")).unwrap();
    let config = load_config_cached_at(&path, root.path(), &Mutex::new(None)).unwrap();
    assert_eq!(config.api.token, token);
    assert_eq!(config.theme.name, "nord");
}

#[test]
fn non_regular_config_does_not_panic_or_replace_existing_authority() {
    let root = tempfile::tempdir().unwrap();
    let token = codeterm_util::instance_token::initialize_at(root.path(), None, true).unwrap();
    let path = root.path().join("config.toml");
    std::fs::create_dir(&path).unwrap();
    let config = load_config_cached_at(&path, root.path(), &Mutex::new(None)).unwrap();
    assert_eq!(config.api.token, token);
    assert!(path.is_dir());
}

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

    assert_rotated(tmp.path(), &config);
    assert_eq!(config.agent.full_access, Some(true));
    let file = config_file(&path);
    let copies = file.corrupt_copies();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), broken);
    let restored: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(restored.api.token.is_none());
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

    assert_rotated(tmp.path(), &config);
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
    assert!(restored.api.token.is_none());
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
    assert_eq!(again.api.token, config.api.token);
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
            crate::schema::INTEGRATION_CONSENT_GRANTED.into(),
            toml::Value::Boolean(true),
        );
    crate::save_hook_reconciliation_config_at(&path, &consented).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(is_salvaged_config(&saved));
    let saved: AppConfig = toml::from_str(&saved).unwrap();
    assert_eq!(
        saved.agents["claude"].extra[crate::schema::INTEGRATION_CONSENT_GRANTED].as_bool(),
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

    assert_rotated(tmp.path(), &config);
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
    codeterm_util::instance_token::initialize_at(tmp.path(), Some("tok-original"), false).unwrap();
    std::fs::write(&path, VALID.replace("token = \"tok-original\"\n", "")).unwrap();
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
    assert!(held.api.token.is_none());
    assert!(file.backups().is_empty());

    std::fs::write(&path, VALID.replace("tok-original", "tok-fixed")).unwrap();
    bump_mtime(&path);
    let fixed = load_config_cached(&path, &cache);
    assert_eq!(fixed.api.token.as_deref(), Some("tok-original"));
    assert!(file.held().is_none(), "a fixed file releases the hold");
}

#[test]
fn config_bounds_invalid_load_keeps_last_known_config() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    codeterm_util::instance_token::initialize_at(tmp.path(), Some("tok-original"), false).unwrap();
    std::fs::write(&path, VALID.replace("token = \"tok-original\"\n", "")).unwrap();
    let cache: ConfigCache = Mutex::new(None);
    let first = load_config_cached(&path, &cache);
    assert_eq!(first.api.token.as_deref(), Some("tok-original"));

    std::fs::write(&path, format!("{VALID}\n[font]\nsize = 0\n")).unwrap();
    bump_mtime(&path);
    let recovered = load_config_cached(&path, &cache);

    assert_eq!(recovered.api.token.as_deref(), Some("tok-original"));
    assert_eq!(recovered.font.size, AppConfig::default().font.size);
    assert!(config_file(&path).held().is_some());
}

#[test]
fn salvage_reads_only_intact_lines_of_the_right_tables() {
    let raw = "token = \"root-level\"\n[api]\nport = 7686\ntoken = \"t1\"\n[mesh]\nfull_access = true\n[agent]\nfull_access = false\n";
    let (token, full_access) = salvaged_critical_values(raw);
    assert_eq!(token.as_deref(), Some("t1"));
    assert_eq!(full_access, Some(false));
}

#[test]
fn migration_removes_config_token_and_never_rereads_credential_on_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(&path, VALID).unwrap();
    let cache = Mutex::new(None);
    let initial = load_config_cached(&path, &cache);
    assert_rotated(tmp.path(), &initial);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("tok-original"));
    assert!(!text.parse::<toml::Table>().unwrap()["api"]
        .as_table()
        .unwrap()
        .contains_key("token"));
    std::fs::write(tmp.path().join(".secrets"), "unreadable-after-start").unwrap();
    std::fs::write(&path, text.replace("nord", "dracula")).unwrap();
    bump_mtime(&path);
    let reloaded = load_config_cached(&path, &cache);
    assert_eq!(reloaded.api.token, initial.api.token);
    assert_eq!(reloaded.theme.name, "dracula");
}

#[test]
fn failed_credential_persistence_keeps_legacy_config_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(&path, VALID).unwrap();
    std::fs::write(tmp.path().join(".secrets"), "damaged").unwrap();
    let cache = Mutex::new(None);
    let config = load_config_cached(&path, &cache);
    assert_eq!(config.api.token.as_deref(), Some("tok-original"));
    assert!(config_file(&path).held().is_some());
    assert!(config_file(&path).save("[api]\nenabled = true\n").is_err());
    assert!(incidents_for(&path)
        .iter()
        .any(|incident| incident.error.contains("rotation failed")));
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".secrets")).unwrap(),
        "damaged"
    );
    // Repairing storage does not change the cached runtime authority on reload.
    std::fs::remove_file(tmp.path().join(".secrets")).unwrap();
    bump_mtime(&path);
    let reloaded = load_config_cached(&path, &cache);
    assert_eq!(reloaded.api.token, config.api.token);
    assert!(!tmp.path().join(".secrets").exists());
    assert!(config_file(&path).held().is_some());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
    assert!(crate::save_hook_reconciliation_config_at(&path, &reloaded).is_err());
    let restarted = load_config_cached(&path, &Mutex::new(None));
    assert_rotated(tmp.path(), &restarted);
    assert!(config_file(&path).held().is_none());
}

#[test]
fn credentials_use_data_root_when_config_is_separate() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let path = config.path().join("config.toml");
    std::fs::write(&path, VALID).unwrap();
    let loaded = load_config_cached_at(&path, data.path(), &Mutex::new(None)).unwrap();
    assert_rotated(data.path(), &loaded);
    assert!(data.path().join(".secrets").is_file());
    assert!(!config.path().join(".secrets").exists());
}

fn assert_rotated(data_root: &Path, config: &AppConfig) {
    assert_ne!(config.api.token.as_deref(), Some("tok-original"));
    assert!(config.api.token.is_some());
    assert_eq!(
        config.api.token,
        codeterm_util::instance_token::read_at(data_root).unwrap()
    );
}

#[test]
fn migration_reuses_persisted_rotation_and_preserves_legacy_backups() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let file = config_file(&path);
    file.save(VALID).unwrap();
    let legacy_backup = file.backups().pop().unwrap();
    let rotated = codeterm_util::instance_token::migrate_at(tmp.path(), "tok-original").unwrap();
    let loaded = load_config_cached(&path, &Mutex::new(None));
    assert_eq!(loaded.api.token.as_deref(), Some(rotated.as_str()));
    assert_eq!(std::fs::read_to_string(legacy_backup).unwrap(), VALID);
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("tok-original"));
    assert_rotated(tmp.path(), &loaded);
}

#[test]
fn failed_migration_config_write_keeps_runtime_authority_and_holds_writes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    std::fs::write(&path, VALID).unwrap();
    let file = config_file(&path);
    let mut config = toml::from_str::<AppConfig>(VALID).unwrap();
    apply_instance_token(
        root.path(),
        &mut config,
        VALID,
        TokenPolicy::GenerateWhenMissing,
        None,
    )
    .unwrap();
    let authority = config.api.token.clone();
    persist_migrated_config(&file, &config, |_| Err("injected write failure".into()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), VALID);
    assert_eq!(config.api.token, authority);
    assert_eq!(
        codeterm_util::instance_token::read_at(root.path()).unwrap(),
        authority
    );
    assert!(file.held().is_some());
    assert!(file.save("[api]\nenabled = true\n").is_err());
    assert!(incidents_for(&path)
        .iter()
        .any(|incident| incident.error.contains("injected write failure")));
}

#[test]
fn startup_does_not_rewrite_retired_config_or_override_explicit_kpi_opt_out() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let original = "[agent]\nresume_skip_permissions = true\n[kpi]\nenabled = false\n";
    std::fs::write(&path, &original).unwrap();
    let config = load_config_cached_at(&path, root.path(), &Mutex::new(None)).unwrap();
    assert_eq!(config.agent.full_access, None);
    assert!(!config.kpi.enabled);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}
