use serde_json::{json, Value};

const PERMISSIONS: [(&str, &str, &str); 7] = [
    (
        "local_recording",
        "localRecording",
        "Record your selected microphone",
    ),
    (
        "project_write",
        "projectWrite",
        "Write files in selected projects",
    ),
    ("manage_workspaces", "manageWorkspaces", "Manage workspaces"),
    ("spawn_agents", "spawnAgents", "Start agents"),
    ("agent_commands", "agentCommands", "Run agent commands"),
    ("secrets", "secrets", "Access plugin secrets"),
    ("graph_events", "graphEvents", "Publish workflow events"),
];

pub(super) fn declared_permissions(manifest: &Value) -> Vec<(&'static str, &'static str)> {
    PERMISSIONS
        .iter()
        .filter(|(key, camel, _)| {
            let permissions = &manifest["permissions"];
            permissions[*key]
                .as_bool()
                .or_else(|| permissions[*camel].as_bool())
                == Some(true)
                || (*key == "local_recording"
                    && manifest["capabilities"]["localRecordingTrigger"].is_object())
        })
        .map(|(key, _, label)| (*key, *label))
        .collect()
}

pub(super) fn permission_states(
    manifest: &Value,
    grants: &codeterm_util::plugin_grants::PluginGrants,
) -> Value {
    Value::Array(
        declared_permissions(manifest)
            .into_iter()
            .map(|(key, label)| {
                let allowed = grants
                    .permissions
                    .get(key)
                    .copied()
                    .unwrap_or(key != "local_recording" && key != "project_write");
                json!({ "key": key, "label": label, "allowed": allowed, "decided": grants.permissions.contains_key(key) })
            })
            .collect(),
    )
}

pub(super) fn permission_action(
    plugin_id: &str,
    permission: &str,
    allowed: bool,
    source: codeterm_plugin::plugin_lifecycle::PermissionChangeSource,
) -> Result<Value, String> {
    let manifests = super::snapshot::scan_plugin_manifests();
    let (_, manifest, _) = manifests.get(plugin_id).ok_or("Plugin is not installed")?;
    validate_permission(manifest, permission)?;
    let action = if let Some(host) = permission.strip_prefix("network:") {
        let host = codeterm_util::network_host::normalize_network_host(host)?;
        codeterm_plugins::PluginAction::Settings {
            id: plugin_id.to_string(),
            allow_host: if allowed {
                vec![host.clone()]
            } else {
                Vec::new()
            },
            disallow_host: if allowed { Vec::new() } else { vec![host] },
        }
    } else {
        codeterm_plugins::PluginAction::Config {
            id: plugin_id.to_string(),
            set: Vec::new(),
            values_json: None,
            secret: None,
            grant: if allowed {
                vec![permission.to_string()]
            } else {
                Vec::new()
            },
            revoke: if allowed {
                Vec::new()
            } else {
                vec![permission.to_string()]
            },
        }
    };
    let output = codeterm_plugins::run(
        codeterm_plugins::PluginCtx {
            data_root: codeterm_service_core::paths::data_dir(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        },
        action,
    )?;
    codeterm_service_core::plugin::js_runtime::hot_reload::refresh_installed_plugins(&[
        plugin_id.to_string()
    ])?;
    if let Some(event) =
        codeterm_plugin::plugin_lifecycle::permission_changed_event(permission, allowed, source)
    {
        codeterm_service_core::plugin::js_runtime::hot_reload::notify_permission_changed(
            plugin_id, &event,
        )?;
    }
    super::action::invalidate_plugin_snapshot();
    Ok(output)
}

fn validate_permission(manifest: &Value, permission: &str) -> Result<(), String> {
    if let Some(host) = permission.strip_prefix("network:") {
        if !manifest["permissions"]["network"].is_object() {
            return Err("Plugin did not declare network access".into());
        }
        codeterm_util::network_host::normalize_network_host(host)?;
        return Ok(());
    }
    if declared_permissions(manifest)
        .iter()
        .any(|(key, _)| *key == permission)
    {
        Ok(())
    } else {
        Err("Plugin did not declare this sensitive permission".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_require_an_explicit_recording_grant_and_preserve_revocation() {
        let manifest = json!({ "permissions": { "localRecording": true, "spawnAgents": true } });
        let mut grants = codeterm_util::plugin_grants::PluginGrants::default();
        assert_eq!(permission_states(&manifest, &grants)[0]["allowed"], false);
        assert_eq!(permission_states(&manifest, &grants)[1]["allowed"], true);
        grants.permissions.insert("local_recording".into(), true);
        grants.permissions.insert("spawn_agents".into(), false);
        let states = permission_states(&manifest, &grants);
        assert_eq!(states[0]["allowed"], true);
        assert_eq!(states[1]["allowed"], false);
    }

    #[test]
    fn recording_capability_declares_permission_without_granting_it() {
        let manifest =
            json!({ "capabilities": { "localRecordingTrigger": { "provider": "input" } } });
        assert_eq!(declared_permissions(&manifest).len(), 1);
        assert!(validate_permission(&manifest, "local_recording").is_ok());
        assert!(validate_permission(&manifest, "spawn_agents").is_err());
        assert!(validate_permission(&manifest, "unknown").is_err());
        assert!(validate_permission(&manifest, "network:api.test").is_err());
        let network = json!({ "permissions": { "network": { "allow": [] } } });
        assert!(validate_permission(&network, "network:api.test:8443").is_ok());
        assert!(validate_permission(&network, "network:*.test").is_err());
    }
}
