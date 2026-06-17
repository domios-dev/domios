//! Per-plugin self-documenting YAML config (plan §1.7).
//!
//! Each plugin owns a `config.yaml` in its bundle dir whose inline comments are
//! the agent-facing docs for every field. YAML is the *storage/authoring* format
//! only: the host parses it to JSON and hands the plugin that JSON via
//! `host.settingsJson()` (QuickJS plugins still `JSON.parse`). Secret-typed
//! fields are routed to the secret store, never written plaintext into the YAML,
//! so what lands here is non-secret.

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

/// Resolve the settings JSON the host delivers to the plugin VM.
///
/// `config.yaml` in the bundle dir wins (parsed YAML → JSON). When absent, fall
/// back to the legacy `config.plugins.<id>` JSON bag so nothing regresses while
/// plugins migrate. Missing/empty → `"null"`.
pub fn resolve_settings_json(bundle_dir: &Path, plugin_id: &str) -> String {
    let yaml_path = config_yaml_path(bundle_dir);
    if let Ok(text) = std::fs::read_to_string(&yaml_path) {
        match yaml_to_json(&text) {
            Ok(json) => return json.to_string(),
            Err(e) => log::warn!("[plugin/{plugin_id}] {e} — falling back to config.plugins"),
        }
    }
    crate::config::load_config()
        .plugins
        .get(plugin_id)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
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
}
