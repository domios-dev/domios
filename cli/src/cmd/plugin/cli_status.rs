use codeterm_plugin::provider_cli::manifest_cli_status;
use std::path::Path;

pub(super) fn attach(value: &mut serde_json::Value, root: &Path, requested: Option<&str>) {
    let mut ids: Vec<String> = value
        .get("updated")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    if let Some(id) = value.get("id").and_then(|v| v.as_str()).or(requested) {
        ids.push(id.into());
    }
    ids.sort();
    ids.dedup();
    let mut statuses = serde_json::Map::new();
    let mut errors = Vec::new();
    for id in ids {
        let path = root.join("plugins").join(&id).join("plugin.json");
        let result = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<codeterm_plugin::js_runtime::manifest::PluginManifest>(
                    &bytes,
                )
                .map_err(|e| e.to_string())
            });
        match result {
            Ok(manifest) => {
                if let Some(status) = manifest_cli_status(
                    &manifest,
                    codeterm_plugin::provider::current_platform_tag(),
                ) {
                    statuses.insert(id, serde_json::to_value(status).unwrap_or_default());
                }
            }
            Err(error) => errors.push(format!("CLI status for {id} could not be read: {error}")),
        }
    }
    value["cli_status"] = serde_json::Value::Object(statuses);
    value["cli_status_warnings"] = serde_json::json!(errors);
}

pub(super) fn print(value: &serde_json::Value) {
    if let Some(statuses) = value.get("cli_status").and_then(|v| v.as_object()) {
        for (id, status) in statuses {
            for line in status_lines(id, status) {
                println!("{line}");
            }
        }
    }
    if let Some(warnings) = value.get("cli_status_warnings").and_then(|v| v.as_array()) {
        for warning in warnings.iter().filter_map(|v| v.as_str()) {
            eprintln!("WARNING: {warning}");
        }
    }
}

pub(crate) fn status_lines(id: &str, status: &serde_json::Value) -> Vec<String> {
    if status.get("required").and_then(|v| v.as_bool()) == Some(false) {
        return vec![format!("{id}: no CLI required")];
    }
    let binary = status.get("binary").and_then(|v| v.as_str()).unwrap_or("?");
    match status.get("installed").and_then(|v| v.as_bool()) {
        Some(true) => vec![format!(
            "{id}: CLI {binary} installed ({})",
            status
                .get("resolved_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
        )],
        Some(false) => {
            let mut lines = vec![format!("WARNING: {id}: CLI `{binary}` is missing")];
            if let Some(command) = status.get("installer").and_then(|v| v.as_str()) {
                lines.push(format!("Install command: {command}"));
                lines.push(format!("fix: codeterm agent cli install {id}"));
            } else {
                lines.push(
                    "WARNING: No installer declared for this OS; install the CLI manually.".into(),
                );
            }
            lines
        }
        None => vec![format!(
            "WARNING: {id}: CLI `{binary}` availability unknown"
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_manifest_drives_offline_plugin_reporting() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("plugins").join("fixture");
        std::fs::create_dir_all(&directory).unwrap();
        let platform = codeterm_plugin::provider::current_platform_tag();
        let manifest = serde_json::json!({
            "id": "fixture", "version": "1.0.0", "displayName": "Fixture",
            "capabilities": {"agentProvider": true},
            "binaryName": "codeterm-intentionally-missing-provider-report-cli",
            "installer": { (platform): "opaque installer" },
            "updater": { (platform): "opaque updater" }
        });
        std::fs::write(directory.join("plugin.json"), manifest.to_string()).unwrap();
        let mut result = serde_json::json!({"ok": true, "id": "fixture"});
        attach(&mut result, root.path(), None);
        assert_eq!(result["cli_status"]["fixture"]["installed"], false);
        assert_eq!(
            result["cli_status"]["fixture"]["installer"],
            "opaque installer"
        );
        assert_eq!(result["cli_status"]["fixture"]["updater"], "opaque updater");
        let mut update = serde_json::json!({"ok": true, "updated": ["fixture"]});
        attach(&mut update, root.path(), None);
        assert_eq!(update["cli_status"], result["cli_status"]);
    }

    #[test]
    fn missing_cli_preserves_installer_and_names_management_verb() {
        let lines = status_lines(
            "fixture",
            &serde_json::json!({"required": true, "binary": "tool", "installed": false, "installer": "opaque --install 'token'"}),
        );
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with("opaque --install 'token'"));
        assert!(lines[2].ends_with("codeterm agent cli install fixture"));
    }
    #[test]
    fn no_cli_and_unknown_do_not_report_missing() {
        assert_eq!(
            status_lines("fixture", &serde_json::json!({"required": false})),
            vec!["fixture: no CLI required"]
        );
        assert_eq!(
            status_lines(
                "fixture",
                &serde_json::json!({"required": true, "binary": "tool", "installed": null})
            )
            .len(),
            1
        );
    }
}
