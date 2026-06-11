use super::*;

pub(super) fn audit_plugin_result(
    kind: &str,
    plugin_id: Option<&str>,
    channel: Option<&str>,
    from: Option<String>,
    to: Option<String>,
    result: &Result<(), String>,
) {
    let event = PluginEvent {
        ts: now_rfc3339(),
        kind: event_kind(kind, result),
        actor: "cli".to_string(),
        ok: result.is_ok(),
        plugin: plugin_id.map(ToOwned::to_owned),
        channel: channel.map(ToOwned::to_owned),
        from,
        to: to.clone(),
        version: to,
        error: result.as_ref().err().cloned(),
    };
    let _ = append_audit_event(&event);
}

pub(super) fn audit_channel_result(kind: &str, channel: Option<&str>, result: &Result<(), String>) {
    let event = PluginEvent {
        ts: now_rfc3339(),
        kind: event_kind(kind, result),
        actor: "cli".to_string(),
        ok: result.is_ok(),
        plugin: None,
        channel: channel.map(ToOwned::to_owned),
        from: None,
        to: None,
        version: None,
        error: result.as_ref().err().cloned(),
    };
    let _ = append_audit_event(&event);
}

pub(super) fn audit_drift_detected(
    channel: &str,
    plugin: &str,
    local: Option<String>,
    remote: Option<String>,
    local_manifest_hash: Option<String>,
    remote_manifest_hash: Option<String>,
    drift: &str,
) {
    let event = serde_json::json!({
        "ts": now_rfc3339(),
        "kind": "drift-detected",
        "actor": "cli",
        "ok": true,
        "channel": channel,
        "plugin": plugin,
        "local": local,
        "remote": remote,
        "local_manifest_hash": local_manifest_hash,
        "remote_manifest_hash": remote_manifest_hash,
        "drift": drift,
    });
    let _ = append_audit_value(&event);
}

pub(super) fn audit_command_failure(action: &PluginAction, error: &str) -> Result<(), String> {
    let (kind, plugin, channel) = command_failure_fields(action);
    let event = PluginEvent {
        ts: now_rfc3339(),
        kind,
        actor: "cli".to_string(),
        ok: false,
        plugin,
        channel,
        from: None,
        to: None,
        version: None,
        error: Some(error.to_string()),
    };
    append_audit_event(&event)
}

pub(super) fn command_failure_fields(action: &PluginAction) -> (String, Option<String>, Option<String>) {
    match action {
        PluginAction::List => ("list-failed".to_string(), None, None),
        PluginAction::Install { id, channel, .. } => {
            ("install-failed".to_string(), id.clone(), channel.clone())
        }
        PluginAction::Update { id } => ("update-failed".to_string(), id.clone(), None),
        PluginAction::UpdateSystem { id } => {
            ("update-system-failed".to_string(), id.clone(), Some(SYSTEM_CHANNEL_NAME.to_string()))
        }
        PluginAction::Remove { id, .. } => ("remove-failed".to_string(), Some(id.clone()), None),
        PluginAction::Channel { command } => match command {
            PluginChannelAction::Add { name, .. } => {
                ("channel-add-failed".to_string(), None, name.clone())
            }
            PluginChannelAction::List => ("channel-list-failed".to_string(), None, None),
            PluginChannelAction::Refresh { name } => {
                ("refresh-failed".to_string(), None, name.clone())
            }
            PluginChannelAction::Diff { name } => {
                ("diff-failed".to_string(), None, name.clone())
            }
            PluginChannelAction::Remove { name, .. } => {
                ("channel-remove-failed".to_string(), None, Some(name.clone()))
            }
        },
    }
}

pub(super) fn event_kind(base: &str, result: &Result<(), String>) -> String {
    let kind = if result.is_ok() {
        match base {
            "update" => "update-completed",
            "update_system" => "update-system-completed",
            other => other,
        }
    } else {
        match base {
            "install" => "install-failed",
            "update" => "update-failed",
            "update_system" => "update-system-failed",
            "remove" => "remove-failed",
            "refresh-fetched" => "refresh-failed",
            other => other,
        }
    };
    kind.to_string()
}

pub(super) fn append_audit_event(event: &PluginEvent) -> Result<(), String> {
    let value = serde_json::to_value(event).map_err(|e| e.to_string())?;
    append_audit_value(&value)
}

pub(super) fn append_audit_value(value: &serde_json::Value) -> Result<(), String> {
    let path = audit_log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    writeln!(file, "{line}").map_err(|e| format!("write {}: {e}", path.display()))
}
