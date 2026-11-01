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

pub fn resolve_settings_json(bundle_dir: &Path, plugin_id: &str) -> String {
    let yaml_text = std::fs::read_to_string(config_yaml_path(bundle_dir, plugin_id)).ok();
    resolve_settings_inner(yaml_text.as_deref(), plugin_id)
}

fn resolve_settings_inner(yaml_text: Option<&str>, plugin_id: &str) -> String {
    match yaml_text.map(yaml_to_json).transpose() {
        Ok(Some(json)) => json.to_string(),
        Ok(None) => "null".to_string(),
        Err(error) => {
            log::warn!("[plugin/{plugin_id}] {error}");
            "null".to_string()
        }
    }
}

#[cfg(test)]
mod tests;
