use super::*;
use std::path::Path;

fn with_data_root<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _guard = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let previous_data_dir = std::env::var("CODETERM_DATA_DIR").ok();
    let previous_system_url = std::env::var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL").ok();
    let root = std::env::temp_dir().join(format!(
        "codeterm-plugin-system-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::env::set_var("CODETERM_DATA_DIR", &root);
    let result = f(&root);
    restore_env("CODETERM_DATA_DIR", previous_data_dir);
    restore_env("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", previous_system_url);
    let _ = std::fs::remove_dir_all(root);
    result
}

fn restore_env(key: &str, value: Option<String>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

fn write_channel(dir: &Path, name: &str, plugins: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    let entries = plugins
        .iter()
        .map(|(id, path)| serde_json::json!({"id": id, "version": "1.0.0", "path": path}))
        .collect::<Vec<_>>();
    std::fs::write(
        dir.join("channel.json"),
        serde_json::json!({"schemaVersion": 1, "name": name, "plugins": entries}).to_string(),
    )
    .unwrap();
    for (id, path) in plugins {
        let plugin_dir = dir.join(path);
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::json!({
                "id": id,
                "version": "1.0.0",
                "displayName": id,
                "main": "plugin.js"
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(plugin_dir.join("plugin.js"), "module.exports = {};").unwrap();
    }
}

fn set_system_channel_source(root: &Path, plugins: &[(&str, &str)]) {
    let source = root.join("system-source");
    write_channel(&source, SYSTEM_CHANNEL_NAME, plugins);
    std::env::set_var(
        "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
        format!("file://{}", source.display()),
    );
}

fn diff_ids(diff: &serde_json::Value) -> Vec<String> {
    diff["diff"]
        .as_array()
        .expect("diff rows")
        .iter()
        .map(|row| row["id"].as_str().expect("plugin id").to_string())
        .collect()
}

fn audit_has_channel_event(kind: &str, channel: &str, ok: bool) -> bool {
    std::fs::read_to_string(audit_log_path())
        .expect("audit log")
        .lines()
        .any(|line| {
            let event: serde_json::Value = serde_json::from_str(line).expect("audit event");
            event["kind"] == kind && event["channel"] == channel && event["ok"] == ok
        })
}

#[test]
fn system_channel_named_diff_materializes_when_unregistered() {
    with_data_root(|root| {
        set_system_channel_source(root, &[("core-plugin", "provider-core")]);

        let diff = channel_diff(Some(SYSTEM_CHANNEL_NAME)).expect("named core diff");

        assert_eq!(diff_ids(&diff), vec!["core-plugin".to_string()]);
        let mut registry = read_channel_registry().unwrap();
        let record = registry
            .channels
            .remove(SYSTEM_CHANNEL_NAME)
            .expect("core registry entry");
        assert!(Path::new(&record.path).is_dir());
        assert_eq!(record.kind, ChannelKind::Local);
    });
}

#[test]
fn system_channel_aggregate_diff_includes_core() {
    with_data_root(|root| {
        set_system_channel_source(root, &[("core-plugin", "provider-core")]);
        assert!(read_channel_registry().unwrap().channels.is_empty());

        let diff = channel_diff(None).expect("aggregate diff");

        assert_eq!(diff_ids(&diff), vec!["core-plugin".to_string()]);
        assert!(read_channel_registry()
            .unwrap()
            .channels
            .contains_key(SYSTEM_CHANNEL_NAME));
    });
}

#[test]
fn system_channel_aggregate_diff_skips_failed_materialization() {
    with_data_root(|root| {
        let missing_source = root.join("missing-system-source");
        std::env::set_var(
            "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
            format!("file://{}", missing_source.display()),
        );
        let healthy_source = root.join("healthy-source");
        write_channel(
            &healthy_source,
            "healthy",
            &[("healthy-plugin", "provider-good")],
        );
        add_channel(
            &format!("file://{}", healthy_source.display()),
            Some("healthy".to_string()),
        )
        .expect("healthy channel registration");

        let diff = channel_diff(None).expect("aggregate diff keeps healthy channel");

        assert_eq!(diff_ids(&diff), vec!["healthy-plugin".to_string()]);
        assert!(audit_has_channel_event(
            "diff-skipped",
            SYSTEM_CHANNEL_NAME,
            false
        ));
    });
}

#[test]
fn system_channel_named_diff_returns_materialization_error() {
    with_data_root(|root| {
        let missing_source = root.join("missing-system-source");
        std::env::set_var(
            "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
            format!("file://{}", missing_source.display()),
        );

        let error = channel_diff(Some(SYSTEM_CHANNEL_NAME)).expect_err("missing fixture fails");

        assert!(!error.contains("not registered"), "{error}");
        assert!(
            error.contains(&missing_source.display().to_string()),
            "{error}"
        );
    });
}

#[test]
fn system_channel_named_refresh_materializes() {
    with_data_root(|root| {
        set_system_channel_source(root, &[("core-plugin", "provider-core")]);

        refresh_channel(Some(SYSTEM_CHANNEL_NAME)).expect("named core refresh");

        let mut registry = read_channel_registry().unwrap();
        let record = registry
            .channels
            .remove(SYSTEM_CHANNEL_NAME)
            .expect("core registry entry");
        let manifest = read_channel_manifest(Path::new(&record.path)).expect("refreshed cache");
        assert_eq!(manifest.name, SYSTEM_CHANNEL_NAME);
        assert_eq!(manifest.plugins[0].id, "core-plugin");
    });
}

#[test]
fn system_channel_aggregate_refresh_materializes_core() {
    with_data_root(|root| {
        set_system_channel_source(root, &[("core-plugin", "provider-core")]);

        refresh_channel(None).expect("aggregate refresh");

        let mut registry = read_channel_registry().unwrap();
        let record = registry
            .channels
            .remove(SYSTEM_CHANNEL_NAME)
            .expect("core registry entry");
        assert!(Path::new(&record.path).is_dir());
    });
}

#[test]
fn system_channel_named_refresh_returns_materialization_error() {
    with_data_root(|root| {
        let missing_source = root.join("missing-system-source");
        std::env::set_var(
            "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
            format!("file://{}", missing_source.display()),
        );

        let error = refresh_channel(Some(SYSTEM_CHANNEL_NAME)).expect_err("missing fixture fails");

        assert!(!error.contains("not registered"), "{error}");
        assert!(
            error.contains(&missing_source.display().to_string()),
            "{error}"
        );
    });
}

#[test]
fn system_channel_aggregate_refresh_skips_failed_materialization() {
    with_data_root(|root| {
        let missing_source = root.join("missing-system-source");
        std::env::set_var(
            "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
            format!("file://{}", missing_source.display()),
        );
        let healthy_source = root.join("healthy-source");
        write_channel(
            &healthy_source,
            "healthy",
            &[("healthy-plugin", "provider-good")],
        );
        add_channel(
            &format!("file://{}", healthy_source.display()),
            Some("healthy".to_string()),
        )
        .expect("healthy channel registration");
        write_channel(
            &healthy_source,
            "healthy",
            &[("refreshed-plugin", "provider-new")],
        );

        refresh_channel(None).expect("aggregate refresh skips core failure");

        let registry = read_channel_registry().unwrap();
        let healthy = registry.channels.get("healthy").expect("healthy channel");
        let manifest = read_channel_manifest(Path::new(&healthy.path)).expect("refreshed cache");
        assert_eq!(manifest.plugins[0].id, "refreshed-plugin");
        assert!(audit_has_channel_event(
            "refresh-failed",
            SYSTEM_CHANNEL_NAME,
            false
        ));
    });
}

#[test]
fn system_channel_list_stays_virtual_without_network() {
    with_data_root(|root| {
        let missing_source = root.join("missing-system-source");
        std::env::set_var(
            "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
            format!("file://{}", missing_source.display()),
        );

        let listed = list_channels().expect("list stays available");

        let core = listed["channels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|channel| channel["name"] == SYSTEM_CHANNEL_NAME)
            .expect("virtual core row");
        assert_eq!(core["path"], "");
        assert_eq!(core["system"], true);
        assert_eq!(core["removable"], false);
        assert!(read_channel_registry().unwrap().channels.is_empty());
        assert!(!channels_root().exists());
    });
}
