use super::*;

pub(super) fn update_command(id: Option<String>) -> Result<serde_json::Value, String> {
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

pub(super) fn update_one(id: &str) -> Result<(), String> {
    let parsed_id = PluginId::parse(id.to_string())?;
    let record = read_install_record(id)?;
    let before = manifest_version(&plugin_dir(&parsed_id)).ok().flatten();
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
            let ch = reg
                .channels
                .get(&channel)
                .ok_or_else(|| format!("channel '{channel}' not registered"))?;
            let source = Path::new(&ch.path).join(&path);
            let after = manifest_version(&source).ok().flatten();
            let next_record = InstallRecord {
                installed_at: now_rfc3339(),
                version: after.clone(),
                last_change: Some(last_change_for_update(
                    before.clone(),
                    after.clone(),
                    Some(channel.clone()),
                )),
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
            let ch = reg
                .channels
                .get(&channel)
                .ok_or_else(|| format!("channel '{channel}' not registered"))?;
            let source = Path::new(&ch.path).join(&path);
            let after = manifest_version(&source).ok().flatten();
            let next_record = InstallRecord {
                installed_at: now_rfc3339(),
                version: after.clone(),
                last_change: Some(last_change_for_update(
                    before.clone(),
                    after.clone(),
                    Some(channel.clone()),
                )),
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

pub(super) fn update_system_command(id: Option<String>) -> Result<serde_json::Value, String> {
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
                .ok_or_else(|| {
                    format!("'{id}' is not listed in the {SYSTEM_CHANNEL_NAME} channel")
                })?;
            vec![plugin]
        }
        None => manifest.plugins,
    };
    let mut updated = Vec::new();
    let mut blocked = Vec::new();
    for plugin in requested {
        let id = plugin.id.clone();
        let parsed_id = PluginId::parse(id.clone())?;
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
            last_change: Some(last_change_for_install(
                version,
                Some(SYSTEM_CHANNEL_NAME.to_string()),
            )),
        };
        let before = manifest_version(&plugin_dir(&parsed_id)).ok().flatten();
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

pub(super) fn removed_system_channel_warnings(
    current_ids: &[String],
) -> Result<Vec<String>, String> {
    let current: std::collections::BTreeSet<_> = current_ids.iter().cloned().collect();
    let mut warnings = Vec::new();
    for id in installed_ids()? {
        let record = read_install_record(&id).ok();
        let Some(InstallRecord {
            source: InstallSource::SystemChannel { channel, .. },
            ..
        }) = record
        else {
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
pub(super) fn ensure_system_channel() -> Result<(), String> {
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

/// Register the default marketplace channel (`codeterm-plugins`) if it is not
/// already registered with the current URL. Register-only: `add_channel`
/// clones the channel manifest and writes a registry entry but installs
/// nothing, so the channel's plugins surface as *available* in Extensions
/// without being installed by default. Requires network on first run; callers
/// invoke it best-effort so an offline launch is a silent no-op (retried next
/// time the marketplace is queried).
pub(super) fn ensure_default_plugin_channel() -> Result<(), String> {
    // Every snapshot calls this; under rapid polling many threads would clone the same
    // missing channel at once. try_lock: one ensure in flight, a slow clone stalls nobody.
    static ENSURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Ok(_guard) = ENSURE_LOCK.try_lock() else {
        return Ok(());
    };
    let url = default_plugin_channel_url();
    let reg = read_channel_registry().unwrap_or_default();
    let needs_add = match reg.channels.get(DEFAULT_PLUGIN_CHANNEL_NAME) {
        Some(ch) => ch.url != url,
        None => true,
    };
    if !needs_add {
        return Ok(());
    }
    add_channel(&url, Some(DEFAULT_PLUGIN_CHANNEL_NAME.to_string()))?;
    Ok(())
}
