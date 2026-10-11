use super::*;
use crate::js_runtime::{hot_reload::renderable_surfaces, PluginManifest};

fn assert_browser_bundle(root: &std::path::Path) {
    let dir = root.join("browser");
    assert_eq!(
        crate::bundle_hash::hash_bundle(&dir).unwrap(),
        crate::system_plugins::lookup("browser").unwrap()
    );
    let manifest: PluginManifest =
        serde_json::from_slice(&std::fs::read(dir.join("plugin.json")).unwrap()).unwrap();
    assert!(dir.join(&manifest.entry).is_file());
    let surfaces = renderable_surfaces(&manifest.capabilities);
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].id, "browser");
    assert_eq!(surfaces[0].stream_runtime(), Some("browser"));
}

#[test]
fn browser_clean_install_ships_renderable_stream() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(bootstrap_into(tmp.path())
        .unwrap()
        .contains(&"browser".into()));
    assert_browser_bundle(tmp.path());
    assert!(bootstrap_into(tmp.path()).unwrap().is_empty());
}

#[test]
fn browser_upgrade_replaces_older_release() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("browser");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        br#"{"id":"browser","version":"1.0.0","entry":"plugin.js"}"#,
    )
    .unwrap();
    assert!(bootstrap_into(tmp.path())
        .unwrap()
        .contains(&"browser".into()));
    assert_browser_bundle(tmp.path());
}

#[test]
fn browser_upgrade_repairs_old_same_version_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("browser");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        br#"{"id":"browser","version":"1.12.4","entry":"plugin.js"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("plugin.js"), "module.exports.default = {};").unwrap();
    assert!(bootstrap_into(tmp.path())
        .unwrap()
        .contains(&"browser".into()));
    assert_browser_bundle(tmp.path());
}

#[test]
fn browser_upgrade_preserves_newer_channel_release() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("browser");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = br#"{"id":"browser","version":"9999.0.0"}"#;
    std::fs::write(dir.join("plugin.json"), manifest).unwrap();
    assert!(!bootstrap_into(tmp.path())
        .unwrap()
        .contains(&"browser".into()));
    assert_eq!(std::fs::read(dir.join("plugin.json")).unwrap(), manifest);
}
