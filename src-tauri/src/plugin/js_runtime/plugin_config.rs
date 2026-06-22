//! Per-plugin self-documenting `config.yaml` (inline comments are the agent docs).
//! YAML is the authoring format only — the host parses it to JSON for
//! `host.settingsJson()`; secret-typed fields route to the secret store, never written here.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The plugin's authoring config file inside its bundle dir.
pub fn config_yaml_path(bundle_dir: &Path) -> PathBuf {
    bundle_dir.join("config.yaml")
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
    let yaml_text = std::fs::read_to_string(config_yaml_path(bundle_dir)).ok();
    resolve_settings_inner(yaml_text.as_deref(), plugin_id, || {
        crate::config::load_config()
            .plugins
            .get(plugin_id)
            .map(|v| v.to_string())
    })
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
        std::fs::write(
            config_yaml_path(dir.path()),
            "# the engine to use\nengine: whisper\n",
        )
        .unwrap();
        let json = resolve_settings_json(dir.path(), "transcribe");
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
