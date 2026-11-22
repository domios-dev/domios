//! Plugins UI settings: the same `config.yaml` that `plugin config` edits and the
//! host stamps into `host.settingsJson()`. There is no second copy in `config.json`.

use std::path::{Path, PathBuf};

use codeterm_plugins::{PluginAction, PluginCtx};
use serde_json::{Map, Value};

/// The settings exactly as the host hands them to the plugin VM.
pub(super) fn read(plugin_id: &str) -> Result<Value, String> {
    Ok(settings_response(&bundle_dir(plugin_id)?, plugin_id))
}

/// Writes through `plugin config --values-json`, pushes the new settings to the live VM,
/// and answers with the host's view.
pub(super) fn write(plugin_id: &str, values: Map<String, Value>) -> Result<Value, String> {
    let dir = bundle_dir(plugin_id)?;
    write_with(super::cli::plugin_ctx(), plugin_id, values)?;
    let json = codeterm_plugin::js_runtime::plugin_config::resolve_settings_json(&dir, plugin_id);
    codeterm_plugin::js_runtime::hot_reload::reload_plugin_settings(plugin_id, json.clone());
    Ok(settings_value(&json))
}

fn write_with(
    ctx: PluginCtx,
    plugin_id: &str,
    values: Map<String, Value>,
) -> Result<Value, String> {
    codeterm_plugins::run(
        ctx,
        PluginAction::Config {
            id: plugin_id.to_string(),
            set: Vec::new(),
            values_json: Some(Value::Object(values).to_string()),
            secret: None,
            grant: Vec::new(),
            revoke: Vec::new(),
        },
    )
}

fn settings_response(dir: &Path, plugin_id: &str) -> Value {
    let json = codeterm_plugin::js_runtime::plugin_config::resolve_settings_json(dir, plugin_id);
    settings_value(&json)
}

fn settings_value(json: &str) -> Value {
    let settings = serde_json::from_str::<Value>(json).unwrap_or(Value::Null);
    serde_json::json!({ "settings": settings })
}

/// First root wins, matching the order the loader and the snapshot scan use.
fn bundle_dir(plugin_id: &str) -> Result<PathBuf, String> {
    super::snapshot::plugin_roots()
        .into_iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .find(|dir| manifest_id(dir).as_deref() == Some(plugin_id))
        .ok_or_else(|| format!("plugin '{plugin_id}' is not loaded"))
}

fn manifest_id(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("plugin.json")).ok()?;
    let manifest: Value = serde_json::from_str(&text).ok()?;
    manifest.get("id")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests;
