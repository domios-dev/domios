use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::args::plugin::{PluginAction, PluginChannelAction};

const SYSTEM_CHANNEL_NAME: &str = "codeterm-core";
const DEFAULT_SYSTEM_CHANNEL_URL: &str = "https://github.com/rollacode/codeterm.git#master:plugins";

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
}

#[derive(Debug, Deserialize)]
struct ChannelManifest {
    name: String,
    plugins: Vec<ChannelPlugin>,
}

#[derive(Debug, Deserialize)]
struct ChannelPlugin {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    source: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct InstalledPluginRow {
    id: String,
    version: Option<String>,
    display_name: Option<String>,
    path: String,
    source: Option<InstallSource>,
}

pub fn handle_plugin(json: bool, action: PluginAction) {
    let result = match action {
        PluginAction::List => list_installed(),
        PluginAction::Install { id, from_dir, id_override, channel } => {
            install_command(id, from_dir, id_override, channel)
        }
        PluginAction::Update { id } => update_command(id),
        PluginAction::UpdateSystem { id } => update_system_command(id),
        PluginAction::Remove { id, force } => remove_installed(&id, force),
        PluginAction::Channel { command } => channel_command(command),
    };

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
        let record = InstallRecord {
            id: install_id.clone(),
            version: manifest_version(&source).ok().flatten(),
            source: InstallSource::Dir { path: source.to_string_lossy().to_string() },
            installed_at: now_rfc3339(),
        };
        install_from_dir(&source, &install_id, Some(&record))?;
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
    let record = InstallRecord {
        id: id.clone(),
        version: candidate.version,
        source: InstallSource::Channel {
            channel: candidate.channel_name,
            path: candidate.plugin_path,
        },
        installed_at: now_rfc3339(),
    };
    install_from_dir(&source_dir, &id, Some(&record))?;
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
    match record.source.clone() {
        InstallSource::Dir { path } => {
            let next_record = InstallRecord { installed_at: now_rfc3339(), ..record };
            install_from_dir(Path::new(&path), id, Some(&next_record))
        }
        InstallSource::Channel { channel, path } => {
            refresh_channel(Some(&channel))?;
            let reg = read_channel_registry()?;
            let ch = reg.channels.get(&channel).ok_or_else(|| format!("channel '{channel}' not registered"))?;
            let source = Path::new(&ch.path).join(&path);
            let next_record = InstallRecord { installed_at: now_rfc3339(), ..record };
            install_from_dir(&source, id, Some(&next_record))
        }
        InstallSource::SystemChannel { channel, path } => {
            ensure_system_channel()?;
            refresh_channel(Some(&channel))?;
            let reg = read_channel_registry()?;
            let ch = reg.channels.get(&channel).ok_or_else(|| format!("channel '{channel}' not registered"))?;
            let source = Path::new(&ch.path).join(&path);
            let next_record = InstallRecord { installed_at: now_rfc3339(), ..record };
            install_from_dir(&source, id, Some(&next_record))
        }
    }
}

fn update_system_command(id: Option<String>) -> Result<serde_json::Value, String> {
    ensure_system_channel()?;
    refresh_channel(Some(SYSTEM_CHANNEL_NAME))?;
    let ids = match id {
        Some(id) => {
            if !is_reserved_system_id(&id) {
                return Err(format!("'{id}' is not a bundled system plugin id"));
            }
            vec![id]
        }
        None => system_plugin_ids(),
    };
    let mut updated = Vec::new();
    for id in ids {
        let candidate = find_channel_plugin(&id, Some(SYSTEM_CHANNEL_NAME))?;
        let source_dir = candidate.channel_path.join(&candidate.plugin_path);
        let record = InstallRecord {
            id: id.clone(),
            version: candidate.version,
            source: InstallSource::SystemChannel {
                channel: SYSTEM_CHANNEL_NAME.to_string(),
                path: candidate.plugin_path,
            },
            installed_at: now_rfc3339(),
        };
        install_from_dir(&source_dir, &id, Some(&record))?;
        updated.push(id);
    }
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Updated {} system plugin(s)", updated.len()),
        "updated": updated,
    }))
}

fn remove_installed(id: &str, force: bool) -> Result<serde_json::Value, String> {
    if is_reserved_system_id(id) && !force {
        return Err(format!("'{id}' is a system plugin id; pass --force to remove its user-installed folder"));
    }
    let dir = plugin_dir(id);
    if !dir.exists() {
        return Err(format!("plugin '{id}' is not installed"));
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("remove {}: {e}", dir.display()))?;
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
        PluginChannelAction::Remove { name } => remove_channel(&name),
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
        let id = manifest_id(&path).unwrap_or_else(|_| {
            folder_id.clone()
        });
        let manifest = read_plugin_manifest(&path).ok();
        rows.push(InstalledPluginRow {
            id: id.clone(),
            version: manifest.as_ref().and_then(|m| m.version.clone()),
            display_name: manifest.and_then(|m| m.display_name),
            path: path.display().to_string(),
            source: read_install_record(&folder_id)
                .or_else(|_| read_install_record(&id))
                .ok()
                .map(|r| r.source),
        });
    }
    Ok(serde_json::json!({ "plugins": rows }))
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
    write_channel_registry(&reg)?;
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Added plugin channel '{}'", name),
        "name": name,
    }))
}

fn list_channels() -> Result<serde_json::Value, String> {
    let reg = read_channel_registry().unwrap_or_default();
    let channels: Vec<_> = reg.channels.values().collect();
    Ok(serde_json::json!({ "channels": channels }))
}

fn remove_channel(name: &str) -> Result<serde_json::Value, String> {
    let mut reg = read_channel_registry().unwrap_or_default();
    let ch = reg.channels.remove(name).ok_or_else(|| format!("channel '{name}' not registered"))?;
    let path = PathBuf::from(&ch.path);
    if path.exists() {
        fs::remove_dir_all(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
    }
    write_channel_registry(&reg)?;
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
        let path = PathBuf::from(&ch.path);
        match ch.kind {
            ChannelKind::Local => {
                if path.exists() {
                    fs::remove_dir_all(&path).map_err(|e| format!("clear {}: {e}", path.display()))?;
                }
                let source = parse_channel_source(&ch.url);
                copy_dir_filtered(&source.source_dir(), &path)?;
            }
            ChannelKind::Git => {
                if path.exists() {
                    fs::remove_dir_all(&path).map_err(|e| format!("clear {}: {e}", path.display()))?;
                }
                let source = parse_channel_source(&ch.url);
                clone_git_channel(&source, &path)?;
            }
        }
    }
    Ok(())
}

fn ensure_system_channel() -> Result<(), String> {
    let url = std::env::var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SYSTEM_CHANNEL_URL.to_string());
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
            out.push(entry.file_name().to_string_lossy().to_string());
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
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".codeterm").join("plugins")
}

fn plugin_dir(id: &str) -> PathBuf {
    plugins_root().join(id)
}

fn channels_root() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".codeterm").join("channels")
}

fn channel_registry_path() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".codeterm").join("plugin-channels.json")
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
    matches!(id, "claude" | "codex" | "opencode")
}

fn system_plugin_ids() -> Vec<String> {
    ["claude", "codex", "opencode"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod store_index_tests {
    use super::ChannelManifest;
    use std::path::Path;

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
}
