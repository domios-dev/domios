//! Per-plugin self-documenting `config.yaml` (inline comments are the agent docs).
//! YAML is the authoring format only — the host parses it to JSON for
//! `host.settingsJson()`; secret-typed fields route to the secret store, never written here.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The plugin's authoring config file: the same path `plugin config` writes.
pub fn config_yaml_path(bundle_dir: &Path, plugin_id: &str) -> PathBuf {
    config_yaml_path_in(
        &codeterm_paths::data_dir().join("plugins"),
        bundle_dir,
        plugin_id,
    )
}

fn config_yaml_path_in(user_plugins_root: &Path, bundle_dir: &Path, plugin_id: &str) -> PathBuf {
    codeterm_util::plugin_config::config_yaml_path(user_plugins_root, plugin_id, bundle_dir)
}

/// Parse a YAML document into a JSON value. An empty document is `null`.
pub fn yaml_to_json(yaml: &str) -> Result<Value, String> {
    if yaml.trim().is_empty() {
        return Ok(Value::Null);
    }
    let v: serde_yaml::Value =
        serde_yaml::from_str(yaml).map_err(|e| format!("parse config.yaml: {e}"))?;
    serde_json::to_value(v).map_err(|e| format!("config.yaml → json: {e}"))
}

/// Resolve the settings JSON delivered to the plugin VM: `config.yaml` wins, else
/// the legacy `config.plugins.<id>` bag (migration safety), else `"null"`.
pub fn resolve_settings_json(bundle_dir: &Path, plugin_id: &str) -> String {
    let yaml_text = std::fs::read_to_string(config_yaml_path(bundle_dir, plugin_id)).ok();
    // Legacy config.plugins bag is not available in this crate; YAML wins.
    resolve_settings_inner(yaml_text.as_deref(), plugin_id, || None)
}

/// Pure core of [`resolve_settings_json`] (inputs injected for testability). A YAML
/// parsing to `null` (empty/comments-only) is treated as absent so the legacy bag isn't clobbered.
fn resolve_settings_inner(
    yaml_text: Option<&str>,
    plugin_id: &str,
    legacy: impl FnOnce() -> Option<String>,
) -> String {
    if let Some(text) = yaml_text {
        match yaml_to_json(text) {
            Ok(Value::Null) => {} // empty/comments-only → fall through to legacy
            Ok(json) => return json.to_string(),
            Err(e) => log::warn!("[plugin/{plugin_id}] {e} — falling back to config.plugins"),
        }
    }
    legacy().unwrap_or_else(|| "null".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_to_json_maps_scalars_and_nested() {
        let json = yaml_to_json("engine: whisper\nport: 8080\nnested:\n  a: 1\n").unwrap();
        assert_eq!(json["engine"], "whisper");
        assert_eq!(json["port"], 8080);
        assert_eq!(json["nested"]["a"], 1);
    }

    #[test]
    fn yaml_empty_is_null() {
        assert_eq!(yaml_to_json("").unwrap(), Value::Null);
        assert_eq!(yaml_to_json("   \n").unwrap(), Value::Null);
    }

    #[test]
    fn config_yaml_wins_over_legacy_bag() {
        let dir = tempfile::tempdir().unwrap();
        let id = "transcribe-fixture-q9";
        std::fs::write(
            config_yaml_path(dir.path(), id),
            "# the engine to use\nengine: whisper\n",
        )
        .unwrap();
        let json = resolve_settings_json(dir.path(), id);
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["engine"], "whisper");
    }

    #[test]
    fn missing_config_yaml_resolves_null_or_legacy() {
        let dir = tempfile::tempdir().unwrap();
        // No config.yaml and (in test) no matching config.plugins entry → "null".
        let json = resolve_settings_json(dir.path(), "no-such-plugin-xyz");
        assert_eq!(json, "null");
    }

    #[test]
    fn workspace_bundle_reads_the_config_written_to_its_installed_copy() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("plugins");
        let installed = user.join("codex");
        let workspace_bundle = root.path().join("repo/plugins/provider-codex");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::create_dir_all(&workspace_bundle).unwrap();
        std::fs::write(installed.join("plugin.json"), "{}").unwrap();
        std::fs::write(installed.join("config.yaml"), "engine: user\n").unwrap();
        std::fs::write(workspace_bundle.join("config.yaml"), "engine: stale\n").unwrap();

        let path = config_yaml_path_in(&user, &workspace_bundle, "codex");

        assert_eq!(path, installed.join("config.yaml"));
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(yaml_to_json(&text).unwrap()["engine"], "user");
    }

    #[test]
    fn real_yaml_wins_and_skips_legacy() {
        let out = resolve_settings_inner(Some("engine: whisper\n"), "p", || {
            panic!("legacy must not be consulted when config.yaml has content")
        });
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["engine"], "whisper");
    }

    #[test]
    fn comments_only_config_yaml_falls_through_to_legacy() {
        // A comments-only config.yaml parses to null and must not clobber the legacy bag.
        let legacy = || Some(r#"{"engine":"legacy"}"#.to_string());
        let out = resolve_settings_inner(Some("# only a comment\n"), "p", legacy);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["engine"], "legacy",
            "must fall through to legacy, not 'null'"
        );

        // No legacy entry either → "null".
        let out2 = resolve_settings_inner(Some("# just a comment\n"), "p", || None);
        assert_eq!(out2, "null");
    }
}
