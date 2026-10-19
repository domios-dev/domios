use super::*;
use crate::PluginCtx;

#[test]
fn retirement_decision_is_installed_official_set_minus_current_channel_set() {
    let retired_id = retired_id();
    let current_id = current_system_id();
    let installed = BTreeSet::from([current_id.clone(), retired_id.clone()]);
    let current_channel = BTreeSet::from([current_id]);

    assert_eq!(
        system_plugins_to_retire(&installed, &current_channel),
        vec![retired_id]
    );
}

#[test]
fn embedded_retired_ids_are_reserved_official_ids() {
    let id = retired_id();
    assert!(retired_system_plugin_ids().contains(&id));
    assert!(is_reserved_system_id(&id));
    assert!(!is_reserved_system_id("user-plugin"));
}

#[test]
fn retirement_decision_removes_a_retired_legacy_copy_without_an_install_record() {
    let id = retired_id();
    assert!(should_retire_plugin(
        &id,
        Some(&id),
        None,
        &BTreeSet::new(),
        &BTreeSet::from([id.clone()]),
    ));
}

#[test]
fn retirement_decision_removes_a_retired_system_channel_install() {
    let id = retired_id();
    let record = install_record(
        &id,
        InstallSource::SystemChannel {
            channel: SYSTEM_CHANNEL_NAME.into(),
            path: id.clone(),
        },
    );

    assert!(should_retire_plugin(
        &id,
        Some(&id),
        Some(&record),
        &BTreeSet::new(),
        &BTreeSet::from([id.clone()]),
    ));
}

#[test]
fn retirement_decision_preserves_an_explicit_local_directory_install() {
    let id = retired_id();
    let record = install_record(
        &id,
        InstallSource::Dir {
            path: "local-plugin".into(),
        },
    );

    assert!(!should_retire_plugin(
        &id,
        Some(&id),
        Some(&record),
        &BTreeSet::new(),
        &BTreeSet::from([id.clone()]),
    ));
}

#[test]
fn retirement_decision_keeps_non_retired_plugins_and_identity_mismatches() {
    let retired_id = retired_id();
    let retired_ids = BTreeSet::from([retired_id.clone()]);

    assert!(!should_retire_plugin(
        &retired_id,
        Some("another-plugin"),
        None,
        &BTreeSet::new(),
        &retired_ids,
    ));
    assert!(!should_retire_plugin(
        "another-plugin",
        Some("another-plugin"),
        None,
        &BTreeSet::new(),
        &retired_ids,
    ));
}

#[test]
fn retirement_rejects_ids_outside_the_embedded_official_set() {
    assert!(validate_retired_ids(&BTreeSet::new(), &["user-plugin".to_string()]).is_err());
}

#[test]
fn retirement_removes_only_authorized_plugin_directories_and_preserves_data() {
    let root = std::env::temp_dir().join(format!(
        "codeterm-plugin-retirement-{}",
        uuid::Uuid::new_v4()
    ));
    let official_data = root.join("official-data");
    std::fs::create_dir_all(&official_data).unwrap();
    write_installed_plugin(
        &root,
        "retired-official",
        InstallSource::SystemChannel {
            channel: SYSTEM_CHANNEL_NAME.into(),
            path: "retired-official".into(),
        },
        &official_data,
    );
    write_legacy_plugin(&root, "legacy-retired", &official_data);
    for (id, source) in [
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
    let current_ids = ["kept-official".to_string()];
    let retired_ids = [
        "retired-official".to_string(),
        "legacy-retired".to_string(),
        "kept-directory-install".to_string(),
        "kept-marketplace-install".to_string(),
    ];
    let retired = retire_missing_official_plugins(&current_ids, &retired_ids).unwrap();

    assert_eq!(retired, vec!["legacy-retired", "retired-official"]);
    assert!(!root.join("plugins/retired-official").exists());
    assert!(!root.join("plugins/legacy-retired").exists());
    assert!(root.join("plugins/kept-directory-install").is_dir());
    assert!(root.join("plugins/kept-marketplace-install").is_dir());
    assert!(official_data.is_dir());

    assert!(retire_missing_official_plugins(&current_ids, &retired_ids)
        .unwrap()
        .is_empty());
    let events = std::fs::read_to_string(root.join("logs/plugin-events.log")).unwrap();
    for id in ["retired-official", "legacy-retired"] {
        let count = events
            .lines()
            .filter(|line| {
                let event: serde_json::Value = serde_json::from_str(line).unwrap();
                event["kind"] == "retire" && event["plugin"] == id
            })
            .count();
        assert_eq!(count, 1, "expected one retirement audit for {id}");
    }
    assert!(events.lines().any(|line| {
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        event["kind"] == "retire-preserved-install-record"
            && event["plugin"] == "kept-directory-install"
    }));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_uses_embedded_retired_ids_for_legacy_copies() {
    let root = std::env::temp_dir().join(format!(
        "codeterm-plugin-startup-retirement-{}",
        uuid::Uuid::new_v4()
    ));
    let id = retired_id();
    let data_path = root.join(format!("{id}-data"));
    std::fs::create_dir_all(&data_path).unwrap();
    write_legacy_plugin(&root, &id, &data_path);

    let retired = crate::retire_unshipped_system_plugins(PluginCtx {
        data_root: root.clone(),
        version: None,
    })
    .unwrap();

    assert_eq!(retired, vec![id.clone()]);
    assert!(!root.join("plugins").join(&id).exists());
    assert!(data_path.is_dir());
    std::fs::remove_dir_all(root).unwrap();
}

fn retired_id() -> String {
    retired_system_plugin_ids()
        .first()
        .expect("embedded channel declares a retired id")
        .clone()
}

fn current_system_id() -> String {
    SYSTEM_PLUGIN_IDS
        .iter()
        .find(|id| {
            !retired_system_plugin_ids()
                .iter()
                .any(|retired| retired == *id)
        })
        .expect("embedded channel declares a current id")
        .to_string()
}

fn install_record(id: &str, source: InstallSource) -> InstallRecord {
    InstallRecord {
        id: id.into(),
        version: Some("0.1.0".into()),
        source,
        installed_at: "2026-10-18T00:00:00Z".into(),
        last_change: None,
    }
}

fn write_installed_plugin(
    root: &std::path::Path,
    id: &str,
    source: InstallSource,
    data_path: &std::path::Path,
) {
    let dir = write_plugin_directory(root, id, data_path);
    std::fs::write(
        dir.join(".codeterm-install.json"),
        serde_json::to_string(&install_record(id, source)).unwrap(),
    )
    .unwrap();
}

fn write_legacy_plugin(root: &std::path::Path, id: &str, data_path: &std::path::Path) {
    write_plugin_directory(root, id, data_path);
}

fn write_plugin_directory(
    root: &std::path::Path,
    id: &str,
    data_path: &std::path::Path,
) -> std::path::PathBuf {
    let dir = root.join("plugins").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::json!({
            "id": id,
            "version": "0.1.0",
            "dataPaths": [data_path.display().to_string()]
        })
        .to_string(),
    )
    .unwrap();
    dir
}
