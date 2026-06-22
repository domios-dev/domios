use super::*;

pub(super) fn channel_command(action: PluginChannelAction) -> Result<serde_json::Value, String> {
    match action {
        PluginChannelAction::Add { url, name } => add_channel(&url, name),
        PluginChannelAction::EnsureDefault => {
            ensure_default_plugin_channel()?;
            Ok(serde_json::json!({
                "ok": true,
                "message": format!("Default plugin channel '{DEFAULT_PLUGIN_CHANNEL_NAME}' ensured"),
                "name": DEFAULT_PLUGIN_CHANNEL_NAME,
            }))
        }
        PluginChannelAction::List => list_channels(),
        PluginChannelAction::Refresh { name } => {
            refresh_channel(name.as_deref())?;
            Ok(serde_json::json!({"ok": true, "message": "Channel refresh complete"}))
        }
        PluginChannelAction::Diff { name } => channel_diff(name.as_deref()),
        PluginChannelAction::Remove { name, force } => remove_channel(&name, force),
    }
}

pub(super) fn add_channel(
    url: &str,
    name_override: Option<String>,
) -> Result<serde_json::Value, String> {
    fs::create_dir_all(channels_root()).map_err(|e| e.to_string())?;
    let source = parse_channel_source(url);
    let is_local = looks_like_local_path(&source.base);
    let temp_name = name_override
        .clone()
        .unwrap_or_else(|| safe_channel_name(url));
    let cache_path = channels_root().join(&temp_name);
    if cache_path.exists() {
        fs::remove_dir_all(&cache_path)
            .map_err(|e| format!("clear {}: {e}", cache_path.display()))?;
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
            fs::remove_dir_all(&final_path)
                .map_err(|e| format!("clear {}: {e}", final_path.display()))?;
        }
        fs::rename(&cache_path, &final_path).map_err(|e| format!("rename channel cache: {e}"))?;
    }
    let mut reg = read_channel_registry().unwrap_or_default();
    reg.channels.insert(
        name.clone(),
        ChannelRecord {
            name: name.clone(),
            url: url.to_string(),
            path: final_path.display().to_string(),
            kind: if is_local {
                ChannelKind::Local
            } else {
                ChannelKind::Git
            },
        },
    );
    let result = write_channel_registry(&reg);
    audit_channel_result("channel_add", Some(&name), &result);
    result?;
    Ok(serde_json::json!({
        "ok": true,
        "message": format!("Added plugin channel '{}'", name),
        "name": name,
    }))
}

pub(super) fn list_channels() -> Result<serde_json::Value, String> {
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
            removable: ch.name != SYSTEM_CHANNEL_NAME && ch.name != DEFAULT_PLUGIN_CHANNEL_NAME,
        })
        .collect();
    // The system marketplace is intrinsic — listed without a network fetch even
    // before it has been registered/cloned.
    if !rows.iter().any(|r| r.name == SYSTEM_CHANNEL_NAME) {
        rows.push(virtual_system_channel());
    }
    rows.sort_by(|a, b| {
        u8::from(b.system)
            .cmp(&u8::from(a.system))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(serde_json::json!({ "channels": rows }))
}

pub(super) fn virtual_system_channel() -> ChannelListRow {
    ChannelListRow {
        name: SYSTEM_CHANNEL_NAME.to_string(),
        url: system_channel_url(),
        path: String::new(),
        kind: ChannelKind::Git,
        system: true,
        removable: false,
    }
}

pub(super) fn channel_diff(name: Option<&str>) -> Result<serde_json::Value, String> {
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
            let remote_version = plugin
                .version
                .clone()
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

pub(super) fn materialize_channel_for_diff(
    ch: &ChannelRecord,
    temp: &Path,
) -> Result<PathBuf, String> {
    match ch.kind {
        ChannelKind::Local => Ok(parse_channel_source(&ch.url).source_dir()),
        ChannelKind::Git => {
            let source = parse_channel_source(&ch.url);
            clone_git_channel(&source, temp)?;
            Ok(temp.to_path_buf())
        }
    }
}

pub(super) fn classify_drift(
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

pub(super) fn remove_channel(name: &str, force: bool) -> Result<serde_json::Value, String> {
    if name == SYSTEM_CHANNEL_NAME && !force {
        return Err(format!(
            "'{name}' is the system plugin channel; pass --force to remove its registry entry"
        ));
    }
    let mut reg = read_channel_registry().unwrap_or_default();
    let ch = reg
        .channels
        .remove(name)
        .ok_or_else(|| format!("channel '{name}' not registered"))?;
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

pub(super) fn refresh_channel(name: Option<&str>) -> Result<(), String> {
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

pub(super) fn refresh_channel_record(ch: &ChannelRecord) -> Result<(), String> {
    let path = PathBuf::from(&ch.path);
    let tmp = path.with_file_name(format!(
        ".{}-refresh-{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("channel"),
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

pub(super) fn materialize_registered_channel(
    ch: &ChannelRecord,
    dest: &Path,
) -> Result<(), String> {
    let source = parse_channel_source(&ch.url);
    match ch.kind {
        ChannelKind::Local => copy_dir_filtered(&source.source_dir(), dest),
        ChannelKind::Git => clone_git_channel(&source, dest),
    }
}

pub(super) struct ChannelCandidate {
    pub(super) channel_name: String,
    pub(super) channel_path: PathBuf,
    pub(super) plugin_path: String,
    pub(super) version: Option<String>,
}

pub(super) fn find_channel_plugin(
    id: &str,
    channel_filter: Option<&str>,
) -> Result<ChannelCandidate, String> {
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
        _ => Err(format!(
            "plugin '{id}' exists in multiple channels; pass --channel <name>"
        )),
    }
}

pub(super) fn plugin_path(plugin: &ChannelPlugin) -> Result<String, String> {
    if let Some(path) = &plugin.path {
        validate_relative_path(path)?;
        return Ok(path.clone().to_string());
    }
    if let Some(source) = &plugin.source {
        if let Some(path) = source.as_str() {
            validate_relative_path(path)?;
            return Ok(path.to_string());
        }
    }
    Err(format!(
        "channel plugin '{}' has no path/source string",
        plugin.id
    ))
}
