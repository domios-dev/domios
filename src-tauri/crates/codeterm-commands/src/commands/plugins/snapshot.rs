use codeterm_plugins::snapshot_contracts::{
    MarketplaceRow, Metadata, PluginLoadError, PluginUpdate, Snapshot,
};
use codeterm_plugins::{ChannelDiffRow, ChannelListRow, InstalledPluginRow};
use serde_json::Value;

use super::cli::{read_audit_events, run_plugin_cli};

/// Scan plugin roots for `plugin.json` manifests, keyed by id. The CLI `plugin list`
/// only reports installed plugins with empty paths, so it can't surface capabilities
/// or workspace-loaded plugins; this gives the on-disk truth the daemon loads.
pub(super) fn scan_plugin_manifests(
) -> std::collections::HashMap<String, (std::path::PathBuf, Value, Option<String>)> {
    let workspace = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."));
    let roots = codeterm_service_core::plugin::js_runtime::default_roots(Some(workspace.as_path()));
    let mut map = std::collections::HashMap::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let dir = e.path();
            let manifest_path = dir.join("plugin.json");
            let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(m) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
                map.entry(id.to_string()).or_insert_with(|| {
                    let icon_svg = manifest_icon_svg(&dir, &m);
                    (dir, m, icon_svg)
                });
            }
        }
    }
    map
}

fn enrich_metadata(entry: &mut Metadata, dir: &std::path::Path, m: &Value, icon_svg: Option<&str>) {
    let caps = m.get("capabilities");
    let agent_provider = match caps {
        Some(c) => c
            .get("agentProvider")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        None => true,
    };
    let cap_bool = |key: &str| {
        caps.and_then(|c| c.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    // secretBackend/chatPreprocessor/chatBackend may be a bool OR an object — either form means enabled.
    let cap_present = |key: &str| caps.and_then(|c| c.get(key)).is_some_and(|v| !v.is_null());
    // A cap written as an object is enabled; an explicit `false` is not.
    let cap_enabled = |key: &str| {
        caps.and_then(|c| c.get(key))
            .is_some_and(|v| v.as_bool().unwrap_or(v.is_object()))
    };
    let cap_obj = codeterm_plugins::snapshot_contracts::Capabilities {
        agent_provider,
        secret_backend: cap_enabled("secretBackend"),
        transcriber: cap_bool("transcriber"),
        glance_view: cap_bool("glanceView"),
        status_bubble: cap_bool("statusBubble"),
        view: cap_bool("view"),
        chat_backend: cap_bool("chatBackend") || cap_present("chatBackend"),
        chat_preprocessor: cap_bool("chatPreprocessor") || cap_present("chatPreprocessor"),
        graph_nodes: cap_enabled("graphNodes"),
        local_recording_trigger: cap_present("localRecordingTrigger"),
    };
    // Persisted grants.yaml override wins over the manifest request. Keys are canonical snake_case, camelCase accepted as fallback.
    let grants = codeterm_service_core::plugin::js_runtime::grants::load_grants(dir);
    let mut granted: Vec<&str> = Vec::new();
    if grants.permissions.get("local_recording") == Some(&true) {
        granted.push("local_recording");
    }
    let mut effective_network_allow: Vec<String> = m
        .get("permissions")
        .and_then(|permissions| permissions.get("network"))
        .and_then(|network| network.get("allow"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let declared_network_allow = effective_network_allow.clone();
    let network_requested = m
        .get("permissions")
        .and_then(|p| p.get("network"))
        .is_some_and(Value::is_object);
    for host in &grants.network_allow {
        if !effective_network_allow.contains(host) {
            effective_network_allow.push(host.clone());
        }
    }
    if !effective_network_allow.is_empty() {
        granted.push("network");
    }
    if let Some(p) = m.get("permissions") {
        let allow_nonempty = |key: &str| {
            p.get(key)
                .and_then(|n| n.get("allow"))
                .and_then(|a| a.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        };
        // Toggleable perm: persisted grant override wins, else the manifest request.
        let perm_on = |snake: &str, camel: &str| {
            if let Some(v) = grants.permissions.get(snake) {
                return *v;
            }
            p.get(snake)
                .and_then(|v| v.as_bool())
                .or_else(|| p.get(camel).and_then(|v| v.as_bool()))
                .unwrap_or(false)
        };
        if allow_nonempty("subprocess") {
            granted.push("subprocess");
        }
        if perm_on("secrets", "secrets") {
            granted.push("secrets");
        }
        if p.get("sendText").and_then(|v| v.as_bool()).unwrap_or(false) {
            granted.push("send_text");
        }
        if perm_on("agent_commands", "agentCommands") {
            granted.push("agent_commands");
        }
        if perm_on("surfaces", "surfaces") {
            granted.push("surfaces");
        }
        if perm_on("manage_workspaces", "manageWorkspaces") {
            granted.push("manage_workspaces");
        }
        if perm_on("spawn_agents", "spawnAgents") {
            granted.push("spawn_agents");
        }
        if perm_on("analytics", "analytics") {
            granted.push("analytics");
        }
        if perm_on("graph_events", "graphEvents") {
            granted.push("graph_events");
        }
    }
    let settings_schema = m
        .get("settingsSchema")
        .and_then(|v| v.as_str())
        .and_then(|rel| std::fs::read_to_string(dir.join(rel)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());

    entry.icon_svg = icon_svg.map(str::to_owned);
    entry.icon_color = m
        .get("iconColor")
        .and_then(Value::as_str)
        .and_then(codeterm_plugin::plugin_icon::sanitize_plugin_icon_color);
    entry.capabilities = Some(cap_obj);
    entry.sensitive_permissions = Some(super::permissions::permission_states(m, &grants));
    entry.granted_permissions = Some(granted.into_iter().map(str::to_owned).collect());
    entry.network_requested = Some(network_requested);
    entry.declared_network_allow = Some(declared_network_allow);
    entry.network_allow = Some(grants.network_allow.clone());
    entry.effective_network_allow = Some(effective_network_allow);
    entry.requires_reload = Some(
        m.get("requiresReload")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );
    if let Some(schema) = settings_schema {
        entry.settings_schema = Some(schema);
    }
    if entry.display_name.is_none() {
        entry.display_name = m
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    if entry.version.is_none() {
        entry.version = m.get("version").and_then(Value::as_str).map(str::to_owned);
    }
    entry.default_model = m
        .get("defaultModel")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(value) = m.get("description").and_then(Value::as_str) {
        entry.description = Some(value.into());
    }
    if let Some(value) = m.get("pitch").and_then(Value::as_str) {
        entry.pitch = Some(value.into());
    }
    if let Some(value) = m.get("configHelp").and_then(Value::as_str) {
        entry.config_help = Some(value.into());
    }
    if let Some(value) = m.get("homepage").and_then(Value::as_str) {
        entry.homepage = Some(value.into());
    }
    let readme_rel = m
        .get("readme")
        .and_then(Value::as_str)
        .unwrap_or("README.md");
    if let Some(value) = read_bounded_text(&dir.join(readme_rel)) {
        entry.readme_markdown = Some(value);
    }
}

/// Read a UTF-8 text file, capped so a runaway README can't bloat the snapshot.
fn read_bounded_text(path: &std::path::Path) -> Option<String> {
    const MAX: usize = 64 * 1024;
    let mut text = std::fs::read_to_string(path).ok()?;
    if text.len() > MAX {
        text.truncate(MAX);
        text.push_str("\n\n… (truncated)");
    }
    Some(text)
}

/// The expensive channel diff runs inside the snapshot's blocking worker.
/// Cold reads complete inline; warm stale reads refresh in the background.
fn fetch_channel_diff() -> Result<Value, String> {
    run_plugin_cli(&["plugin", "channel", "diff", "--json"])
}

/// Synchronously recompute + cache channel drift after explicit plugin/channel actions.
/// The action response must describe post-action state; passive reads stay backgrounded.
pub(super) fn refresh_channel_diff_now() {
    let _ = super::diff_cache::refresh_diff_now(fetch_channel_diff);
}

pub(super) fn build_snapshot(
    last_output: Option<Value>,
    _chatbackends: Option<&codeterm_service_core::chatbackend::ChatBackendRegistry>,
) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let listed = run_plugin_cli(&["plugin", "list", "--json"])?;
    let mut plugins: Vec<InstalledPluginRow> = serde_json::from_value(
        listed
            .get("plugins")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|error| error.to_string())?;
    let manifests = scan_plugin_manifests();
    let pending_reload: std::collections::HashSet<String> =
        codeterm_service_core::plugin::js_runtime::pending_reload_ids()
            .into_iter()
            .collect();
    let mut seen = std::collections::HashSet::new();
    for entry in &mut plugins {
        seen.insert(entry.id.clone());
        entry.metadata.icon_svg = None;
        entry.metadata.icon_color = None;
        if let Some((dir, m, icon_svg)) = manifests.get(&entry.id) {
            enrich_metadata(&mut entry.metadata, dir, m, icon_svg.as_deref());
        }
        entry.pending_reload = Some(pending_reload.contains(&entry.id));
    }
    for (id, (dir, m, icon_svg)) in &manifests {
        if seen.contains(id) {
            continue;
        }
        let mut entry = InstalledPluginRow {
            id: id.clone(),
            state: "active".into(),
            system: false,
            removable: false,
            dev: Some(true),
            install_path: Some(Some(dir.to_string_lossy().into_owned())),
            ..Default::default()
        };
        enrich_metadata(&mut entry.metadata, dir, m, icon_svg.as_deref());
        plugins.push(entry);
    }
    attach_errors(
        &mut plugins,
        codeterm_service_core::plugin::js_runtime::plugin_load_errors(),
    );
    let _ = run_plugin_cli(&["plugin", "channel", "ensure-default", "--json"]);
    let channel_result = run_plugin_cli(&["plugin", "channel", "list", "--json"])?;
    let channels: Vec<ChannelListRow> = serde_json::from_value(
        channel_result
            .get("channels")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|error| error.to_string())?;
    let diff_result = super::diff_cache::cached_diff(fetch_channel_diff);
    let diff: Vec<ChannelDiffRow> = serde_json::from_value(
        diff_result
            .get("diff")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|error| error.to_string())?;
    reconcile_updates(&mut plugins, &diff);
    let installed_ids = plugins.iter().map(|plugin| plugin.id.clone()).collect();
    let marketplace = marketplace_rows(&diff, &channels, &installed_ids);
    let audit = read_audit_events(80);
    log::debug!(
        "[plugin-snapshot] build_snapshot in {}ms",
        started.elapsed().as_millis()
    );
    Ok(serde_json::json!(Snapshot {
        plugins,
        channels,
        diff,
        marketplace,
        audit,
        last_output
    }))
}

fn attach_errors(plugins: &mut Vec<InstalledPluginRow>, errors: Vec<PluginLoadError>) {
    let mut errors: std::collections::HashMap<_, _> = errors
        .into_iter()
        .map(|error| (error.id.clone(), error))
        .collect();
    for entry in plugins.iter_mut() {
        entry.load_error = Some(errors.remove(&entry.id));
    }
    let mut orphans: Vec<_> = errors.into_iter().collect();
    orphans.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, error) in orphans {
        let install_path = error
            .path
            .parent()
            .map(|dir| dir.to_string_lossy().into_owned());
        plugins.push(InstalledPluginRow {
            id,
            state: "error".into(),
            system: false,
            removable: false,
            dev: Some(true),
            install_path: Some(install_path),
            load_error: Some(Some(error)),
            ..Default::default()
        });
    }
}

fn reconcile_updates(plugins: &mut [InstalledPluginRow], diff: &[ChannelDiffRow]) {
    use codeterm_plugins::{
        update_availability, AvailabilityItem, InstallSource, UpdateOffer, UpdateSource,
    };
    for plugin in plugins {
        let source = plugin.source.as_ref().and_then(Option::as_ref);
        let channel = match source {
            Some(
                InstallSource::Channel { channel, .. }
                | InstallSource::SystemChannel { channel, .. },
            ) => Some(channel.as_str()),
            _ => None,
        };
        let update_source = match source {
            Some(InstallSource::Dir { .. }) => UpdateSource::Local,
            _ => channel
                .map(UpdateSource::Channel)
                .unwrap_or(UpdateSource::Unknown),
        };
        let row = diff
            .iter()
            .find(|row| row.id == plugin.id && Some(row.channel.as_str()) == channel);
        let offer = row.map(|row| UpdateOffer {
            channel: &row.channel,
            version: row.remote_version.as_deref(),
            payload_version: row.payload_version.as_deref(),
            compatible: row.compatible,
        });
        let decision =
            update_availability(plugin.metadata.version.as_deref(), update_source, offer);
        let availability = AvailabilityItem::plugin(
            &plugin.id,
            plugin.metadata.version.as_deref(),
            row.and_then(|row| row.remote_version.as_deref()),
            channel,
            row.is_some_and(|row| row.compatible),
            plugin.system,
            &decision,
        );
        plugin.update = Some(PluginUpdate {
            available: decision.available && !plugin.system,
            availability,
            reason: decision.reason.into(),
            channel: channel.map(str::to_owned),
            remote_version: row.and_then(|row| row.remote_version.clone()),
            basis: "channel_diff".into(),
        });
    }
}

fn marketplace_rows(
    diff: &[ChannelDiffRow],
    channels: &[ChannelListRow],
    installed_ids: &std::collections::HashSet<String>,
) -> Vec<MarketplaceRow> {
    diff.iter()
        .map(|row| {
            let mut metadata = Metadata::default();
            if let Some((dir, manifest, icon_svg)) =
                channel_plugin_manifest(channels, &row.channel, &row.id)
            {
                enrich_metadata(&mut metadata, &dir, &manifest, icon_svg.as_deref());
            }
            MarketplaceRow {
                diff: row.clone(),
                installed: installed_ids.contains(&row.id),
                metadata,
            }
        })
        .collect()
}

/// Locate a channel plugin's on-disk dir + parsed manifest from the channel cache.
/// Reads `<channel.path>/channel.json` to map the plugin id → its relative path.
fn channel_plugin_manifest(
    channels: &[ChannelListRow],
    channel: &str,
    id: &str,
) -> Option<(std::path::PathBuf, Value, Option<String>)> {
    let channel_path = channels
        .iter()
        .find(|c| c.name == channel)
        .map(|c| c.path.as_str())
        .filter(|p| !p.is_empty())?;
    let root = std::path::PathBuf::from(channel_path);
    let manifest_text = std::fs::read_to_string(root.join("channel.json")).ok()?;
    let manifest: Value = serde_json::from_str(&manifest_text).ok()?;
    let rel = manifest
        .get("plugins")
        .and_then(|p| p.as_array())?
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
        .and_then(|p| {
            p.get("path")
                .and_then(|v| v.as_str())
                .or_else(|| p.get("source").and_then(|v| v.as_str()))
        })?;
    let dir = root.join(rel);
    let plugin_path = dir.join("plugin.json");
    let plugin_text = std::fs::read_to_string(&plugin_path).ok()?;
    let plugin_manifest: Value = serde_json::from_str(&plugin_text).ok()?;
    let icon_svg = manifest_icon_svg(&dir, &plugin_manifest);
    Some((dir, plugin_manifest, icon_svg))
}

// Icons never gate enrichment: a manifest the strict loader rejects still gets its description and capabilities.
fn manifest_icon_svg(dir: &std::path::Path, manifest: &Value) -> Option<String> {
    if let Some(relative) = manifest.get("icon").and_then(Value::as_str) {
        if let Some(svg) = codeterm_plugin::plugin_icon::load_plugin_icon(dir, relative) {
            return Some(svg);
        }
    }
    // Channel plugins published before icon.svg still inline their icon; it passes the same sanitizer.
    let inline = manifest.get("iconSvg")?.as_str()?;
    codeterm_plugin::plugin_icon::sanitize_plugin_icon_svg(inline.as_bytes())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod availability_regressions;

#[cfg(test)]
fn enrich_from_manifest(entry: &mut Value, dir: &std::path::Path, manifest: &Value) {
    enrich_from_manifest_with_icon(entry, dir, manifest, None);
}
#[cfg(test)]
fn enrich_from_manifest_with_icon(
    entry: &mut Value,
    dir: &std::path::Path,
    manifest: &Value,
    icon: Option<&str>,
) {
    let mut row: InstalledPluginRow = serde_json::from_value(entry.clone()).unwrap();
    enrich_metadata(&mut row.metadata, dir, manifest, icon);
    *entry = serde_json::json!(row);
}
#[cfg(test)]
fn fixture_plugins(value: &Value) -> Vec<InstalledPluginRow> {
    let mut value = value.clone();
    for row in value.as_array_mut().unwrap() {
        if let Some(source) = row.get_mut("source").and_then(Value::as_object_mut) {
            source.entry("path").or_insert(serde_json::json!(""));
        }
    }
    serde_json::from_value(value).unwrap()
}
#[cfg(test)]
fn fixture_diff(value: &Value) -> Vec<ChannelDiffRow> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|row| ChannelDiffRow {
            id: row["id"].as_str().unwrap_or_default().into(),
            channel: row["channel"].as_str().unwrap_or_default().into(),
            local_version: row["local_version"].as_str().map(str::to_owned),
            remote_version: row["remote_version"].as_str().map(str::to_owned),
            payload_version: row["payload_version"].as_str().map(str::to_owned),
            local_manifest_hash: None,
            remote_manifest_hash: None,
            drift: row["drift"].as_str().unwrap_or_default().into(),
            compatible: row["compatible"].as_bool().unwrap_or(false),
            blocked_reason: row["blocked_reason"].as_str().map(str::to_owned),
            availability: codeterm_plugins::AvailabilityItem::plugin(
                "fixture",
                None,
                None,
                None,
                false,
                false,
                &codeterm_plugins::UpdateAvailability {
                    available: false,
                    reason: "fixture",
                },
            ),
        })
        .collect()
}
#[cfg(test)]
fn attach_load_errors(rows: &mut Vec<Value>, errors: Vec<PluginLoadError>) {
    let mut typed = fixture_plugins(&serde_json::json!(rows));
    attach_errors(&mut typed, errors);
    *rows = typed
        .into_iter()
        .map(|row| serde_json::json!(row))
        .collect();
}
#[cfg(test)]
fn reconcile_update_availability(plugins: &mut Value, diff: &Value) {
    let mut typed = fixture_plugins(plugins);
    reconcile_updates(&mut typed, &fixture_diff(diff));
    *plugins = serde_json::json!(typed);
}
#[cfg(test)]
fn build_marketplace(
    diff: &[Value],
    channels: &[Value],
    installed: &std::collections::HashSet<String>,
) -> Vec<Value> {
    let channels = channels
        .iter()
        .map(|row| ChannelListRow {
            name: row["name"].as_str().unwrap_or_default().into(),
            url: String::new(),
            path: row["path"].as_str().unwrap_or_default().into(),
            kind: codeterm_plugins::ChannelKind::Local,
            system: false,
            removable: false,
        })
        .collect::<Vec<_>>();
    marketplace_rows(
        &fixture_diff(&serde_json::json!(diff)),
        &channels,
        installed,
    )
    .into_iter()
    .map(|row| serde_json::json!(row))
    .collect()
}
