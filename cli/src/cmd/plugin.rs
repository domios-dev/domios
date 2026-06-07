use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::args::plugin::{PluginAction, PluginChannelAction};
use crate::data_root;

const SYSTEM_CHANNEL_NAME: &str = "codeterm-core";
const DEFAULT_SYSTEM_CHANNEL_URL: &str = "https://github.com/rollacode/codeterm.git#develop:plugins";
const SYSTEM_PLUGIN_IDS: [&str; 3] = ["claude", "codex", "opencode"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ChannelRegistry {
    #[serde(default)]
    channels: BTreeMap<String, ChannelRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChannelRecord {
    name: String,
    url: String,
    path: String,
    kind: ChannelKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ChannelKind {
    Local,
    Git,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallRecord {
    id: String,
    version: Option<String>,
    source: InstallSource,
    installed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_change: Option<LastChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InstallSource {
    Dir { path: String },
    Channel { channel: String, path: String },
    SystemChannel { channel: String, path: String },
}

#[derive(Debug, Deserialize)]
struct PluginManifest {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default, rename = "minCodeterm")]
    min_codeterm: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelManifest {
    name: String,
    plugins: Vec<ChannelPlugin>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelPlugin {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    source: Option<serde_json::Value>,
    #[serde(default, rename = "minCodeterm")]
    min_codeterm: Option<String>,
}

#[derive(Debug, Serialize)]
struct InstalledPluginRow {
    id: String,
    version: Option<String>,
    display_name: Option<String>,
    path: String,
    install_path: String,
    install_time: Option<String>,
    manifest_hash: Option<String>,
    verified: bool,
    last_change: Option<LastChange>,
    state: String,
    source: Option<InstallSource>,
    system: bool,
    removable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastChange {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    time: String,
}

#[derive(Debug, Serialize)]
struct ChannelDiffRow {
    channel: String,
    id: String,
    local_version: Option<String>,
    remote_version: Option<String>,
    local_manifest_hash: Option<String>,
    remote_manifest_hash: Option<String>,
    drift: String,
    compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocked_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChannelListRow {
    name: String,
    url: String,
    path: String,
    kind: ChannelKind,
    system: bool,
    removable: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PluginEvent {
    ts: String,
    kind: String,
    actor: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn handle_plugin(json: bool, action: PluginAction) {
    let result = match action.clone() {
        PluginAction::List => list_installed(),
        PluginAction::Install { id, from_dir, id_override, channel } => {
            install_command(id, from_dir, id_override, channel)
        }
        PluginAction::Update { id } => update_command(id),
        PluginAction::UpdateSystem { id } => update_system_command(id),
        PluginAction::Remove { id, force } => remove_installed(&id, force),
        PluginAction::Channel { command } => channel_command(command),
    };

    if let Err(e) = &result {
        let _ = audit_command_failure(&action, e);
    }

    match result {
        Ok(value) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                print_human(&value);
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn print_human(value: &serde_json::Value) {
    if let Some(arr) = value.get("plugins").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            println!("No plugins installed.");
            return;
        }
        for p in arr {
            let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let version = p.get("version").and_then(|v| v.as_str()).unwrap_or("unknown");
            let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
            println!("{id}\t{version}\t{path}");
        }
        return;
    }
    if let Some(arr) = value.get("channels").and_then(|v| v.as_array()) {
        if arr.is_empty() {
            println!("No plugin channels registered.");
            return;
        }
        for c in arr {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let url = c.get("url").and_then(|v| v.as_str()).unwrap_or("");
            println!("{name}\t{url}");
        }
        return;
    }
    if let Some(msg) = value.get("message").and_then(|v| v.as_str()) {
        println!("{msg}");
        return;
    }
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

fn install_command(
    id: Option<String>,
    from_dir: Option<String>,
    id_override: Option<String>,
    channel: Option<String>,
) -> Result<serde_json::Value, String> {
    if let Some(dir) = from_dir {
        let source = absolutize(Path::new(&dir))?;
        let install_id = id_override
            .or(id)
            .or_else(|| manifest_id(&source).ok())
            .unwrap_or_else(|| source.file_name().and_then(|s| s.to_str()).unwrap_or("plugin").to_string());
        let version = manifest_version(&source).ok().flatten();
        let record = InstallRecord {
            id: install_id.clone(),
            version: version.clone(),
            source: InstallSource::Dir { path: source.to_string_lossy().to_string() },
            installed_at: now_rfc3339(),
            last_change: Some(last_change_for_install(version, None)),
        };
        if let Err(reason) = compat_check(manifest_min_codeterm(&source).as_deref()) {
            return Err(format!("plugin '{install_id}' {reason}"));
        }
        let before = manifest_version(&plugin_dir(&install_id)).ok().flatten();
        let result = install_from_dir(&source, &install_id, Some(&record));
        audit_plugin_result(
            "install",
            Some(&install_id),
            None,
            before,
            record.version.clone(),
            &result,
        );
        result?;
        return Ok(serde_json::json!({
            "ok": true,
            "message": format!("Installed plugin '{}' from {}", install_id, source.display()),
            "id": install_id,
            "path": plugin_dir(&install_id).display().to_string(),
        }));
    }

    let id = id.ok_or("install requires <id> or --from-dir <path>")?;
    let candidate = find_channel_plugin(&id, channel.as_deref())?;
    let source_dir = candidate.channel_path.join(&candidate.plugin_path);
    let channel_name = candidate.channel_name.clone();
    let version = candidate.version;
    let record = InstallRecord {
        id: id.clone(),
        version: version.clone(),
        source: InstallSource::Channel {
            channel: channel_name.clone(),
            path: candidate.plugin_path,
        },
        installed_at: now_rfc3339(),
        last_change: Some(last_change_for_install(version, Some(channel_name.clone()))),
    };
    if let Err(reason) = compat_check(manifest_min_codeterm(&source_dir).as_deref()) {
        return Err(format!("plugin '{id}' {reason}"));
    }
    let before = manifest_version(&plugin_dir(&id)).ok().flatten();
    let result = install_from_dir(&source_dir, &id, Some(&record));
    audit_plugin_result(
        "install",
        Some(&id),
        Some(&channel_name),
        before,
        record.version.clone(),
        &result,
    );
    result?;
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Installed plugin '{}' from channel", id),
        "id": id,
        "path": plugin_dir(&id).display().to_string(),
    }))
}

fn update_command(id: Option<String>) -> Result<serde_json::Value, String> {
    if let Some(id) = id {
        update_one(&id)?;
        return Ok(serde_json::json!({"ok": true, "message": format!("Updated plugin '{}'", id)}));
    }
    let mut updated = Vec::new();
    for id in installed_ids()? {
        if update_one(&id).is_ok() {
            updated.push(id);
        }
    }
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Updated {} plugin(s)", updated.len()),
        "updated": updated,
    }))
}

fn update_one(id: &str) -> Result<(), String> {
    let record = read_install_record(id)?;
    let before = manifest_version(&plugin_dir(id)).ok().flatten();
    match record.source.clone() {
        InstallSource::Dir { path } => {
            let after = manifest_version(Path::new(&path)).ok().flatten();
            let next_record = InstallRecord {
                installed_at: now_rfc3339(),
                version: after.clone(),
                last_change: Some(last_change_for_update(before.clone(), after.clone(), None)),
                ..record
            };
            let result = install_from_dir(Path::new(&path), id, Some(&next_record));
            audit_plugin_result("update", Some(id), None, before, after, &result);
            result
        }
        InstallSource::Channel { channel, path } => {
            refresh_channel(Some(&channel))?;
            let reg = read_channel_registry()?;
            let ch = reg.channels.get(&channel).ok_or_else(|| format!("channel '{channel}' not registered"))?;
            let source = Path::new(&ch.path).join(&path);
            let after = manifest_version(&source).ok().flatten();
            let next_record = InstallRecord {
                installed_at: now_rfc3339(),
                version: after.clone(),
                last_change: Some(last_change_for_update(before.clone(), after.clone(), Some(channel.clone()))),
                ..record
            };
            if let Err(reason) = compat_check(manifest_min_codeterm(&source).as_deref()) {
                return Err(format!("plugin '{id}' {reason}"));
            }
            let result = install_from_dir(&source, id, Some(&next_record));
            audit_plugin_result("update", Some(id), Some(&channel), before, after, &result);
            result
        }
        InstallSource::SystemChannel { channel, path } => {
            ensure_system_channel()?;
            refresh_channel(Some(&channel))?;
            let reg = read_channel_registry()?;
            let ch = reg.channels.get(&channel).ok_or_else(|| format!("channel '{channel}' not registered"))?;
            let source = Path::new(&ch.path).join(&path);
            let after = manifest_version(&source).ok().flatten();
            let next_record = InstallRecord {
                installed_at: now_rfc3339(),
                version: after.clone(),
                last_change: Some(last_change_for_update(before.clone(), after.clone(), Some(channel.clone()))),
                ..record
            };
            if let Err(reason) = compat_check(manifest_min_codeterm(&source).as_deref()) {
                return Err(format!("plugin '{id}' {reason}"));
            }
            let result = install_from_dir(&source, id, Some(&next_record));
            audit_plugin_result("update", Some(id), Some(&channel), before, after, &result);
            result
        }
    }
}

fn update_system_command(id: Option<String>) -> Result<serde_json::Value, String> {
    ensure_system_channel()?;
    refresh_channel(Some(SYSTEM_CHANNEL_NAME))?;
    let reg = read_channel_registry()?;
    let channel = reg
        .channels
        .get(SYSTEM_CHANNEL_NAME)
        .ok_or_else(|| format!("channel '{SYSTEM_CHANNEL_NAME}' not registered"))?;
    let channel_path = PathBuf::from(&channel.path);
    let manifest = read_channel_manifest(&channel_path)?;
    let requested = match id {
        Some(id) => {
            let plugin = manifest
                .plugins
                .iter()
                .find(|plugin| plugin.id == id)
                .cloned()
                .ok_or_else(|| format!("'{id}' is not listed in the {SYSTEM_CHANNEL_NAME} channel"))?;
            vec![plugin]
        }
        None => manifest.plugins,
    };
    let mut updated = Vec::new();
    let mut blocked = Vec::new();
    for plugin in requested {
        let id = plugin.id.clone();
        let plugin_path = plugin_path(&plugin)?;
        let source_dir = channel_path.join(&plugin_path);
        let min = plugin
            .min_codeterm
            .clone()
            .or_else(|| manifest_min_codeterm(&source_dir));
        if let Err(reason) = compat_check(min.as_deref()) {
            blocked.push(format!("system plugin '{id}' {reason}"));
            continue;
        }
        let version = plugin.version;
        let record = InstallRecord {
            id: id.clone(),
            version: version.clone(),
            source: InstallSource::SystemChannel {
                channel: SYSTEM_CHANNEL_NAME.to_string(),
                path: plugin_path,
            },
            installed_at: now_rfc3339(),
            last_change: Some(last_change_for_install(version, Some(SYSTEM_CHANNEL_NAME.to_string()))),
        };
        let before = manifest_version(&plugin_dir(&id)).ok().flatten();
        let result = install_from_dir(&source_dir, &id, Some(&record));
        audit_plugin_result(
            "update_system",
            Some(&id),
            Some(SYSTEM_CHANNEL_NAME),
            before,
            record.version.clone(),
            &result,
        );
        result?;
        updated.push(id);
    }
    if updated.is_empty() && !blocked.is_empty() {
        return Err(blocked.join("; "));
    }
    let mut warnings = removed_system_channel_warnings(&updated)?;
    warnings.extend(blocked);
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Updated {} system plugin(s)", updated.len()),
        "updated": updated,
        "warnings": warnings,
    }))
}

fn removed_system_channel_warnings(current_ids: &[String]) -> Result<Vec<String>, String> {
    let current: std::collections::BTreeSet<_> = current_ids.iter().cloned().collect();
    let mut warnings = Vec::new();
    for id in installed_ids()? {
        let record = read_install_record(&id).ok();
        let Some(InstallRecord { source: InstallSource::SystemChannel { channel, .. }, .. }) = record else {
            continue;
        };
        if channel == SYSTEM_CHANNEL_NAME && !current.contains(&id) {
            warnings.push(format!(
                "system plugin '{id}' is installed but no longer listed in {SYSTEM_CHANNEL_NAME}; it was left installed"
            ));
        }
    }
    Ok(warnings)
}

fn remove_installed(id: &str, force: bool) -> Result<serde_json::Value, String> {
    if is_reserved_system_id(id) && !force {
        return Err(format!("'{id}' is a system plugin id; pass --force to remove its user-installed folder"));
    }
    let dir = plugin_dir(id);
    if !dir.exists() {
        return Err(format!("plugin '{id}' is not installed"));
    }
    let before = manifest_version(&dir).ok().flatten();
    let result = fs::remove_dir_all(&dir).map_err(|e| format!("remove {}: {e}", dir.display()));
    audit_plugin_result("remove", Some(id), None, before, None, &result);
    result?;
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Removed plugin '{}'", id),
    }))
}

fn channel_command(action: PluginChannelAction) -> Result<serde_json::Value, String> {
    match action {
        PluginChannelAction::Add { url, name } => add_channel(&url, name),
        PluginChannelAction::List => list_channels(),
        PluginChannelAction::Refresh { name } => {
            refresh_channel(name.as_deref())?;
            Ok(serde_json::json!({"ok": true, "message": "Channel refresh complete"}))
        }
        PluginChannelAction::Diff { name } => channel_diff(name.as_deref()),
        PluginChannelAction::Remove { name, force } => remove_channel(&name, force),
    }
}

fn list_installed() -> Result<serde_json::Value, String> {
    let root = plugins_root();
    fs::create_dir_all(&root).map_err(|e| format!("mkdir {}: {e}", root.display()))?;
    let mut rows = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| format!("read_dir {}: {e}", root.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let path = entry.path();
        let folder_id = path.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        if folder_id.ends_with(".tmp-install") {
            continue;
        }
        let id = manifest_id(&path).unwrap_or_else(|_| {
            folder_id.clone()
        });
        let manifest = read_plugin_manifest(&path).ok();
        let record = read_install_record(&folder_id)
            .or_else(|_| read_install_record(&id))
            .ok();
        rows.push(InstalledPluginRow {
            id: id.clone(),
            version: manifest.as_ref().and_then(|m| m.version.clone()),
            display_name: manifest.and_then(|m| m.display_name),
            path: path.display().to_string(),
            install_path: path.display().to_string(),
            install_time: record.as_ref().map(|r| r.installed_at.clone()),
            manifest_hash: manifest_hash(&path).ok(),
            verified: is_verified_install(&id, record.as_ref()),
            last_change: record.as_ref().and_then(|r| r.last_change.clone()),
            state: if path.with_extension("tmp-install").exists() {
                "mid-install".to_string()
            } else {
                "installed".to_string()
            },
            source: record.map(|r| r.source),
            system: is_reserved_system_id(&id),
            removable: !is_reserved_system_id(&id),
        });
    }
    merge_system_rows(&mut rows);
    Ok(serde_json::json!({ "plugins": rows }))
}

// System provider plugins are intrinsic: always present and non-removable, even
// when not extracted to disk (dev, or a fresh prod before bootstrap).
fn merge_system_rows(rows: &mut Vec<InstalledPluginRow>) {
    let present: std::collections::BTreeSet<String> =
        rows.iter().map(|r| r.id.clone()).collect();
    for id in SYSTEM_PLUGIN_IDS {
        if !present.contains(id) {
            rows.push(virtual_system_row(id));
        }
    }
    rows.sort_by(|a, b| system_rank(&a.id).cmp(&system_rank(&b.id)).then_with(|| a.id.cmp(&b.id)));
}

fn system_rank(id: &str) -> (u8, usize) {
    SYSTEM_PLUGIN_IDS
        .iter()
        .position(|s| *s == id)
        .map(|p| (0, p))
        .unwrap_or((1, 0))
}

fn virtual_system_row(id: &str) -> InstalledPluginRow {
    InstalledPluginRow {
        id: id.to_string(),
        version: system_channel_version(id),
        display_name: None,
        path: String::new(),
        install_path: String::new(),
        install_time: None,
        manifest_hash: None,
        verified: true,
        last_change: None,
        state: "bundled".to_string(),
        source: Some(InstallSource::SystemChannel {
            channel: SYSTEM_CHANNEL_NAME.to_string(),
            path: format!("provider-{id}"),
        }),
        system: true,
        removable: false,
    }
}

fn add_channel(url: &str, name_override: Option<String>) -> Result<serde_json::Value, String> {
    fs::create_dir_all(channels_root()).map_err(|e| e.to_string())?;
    let source = parse_channel_source(url);
    let is_local = looks_like_local_path(&source.base);
    let temp_name = name_override.clone().unwrap_or_else(|| safe_channel_name(url));
    let cache_path = channels_root().join(&temp_name);
    if cache_path.exists() {
        fs::remove_dir_all(&cache_path).map_err(|e| format!("clear {}: {e}", cache_path.display()))?;
    }
    if is_local {
        copy_dir_filtered(&source.source_dir(), &cache_path)?;
    } else {
        clone_git_channel(&source, &cache_path)?;
    }
    let manifest = read_channel_manifest(&cache_path)?;
    let name = name_override.unwrap_or_else(|| manifest.name.clone());
    let final_path = channels_root().join(&name);
    if final_path != cache_path {
        if final_path.exists() {
            fs::remove_dir_all(&final_path).map_err(|e| format!("clear {}: {e}", final_path.display()))?;
        }
        fs::rename(&cache_path, &final_path).map_err(|e| format!("rename channel cache: {e}"))?;
    }
    let mut reg = read_channel_registry().unwrap_or_default();
    reg.channels.insert(name.clone(), ChannelRecord {
        name: name.clone(),
        url: url.to_string(),
        path: final_path.display().to_string(),
        kind: if is_local { ChannelKind::Local } else { ChannelKind::Git },
    });
    let result = write_channel_registry(&reg);
    audit_channel_result("channel_add", Some(&name), &result);
    result?;
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Added plugin channel '{}'", name),
        "name": name,
    }))
}

fn list_channels() -> Result<serde_json::Value, String> {
    let reg = read_channel_registry().unwrap_or_default();
    let mut rows: Vec<ChannelListRow> = reg
        .channels
        .values()
        .map(|ch| ChannelListRow {
            name: ch.name.clone(),
            url: ch.url.clone(),
            path: ch.path.clone(),
            kind: ch.kind.clone(),
            system: ch.name == SYSTEM_CHANNEL_NAME,
            removable: ch.name != SYSTEM_CHANNEL_NAME,
        })
        .collect();
    // The system marketplace is intrinsic — listed without a network fetch even
    // before it has been registered/cloned.
    if !rows.iter().any(|r| r.name == SYSTEM_CHANNEL_NAME) {
        rows.push(virtual_system_channel());
    }
    rows.sort_by(|a, b| {
        u8::from(b.system).cmp(&u8::from(a.system)).then_with(|| a.name.cmp(&b.name))
    });
    Ok(serde_json::json!({ "channels": rows }))
}

fn virtual_system_channel() -> ChannelListRow {
    ChannelListRow {
        name: SYSTEM_CHANNEL_NAME.to_string(),
        url: system_channel_url(),
        path: String::new(),
        kind: ChannelKind::Git,
        system: true,
        removable: false,
    }
}

fn channel_diff(name: Option<&str>) -> Result<serde_json::Value, String> {
    let reg = read_channel_registry().unwrap_or_default();
    let channels: Vec<ChannelRecord> = reg
        .channels
        .values()
        .filter(|ch| name.map(|n| n == ch.name).unwrap_or(true))
        .cloned()
        .collect();
    if let Some(name) = name {
        if channels.is_empty() {
            return Err(format!("channel '{name}' not registered"));
        }
    }

    let mut rows = Vec::new();
    for ch in channels {
        let temp = std::env::temp_dir().join(format!(
            "codeterm-channel-diff-{}-{}",
            ch.name,
            uuid::Uuid::new_v4()
        ));
        let channel_path = materialize_channel_for_diff(&ch, &temp)?;
        let manifest = read_channel_manifest(&channel_path)?;
        for plugin in manifest.plugins {
            let remote_path = plugin_path(&plugin)?;
            let remote_dir = channel_path.join(&remote_path);
            let local_dir = plugin_dir(&plugin.id);
            let local_version = manifest_version(&local_dir).ok().flatten();
            let remote_version = plugin.version.clone()
                .or_else(|| manifest_version(&remote_dir).ok().flatten());
            let local_manifest_hash = manifest_hash(&local_dir).ok();
            let remote_manifest_hash = manifest_hash(&remote_dir).ok();
            let drift = classify_drift(
                local_version.as_deref(),
                remote_version.as_deref(),
                local_manifest_hash.as_deref(),
                remote_manifest_hash.as_deref(),
            );
            let remote_min = plugin
                .min_codeterm
                .clone()
                .or_else(|| manifest_min_codeterm(&remote_dir));
            let (compatible, blocked_reason) = match compat_check(remote_min.as_deref()) {
                Ok(()) => (true, None),
                Err(reason) => (false, Some(reason)),
            };
            if drift != "up_to_date" {
                audit_drift_detected(
                    &ch.name,
                    &plugin.id,
                    local_version.clone(),
                    remote_version.clone(),
                    local_manifest_hash.clone(),
                    remote_manifest_hash.clone(),
                    &drift,
                );
            }
            rows.push(ChannelDiffRow {
                channel: ch.name.clone(),
                id: plugin.id,
                local_version,
                remote_version,
                local_manifest_hash,
                remote_manifest_hash,
                drift,
                compatible,
                blocked_reason,
            });
        }
        if temp.exists() {
            let _ = fs::remove_dir_all(&temp);
        }
    }

    Ok(serde_json::json!({ "diff": rows }))
}

fn materialize_channel_for_diff(ch: &ChannelRecord, temp: &Path) -> Result<PathBuf, String> {
    match ch.kind {
        ChannelKind::Local => Ok(parse_channel_source(&ch.url).source_dir()),
        ChannelKind::Git => {
            let source = parse_channel_source(&ch.url);
            clone_git_channel(&source, temp)?;
            Ok(temp.to_path_buf())
        }
    }
}

fn classify_drift(
    local_version: Option<&str>,
    remote_version: Option<&str>,
    local_hash: Option<&str>,
    remote_hash: Option<&str>,
) -> String {
    match (local_version, remote_version) {
        (None, Some(_)) => "not_installed".to_string(),
        (Some(_), None) => "not_in_channel".to_string(),
        (Some(local), Some(remote)) => match version_cmp(local, remote) {
            std::cmp::Ordering::Less => "update_available".to_string(),
            std::cmp::Ordering::Greater => "local_ahead".to_string(),
            std::cmp::Ordering::Equal if local_hash != remote_hash => "hash_drift".to_string(),
            std::cmp::Ordering::Equal => "up_to_date".to_string(),
        },
        (None, None) => "unknown".to_string(),
    }
}

fn remove_channel(name: &str, force: bool) -> Result<serde_json::Value, String> {
    if name == SYSTEM_CHANNEL_NAME && !force {
        return Err(format!(
            "'{name}' is the system plugin channel; pass --force to remove its registry entry"
        ));
    }
    let mut reg = read_channel_registry().unwrap_or_default();
    let ch = reg.channels.remove(name).ok_or_else(|| format!("channel '{name}' not registered"))?;
    let path = PathBuf::from(&ch.path);
    if path.exists() {
        fs::remove_dir_all(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
    }
    let result = write_channel_registry(&reg);
    audit_channel_result("channel_remove", Some(name), &result);
    result?;
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Removed plugin channel '{}'", name),
    }))
}

fn refresh_channel(name: Option<&str>) -> Result<(), String> {
    let reg = read_channel_registry().unwrap_or_default();
    if let Some(name) = name {
        if !reg.channels.contains_key(name) {
            return Err(format!("channel '{name}' not registered"));
        }
    }
    for ch in reg.channels.values() {
        if let Some(name) = name {
            if ch.name != name {
                continue;
            }
        }
        refresh_channel_record(ch)?;
    }
    let result = Ok(());
    audit_channel_result("refresh-fetched", name, &result);
    result
}

fn refresh_channel_record(ch: &ChannelRecord) -> Result<(), String> {
    let path = PathBuf::from(&ch.path);
    let tmp = path.with_file_name(format!(
        ".{}-refresh-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("channel"),
        uuid::Uuid::new_v4()
    ));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("clear {}: {e}", tmp.display()))?;
    }
    let result = materialize_registered_channel(ch, &tmp)
        .and_then(|_| read_channel_manifest(&tmp).map(|_| ()));
    if let Err(err) = result {
        if tmp.exists() {
            let _ = fs::remove_dir_all(&tmp);
        }
        return Err(err);
    }
    if path.exists() {
        fs::remove_dir_all(&path).map_err(|e| format!("clear {}: {e}", path.display()))?;
    }
    fs::rename(&tmp, &path).map_err(|e| format!("replace channel cache: {e}"))
}

fn materialize_registered_channel(ch: &ChannelRecord, dest: &Path) -> Result<(), String> {
    let source = parse_channel_source(&ch.url);
    match ch.kind {
        ChannelKind::Local => copy_dir_filtered(&source.source_dir(), dest),
        ChannelKind::Git => clone_git_channel(&source, dest),
    }
}

fn ensure_system_channel() -> Result<(), String> {
    let url = system_channel_url();
    let mut reg = read_channel_registry().unwrap_or_default();
    let needs_add = match reg.channels.get(SYSTEM_CHANNEL_NAME) {
        Some(ch) => ch.url != url,
        None => true,
    };
    if !needs_add {
        return Ok(());
    }
    let value = add_channel(&url, Some(SYSTEM_CHANNEL_NAME.to_string()))?;
    if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        reg = read_channel_registry().unwrap_or_default();
        if reg.channels.contains_key(SYSTEM_CHANNEL_NAME) {
            return Ok(());
        }
    }
    Err("failed to register system plugin channel".to_string())
}

struct ChannelCandidate {
    channel_name: String,
    channel_path: PathBuf,
    plugin_path: String,
    version: Option<String>,
}

fn find_channel_plugin(id: &str, channel_filter: Option<&str>) -> Result<ChannelCandidate, String> {
    let reg = read_channel_registry().unwrap_or_default();
    let mut matches = Vec::new();
    for ch in reg.channels.values() {
        if let Some(filter) = channel_filter {
            if ch.name != filter {
                continue;
            }
        }
        let path = PathBuf::from(&ch.path);
        let manifest = read_channel_manifest(&path)?;
        for plugin in manifest.plugins {
            if plugin.id != id {
                continue;
            }
            let plugin_path = plugin_path(&plugin)?;
            matches.push(ChannelCandidate {
                channel_name: ch.name.clone(),
                channel_path: path.clone(),
                plugin_path,
                version: plugin.version,
            });
        }
    }
    match matches.len() {
        0 => Err(format!("plugin '{id}' not found in registered channels")),
        1 => Ok(matches.remove(0)),
        _ => Err(format!("plugin '{id}' exists in multiple channels; pass --channel <name>")),
    }
}

fn plugin_path(plugin: &ChannelPlugin) -> Result<String, String> {
    if let Some(path) = &plugin.path {
        validate_relative_path(path)?;
        return Ok(path.clone());
    }
    if let Some(source) = &plugin.source {
        if let Some(path) = source.as_str() {
            validate_relative_path(path)?;
            return Ok(path.to_string());
        }
    }
    Err(format!("channel plugin '{}' has no path/source string", plugin.id))
}

fn install_from_dir(source: &Path, id: &str, record: Option<&InstallRecord>) -> Result<(), String> {
    if !source.join("plugin.json").exists() {
        return Err(format!("{} has no plugin.json", source.display()));
    }
    let dest = plugin_dir(id);
    let tmp = dest.with_extension("tmp-install");
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("clear {}: {e}", tmp.display()))?;
    }
    copy_dir_filtered(source, &tmp)?;
    if let Some(record) = record {
        let body = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
        fs::write(tmp.join(".codeterm-install.json"), body).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("replace {}: {e}", dest.display()))?;
    }
    fs::rename(&tmp, &dest).map_err(|e| format!("install {}: {e}", dest.display()))?;
    Ok(())
}

fn copy_dir_filtered(source: &Path, dest: &Path) -> Result<(), String> {
    let source = absolutize(source)?;
    if !source.is_dir() {
        return Err(format!("{} is not a directory", source.display()));
    }
    fs::create_dir_all(dest).map_err(|e| format!("mkdir {}: {e}", dest.display()))?;
    copy_dir_inner(&source, dest, &source)
}

fn copy_dir_inner(source: &Path, dest: &Path, root: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|e| format!("read_dir {}: {e}", source.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if should_skip(&name) {
            continue;
        }
        let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
        let out = dest.join(rel);
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("mkdir {}: {e}", out.display()))?;
            copy_dir_inner(&path, dest, root)?;
        } else if ft.is_file() {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            fs::copy(&path, &out).map_err(|e| format!("copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn should_skip(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | "target" | ".DS_Store" | "Thumbs.db" | "desktop.ini"
    )
}

fn read_plugin_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let text = fs::read_to_string(dir.join("plugin.json"))
        .map_err(|e| format!("read {}: {e}", dir.join("plugin.json").display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse plugin.json: {e}"))
}

fn read_channel_manifest(dir: &Path) -> Result<ChannelManifest, String> {
    let text = fs::read_to_string(dir.join("channel.json"))
        .map_err(|e| format!("read {}: {e}", dir.join("channel.json").display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse channel.json: {e}"))
}

fn manifest_id(dir: &Path) -> Result<String, String> {
    Ok(read_plugin_manifest(dir)?.id)
}

fn manifest_version(dir: &Path) -> Result<Option<String>, String> {
    Ok(read_plugin_manifest(dir)?.version)
}

fn manifest_hash(dir: &Path) -> Result<String, String> {
    let path = dir.join("plugin.json");
    let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn last_change_for_install(version: Option<String>, channel: Option<String>) -> LastChange {
    LastChange {
        kind: "installed".to_string(),
        from_version: None,
        to_version: version,
        channel,
        time: now_rfc3339(),
    }
}

fn last_change_for_update(
    from_version: Option<String>,
    to_version: Option<String>,
    channel: Option<String>,
) -> LastChange {
    let kind = match (&from_version, &to_version) {
        (Some(from), Some(to)) if version_cmp(to, from).is_lt() => "downgraded",
        _ => "updated",
    };
    LastChange {
        kind: kind.to_string(),
        from_version,
        to_version,
        channel,
        time: now_rfc3339(),
    }
}

fn version_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    match (semverish(left), semverish(right)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => left.cmp(right),
    }
}

fn semverish(value: &str) -> Option<Vec<u64>> {
    value
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

fn read_install_record(id: &str) -> Result<InstallRecord, String> {
    let path = plugin_dir(id).join(".codeterm-install.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn installed_ids() -> Result<Vec<String>, String> {
    let root = plugins_root();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            let id = entry.file_name().to_string_lossy().to_string();
            if !id.ends_with(".tmp-install") {
                out.push(id);
            }
        }
    }
    Ok(out)
}

fn read_channel_registry() -> Result<ChannelRegistry, String> {
    let path = channel_registry_path();
    if !path.exists() {
        return Ok(ChannelRegistry::default());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn write_channel_registry(registry: &ChannelRegistry) -> Result<(), String> {
    let path = channel_registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(registry).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

fn plugins_root() -> PathBuf {
    plugin_data_root().join("plugins")
}

fn plugin_dir(id: &str) -> PathBuf {
    plugins_root().join(id)
}

fn channels_root() -> PathBuf {
    plugin_data_root().join("channels")
}

fn channel_registry_path() -> PathBuf {
    plugin_data_root().join("plugin-channels.json")
}

fn plugin_data_root() -> PathBuf {
    data_root()
}

fn audit_log_path() -> PathBuf {
    plugin_data_root().join("logs").join("plugin-events.log")
}

fn audit_plugin_result(
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

fn audit_channel_result(kind: &str, channel: Option<&str>, result: &Result<(), String>) {
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

fn audit_drift_detected(
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

fn audit_command_failure(action: &PluginAction, error: &str) -> Result<(), String> {
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

fn command_failure_fields(action: &PluginAction) -> (String, Option<String>, Option<String>) {
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

fn event_kind(base: &str, result: &Result<(), String>) -> String {
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

fn append_audit_event(event: &PluginEvent) -> Result<(), String> {
    let value = serde_json::to_value(event).map_err(|e| e.to_string())?;
    append_audit_value(&value)
}

fn append_audit_value(value: &serde_json::Value) -> Result<(), String> {
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

fn looks_like_local_path(value: &str) -> bool {
    value.starts_with("file://")
        || value.starts_with('.')
        || value.starts_with('/')
        || value.contains(":\\")
        || Path::new(value).exists()
}

struct ChannelSource {
    base: String,
    git_ref: Option<String>,
    subdir: Option<String>,
}

impl ChannelSource {
    fn source_dir(&self) -> PathBuf {
        let base = local_path_from_url(&self.base).unwrap_or_else(|| PathBuf::from(&self.base));
        match &self.subdir {
            Some(subdir) => base.join(subdir),
            None => base,
        }
    }
}

fn parse_channel_source(raw: &str) -> ChannelSource {
    let Some((base, fragment)) = raw.split_once('#') else {
        return ChannelSource { base: raw.to_string(), git_ref: None, subdir: None };
    };
    let (git_ref, subdir) = fragment
        .split_once(':')
        .map(|(r, s)| (empty_to_none(r), empty_to_none(s)))
        .unwrap_or_else(|| (empty_to_none(fragment), None));
    ChannelSource { base: base.to_string(), git_ref, subdir }
}

fn empty_to_none(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
}

fn local_path_from_url(value: &str) -> Option<PathBuf> {
    let raw = value.strip_prefix("file://")?;
    #[cfg(target_os = "windows")]
    {
        let raw = raw.strip_prefix('/').unwrap_or(raw);
        return Some(PathBuf::from(raw));
    }
    #[cfg(not(target_os = "windows"))]
    {
        Some(PathBuf::from(raw))
    }
}

fn clone_git_channel(source: &ChannelSource, cache_path: &Path) -> Result<(), String> {
    let tmp = cache_path.with_extension("git-clone");
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("clear {}: {e}", tmp.display()))?;
    }
    run_git(["clone", "--depth", "1", &source.base, &tmp.to_string_lossy()])?;
    if let Some(git_ref) = &source.git_ref {
        run_git_in(&tmp, ["fetch", "--depth", "1", "origin", git_ref])?;
        run_git_in(&tmp, ["checkout", "FETCH_HEAD"])?;
    }
    if let Some(subdir) = &source.subdir {
        validate_relative_path(subdir)?;
        copy_dir_filtered(&tmp.join(subdir), cache_path)?;
        fs::remove_dir_all(&tmp).map_err(|e| format!("remove {}: {e}", tmp.display()))?;
    } else {
        fs::rename(&tmp, cache_path).map_err(|e| format!("rename channel cache: {e}"))?;
    }
    Ok(())
}

fn safe_channel_name(value: &str) -> String {
    let leaf = Path::new(value)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(value);
    let out = leaf
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if out.is_empty() { "channel".to_string() } else { out }
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute() || path.contains("..") {
        return Err(format!("unsafe plugin path in channel: {path}"));
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|e| format!("resolve {}: {e}", path.display()))
}

fn run_git<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let out = Command::new("git").args(args).output().map_err(|e| format!("run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn run_git_in<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<(), String> {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn is_reserved_system_id(id: &str) -> bool {
    SYSTEM_PLUGIN_IDS.contains(&id)
}

fn system_channel_url() -> String {
    std::env::var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SYSTEM_CHANNEL_URL.to_string())
}

fn app_version() -> Option<String> {
    std::env::var("CODETERM_VERSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn manifest_min_codeterm(dir: &Path) -> Option<String> {
    read_plugin_manifest(dir).ok().and_then(|m| m.min_codeterm)
}

fn compat_reason(min: &str, app: Option<&str>) -> String {
    match app {
        Some(app) => format!("requires CodeTerm \u{2265} {min}, but this is {app}"),
        None => format!("requires CodeTerm \u{2265} {min}"),
    }
}

// Fail-open when min is absent or the running version is unknown (raw CLI with
// no daemon); only block when we positively know app < min.
fn compat_check(min: Option<&str>) -> Result<(), String> {
    let Some(min) = min else { return Ok(()) };
    let Some(app) = app_version() else { return Ok(()) };
    if version_cmp(&app, min).is_lt() {
        Err(compat_reason(min, Some(&app)))
    } else {
        Ok(())
    }
}

// Best-effort version for a system plugin not on disk: read the system channel
// cache manifest if it has already been fetched. Never triggers a network call.
fn system_channel_version(id: &str) -> Option<String> {
    let reg = read_channel_registry().ok()?;
    let ch = reg.channels.get(SYSTEM_CHANNEL_NAME)?;
    let manifest = read_channel_manifest(Path::new(&ch.path)).ok()?;
    manifest
        .plugins
        .into_iter()
        .find(|p| p.id == id)
        .and_then(|p| p.version)
}

fn is_verified_install(id: &str, record: Option<&InstallRecord>) -> bool {
    record
        .map(|r| matches!(r.source, InstallSource::SystemChannel { .. }))
        .unwrap_or_else(|| is_reserved_system_id(id))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod store_index_tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn restore_env(key: &str, value: Option<String>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    fn with_data_root<T>(f: impl FnOnce(PathBuf) -> T) -> T {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var("CODETERM_DATA_DIR").ok();
        let root = std::env::temp_dir().join(format!("codeterm-plugin-test-{}", uuid::Uuid::new_v4()));
        std::env::set_var("CODETERM_DATA_DIR", &root);
        let result = f(root.clone());
        restore_env("CODETERM_DATA_DIR", prev);
        let _ = std::fs::remove_dir_all(root);
        result
    }

    fn write_channel(dir: &Path, name: &str, plugins: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        let entries = plugins
            .iter()
            .map(|(id, path)| serde_json::json!({"id": id, "version": "1.0.0", "path": path}))
            .collect::<Vec<_>>();
        std::fs::write(
            dir.join("channel.json"),
            serde_json::json!({"schemaVersion": 1, "name": name, "plugins": entries}).to_string(),
        )
        .unwrap();
        for (id, path) in plugins {
            let plugin_dir = dir.join(path);
            std::fs::create_dir_all(&plugin_dir).unwrap();
            std::fs::write(
                plugin_dir.join("plugin.json"),
                serde_json::json!({
                    "id": id,
                    "version": "1.0.0",
                    "displayName": id,
                    "main": "plugin.js"
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(plugin_dir.join("plugin.js"), "module.exports = {};").unwrap();
        }
    }

    /// The repo's `plugins/` dir IS the `codeterm-core` channel (served at
    /// github.com/rollacode/codeterm.git#master:plugins). `channel.json` is its
    /// store index. This guards the index against drift with the actual plugin
    /// folders — the exact check the live channel-refresh does at runtime.
    #[test]
    fn channel_json_is_a_consistent_store_index() {
        let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins");
        let text = std::fs::read_to_string(plugins_dir.join("channel.json"))
            .expect("plugins/channel.json must exist (store index)");
        let manifest: ChannelManifest =
            serde_json::from_str(&text).expect("channel.json must parse as ChannelManifest");

        assert_eq!(manifest.name, "codeterm-core");
        assert_eq!(
            manifest.plugins.len(),
            3,
            "expected claude/codex/opencode in the core channel"
        );

        for p in &manifest.plugins {
            let rel = p.path.as_ref().expect("each channel plugin needs a path");
            let dir = plugins_dir.join(rel);
            assert!(dir.is_dir(), "declared plugin dir missing: {}", dir.display());

            // channel.json#id must match the plugin's own plugin.json#id.
            let pj = std::fs::read_to_string(dir.join("plugin.json"))
                .unwrap_or_else(|_| panic!("plugin.json missing in {}", dir.display()));
            let pjv: serde_json::Value =
                serde_json::from_str(&pj).expect("plugin.json must parse");
            assert_eq!(
                pjv.get("id").and_then(|v| v.as_str()),
                Some(p.id.as_str()),
                "channel.json id '{}' must match {}/plugin.json#id",
                p.id,
                rel
            );
        }
    }

    #[test]
    fn default_system_channel_points_at_develop_plugins_index() {
        assert_eq!(
            DEFAULT_SYSTEM_CHANNEL_URL,
            "https://github.com/rollacode/codeterm.git#develop:plugins"
        );
    }

    #[test]
    fn plugin_storage_roots_follow_code_term_data_dir_override() {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var("CODETERM_DATA_DIR").ok();
        let root = std::env::temp_dir().join(format!("codeterm-plugin-root-{}", uuid::Uuid::new_v4()));
        std::env::set_var("CODETERM_DATA_DIR", &root);

        assert_eq!(plugins_root(), root.join("plugins"));
        assert_eq!(channels_root(), root.join("channels"));
        assert_eq!(channel_registry_path(), root.join("plugin-channels.json"));

        restore_env("CODETERM_DATA_DIR", prev);
    }

    #[test]
    fn refresh_preserves_existing_channel_cache_when_new_manifest_is_malformed() {
        with_data_root(|root| {
            let bad_source = root.join("bad-source");
            std::fs::create_dir_all(&bad_source).unwrap();
            std::fs::write(bad_source.join("channel.json"), "{not-json").unwrap();

            let cache = channels_root().join("demo");
            write_channel(&cache, "demo", &[("demo-plugin", "provider-demo")]);
            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([(
                    "demo".to_string(),
                    ChannelRecord {
                        name: "demo".to_string(),
                        url: format!("file://{}", bad_source.display()),
                        path: cache.display().to_string(),
                        kind: ChannelKind::Local,
                    },
                )]),
            })
            .unwrap();

            let err = refresh_channel(Some("demo")).expect_err("malformed refresh must fail");
            assert!(err.contains("parse channel.json"), "{err}");
            let manifest = read_channel_manifest(&cache).expect("old cache must remain readable");
            assert_eq!(manifest.name, "demo");
            assert_eq!(manifest.plugins[0].id, "demo-plugin");
        });
    }

    #[test]
    fn update_system_installs_manifest_additions_and_warns_without_auto_removing_deletions() {
        with_data_root(|root| {
            let prev_system_url = std::env::var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL").ok();
            let source = root.join("system-source");
            write_channel(&source, SYSTEM_CHANNEL_NAME, &[("extra-system", "provider-extra")]);
            std::env::set_var(
                "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
                format!("file://{}", source.display()),
            );

            let first = update_system_command(None).expect("system manifest addition installs");
            assert_eq!(first["updated"], serde_json::json!(["extra-system"]));
            assert!(plugin_dir("extra-system").join("plugin.json").exists());
            let listed = list_installed().unwrap();
            let extra = listed["plugins"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == serde_json::json!("extra-system"))
                .expect("installed system plugin should be listed");
            assert_eq!(extra["verified"], serde_json::json!(true));

            write_channel(&source, SYSTEM_CHANNEL_NAME, &[]);
            let second = update_system_command(None).expect("removal warning does not uninstall");
            assert_eq!(second["updated"], serde_json::json!([]));
            assert!(second["warnings"][0]
                .as_str()
                .unwrap()
                .contains("left installed"));
            assert!(plugin_dir("extra-system").join("plugin.json").exists());

            restore_env("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", prev_system_url);
        });
    }

    #[test]
    fn installed_ids_ignore_leftover_tmp_install_dirs() {
        with_data_root(|_root| {
            write_channel(&plugin_dir("real"), "unused", &[]);
            std::fs::create_dir_all(plugins_root().join("partial.tmp-install")).unwrap();
            let ids = installed_ids().unwrap();
            assert_eq!(ids, vec!["real".to_string()]);
            let listed = list_installed().unwrap();
            let rows = listed["plugins"].as_array().unwrap();
            // One real on-disk plugin + the always-present system intrinsics.
            let non_system: Vec<_> = rows.iter().filter(|p| p["system"] != serde_json::json!(true)).collect();
            assert_eq!(non_system.len(), 1);
            assert_eq!(non_system[0]["id"], serde_json::json!("real"));
            assert!(rows.iter().any(|p| p["id"] == serde_json::json!("claude") && p["removable"] == serde_json::json!(false)));
        });
    }

    #[test]
    fn list_installed_injects_locked_system_rows_when_dir_empty() {
        with_data_root(|_root| {
            let listed = list_installed().unwrap();
            let rows = listed["plugins"].as_array().unwrap();
            for id in SYSTEM_PLUGIN_IDS {
                let row = rows
                    .iter()
                    .find(|p| p["id"] == serde_json::json!(id))
                    .unwrap_or_else(|| panic!("system plugin {id} must be listed"));
                assert_eq!(row["system"], serde_json::json!(true));
                assert_eq!(row["removable"], serde_json::json!(false));
                assert_eq!(row["state"], serde_json::json!("bundled"));
            }
        });
    }

    #[test]
    fn list_channels_injects_locked_system_channel_when_registry_empty() {
        with_data_root(|_root| {
            let listed = list_channels().unwrap();
            let core = listed["channels"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == serde_json::json!(SYSTEM_CHANNEL_NAME))
                .expect("system channel must be listed")
                .clone();
            assert_eq!(core["system"], serde_json::json!(true));
            assert_eq!(core["removable"], serde_json::json!(false));
        });
    }

    #[test]
    fn compat_check_blocks_only_when_app_older_than_min() {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var("CODETERM_VERSION").ok();

        std::env::set_var("CODETERM_VERSION", "1.3.2");
        assert!(compat_check(Some("1.0.0")).is_ok());
        assert!(compat_check(Some("1.3.2")).is_ok());
        let err = compat_check(Some("2.0.0")).unwrap_err();
        assert!(err.contains("2.0.0") && err.contains("1.3.2"), "got: {err}");

        // Fail-open: unknown running version never blocks.
        std::env::remove_var("CODETERM_VERSION");
        assert!(compat_check(Some("99.0.0")).is_ok());
        // No declared minimum never blocks.
        std::env::set_var("CODETERM_VERSION", "1.0.0");
        assert!(compat_check(None).is_ok());

        restore_env("CODETERM_VERSION", prev);
    }

    #[test]
    fn install_from_channel_refuses_incompatible_min_codeterm() {
        with_data_root(|_root| {
            // env_lock is already held by with_data_root — set directly, restore at end.
            let prev_v = std::env::var("CODETERM_VERSION").ok();
            std::env::set_var("CODETERM_VERSION", "1.0.0");

            let cache = channels_root().join("needychan");
            std::fs::create_dir_all(cache.join("provider-needy")).unwrap();
            std::fs::write(
                cache.join("channel.json"),
                serde_json::json!({
                    "schemaVersion": 1,
                    "name": "needychan",
                    "plugins": [{"id":"needy","version":"1.0.0","path":"provider-needy","minCodeterm":"2.0.0"}]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(
                cache.join("provider-needy/plugin.json"),
                serde_json::json!({"id":"needy","version":"1.0.0","minCodeterm":"2.0.0"}).to_string(),
            )
            .unwrap();
            std::fs::write(cache.join("provider-needy/plugin.js"), "module.exports={};").unwrap();
            write_channel_registry(&ChannelRegistry {
                channels: BTreeMap::from([(
                    "needychan".to_string(),
                    ChannelRecord {
                        name: "needychan".to_string(),
                        url: format!("file://{}", cache.display()),
                        path: cache.display().to_string(),
                        kind: ChannelKind::Local,
                    },
                )]),
            })
            .unwrap();

            let err = install_command(Some("needy".to_string()), None, None, Some("needychan".to_string()))
                .unwrap_err();
            assert!(err.contains("requires CodeTerm"), "got: {err}");
            assert!(!plugin_dir("needy").exists(), "incompatible plugin must not be installed");

            restore_env("CODETERM_VERSION", prev_v);
        });
    }
}
