use super::super::manifest::PluginManifest;
use super::super::vm::PluginVm;
use super::*;
use std::sync::Arc;

#[test]
fn declared_toml_folder_trust_native_js_parity() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("project");
    std::fs::create_dir(&cwd).unwrap();
    let canonical = std::fs::canonicalize(&cwd).unwrap();
    let canonical = codeterm_paths::strip_verbatim_prefix(&canonical.to_string_lossy());
    let record = temp.path().join("trusted_folders.toml");
    let manifest_json = serde_json::json!({
        "id": "folder-fixture", "version": "0.1.0", "displayName": "Folder fixture", "entry": "plugin.js",
        "directoryTrustRecord": {"format": "toml_folders", "path": record},
        "sessions": {"rootDir": temp.path().join("sessions"), "fileExtension": "jsonl"},
        "spawn": {"readinessStrategy":{"kind":"poll_shell_prompt","timeoutSecs":1},
            "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}}
    }).to_string();
    let manifest: PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let mut outside = manifest.clone();
    outside.directory_trust_record =
        Some(super::super::manifest::DirectoryTrustRecord::TomlFolders {
            path: temp
                .path()
                .parent()
                .unwrap()
                .join("foreign.toml")
                .to_string_lossy()
                .to_string(),
        });
    let outside_vm = PluginVm::from_source_with(
        "outside-fixture",
        "module.exports.default = {};".into(),
        None,
    )
    .unwrap();
    let outside_js = JsProvider::new(outside, Arc::new(outside_vm));
    assert!(outside_js
        .pre_trust_directory(cwd.to_str().unwrap())
        .is_err());
    let vm = PluginVm::from_source_with(
        "folder-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json),
    )
    .unwrap();
    let js = JsProvider::new(manifest, Arc::new(vm));
    for source in ["", "[folders.\"/other\"]\ntrusted = false\n", "malformed"] {
        std::fs::write(&record, source).unwrap();
        let native_read = crate::toml_folder_trust::trusted(&record, &canonical);
        let native = crate::toml_folder_trust::grant(&record, &canonical);
        std::fs::write(&record, source).unwrap();
        assert_eq!(
            js.is_directory_trusted(cwd.to_str().unwrap()).is_ok(),
            native_read.is_ok()
        );
        assert_eq!(
            js.pre_trust_directory(cwd.to_str().unwrap()).unwrap(),
            native
        );
        if native == crate::provider::PreTrustOutcome::Granted {
            assert_eq!(js.is_directory_trusted(cwd.to_str().unwrap()), Ok(true));
            assert!(!crate::toml_folder_trust::trusted(&record, "/other").unwrap());
        }
    }
}
