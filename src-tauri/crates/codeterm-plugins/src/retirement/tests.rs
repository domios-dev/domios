use super::*;
use crate::PluginCtx;

#[test]
fn retirement_decision_is_installed_official_set_minus_current_channel_set() {
    let installed = BTreeSet::from(["codex".to_string(), "codex-cloud".to_string()]);
    let current_channel = BTreeSet::from(["codex".to_string()]);

    assert_eq!(
        system_plugins_to_retire(&installed, &current_channel),
        vec!["codex-cloud".to_string()]
    );
}

#[test]
fn retirement_removes_only_official_bundle_and_preserves_plugin_data() {
    let root = std::env::temp_dir().join(format!(
        "codeterm-plugin-retirement-{}",
        uuid::Uuid::new_v4()
    ));
    let official_data = root.join("official-data");
    std::fs::create_dir_all(&official_data).unwrap();
    for (id, source) in [
        (
            "retired-official",
            InstallSource::SystemChannel {
                channel: SYSTEM_CHANNEL_NAME.into(),
                path: "retired-official".into(),
            },
        ),
        (
            "kept-directory-install",
            InstallSource::Dir {
                path: "local-plugin".into(),
            },
        ),
        (
            "kept-marketplace-install",
            InstallSource::Channel {
                channel: "codeterm-plugins".into(),
                path: "marketplace-plugin".into(),
            },
        ),
    ] {
        write_installed_plugin(&root, id, source, &official_data);
    }

    let _context = crate::enter_ctx(PluginCtx {
        data_root: root.clone(),
        version: None,
    });
    let retired = retire_missing_official_plugins(&["kept-official".to_string()]).unwrap();

    assert_eq!(retired, vec!["retired-official"]);
    assert!(!root.join("plugins/retired-official").exists());
    assert!(root.join("plugins/kept-directory-install").is_dir());
    assert!(root.join("plugins/kept-marketplace-install").is_dir());
    assert!(official_data.is_dir());

    let events = std::fs::read_to_string(root.join("logs/plugin-events.log")).unwrap();
    let retirement_events: Vec<_> = events
        .lines()
        .filter(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            event["kind"] == "retire" && event["plugin"] == "retired-official"
        })
        .collect();
    assert_eq!(retirement_events.len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

fn write_installed_plugin(
    root: &std::path::Path,
    id: &str,
    source: InstallSource,
    data_path: &std::path::Path,
) {
    let dir = root.join("plugins").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::json!({
            "id":id,
            "version":"0.1.0",
            "dataPaths":[data_path.display().to_string()]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join(".codeterm-install.json"),
        serde_json::to_string(&InstallRecord {
            id: id.into(),
            version: Some("0.1.0".into()),
            source,
            installed_at: "2026-10-18T00:00:00Z".into(),
            last_change: None,
        })
        .unwrap(),
    )
    .unwrap();
}
