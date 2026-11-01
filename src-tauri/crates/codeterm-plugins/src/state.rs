use super::*;

pub(super) fn read_install_record(id: &str) -> Result<InstallRecord, String> {
    let id = PluginId::parse(id.to_string())?;
    let path = plugin_dir(&id).join(".codeterm-install.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

pub(super) fn read_install_record_if_present(id: &str) -> Result<Option<InstallRecord>, String> {
    let id = PluginId::parse(id.to_string())?;
    let path = plugin_dir(&id).join(".codeterm-install.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("parse {}: {error}", path.display()))
}

pub(super) fn installed_ids() -> Result<Vec<String>, String> {
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

pub(super) fn read_channel_registry() -> Result<ChannelRegistry, String> {
    let path = channel_registry_path();
    if !path.exists() {
        return Ok(ChannelRegistry::default());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut registry: ChannelRegistry =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    // A relocated data root keeps its registry, so a stored absolute cache path would recreate the old root.
    for record in registry.channels.values_mut() {
        record.path = channels_root().join(&record.name).display().to_string();
    }
    migrate_system_channel(&mut registry, &system_channel_url());
    Ok(registry)
}

fn migrate_system_channel(registry: &mut ChannelRegistry, url: &str) -> bool {
    let Some(record) = registry.channels.get_mut(SYSTEM_CHANNEL_NAME) else {
        return false;
    };
    if record.url == url {
        return false;
    }
    record.url = url.to_string();
    record.kind = if looks_like_local_path(&parse_channel_source(url).base) {
        ChannelKind::Local
    } else if archive_channel::is_archive_url(url) {
        ChannelKind::Archive
    } else {
        ChannelKind::Git
    };
    true
}

static CHANNEL_REGISTRY_MUTATION: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn mutate_channel_registry<T>(
    mutate: impl FnOnce(&mut ChannelRegistry) -> Result<T, String>,
) -> Result<T, String> {
    mutate_channel_registry_with(mutate, persist_channel_registry)
}

fn mutate_channel_registry_with<T>(
    mutate: impl FnOnce(&mut ChannelRegistry) -> Result<T, String>,
    persist: impl FnOnce(&ChannelRegistry) -> Result<(), String>,
) -> Result<T, String> {
    let _guard = CHANNEL_REGISTRY_MUTATION
        .lock()
        .map_err(|_| "Plugin channel registry lock poisoned")?;
    let mut registry = read_channel_registry()?;
    let result = mutate(&mut registry)?;
    persist(&registry)?;
    Ok(result)
}

#[cfg(test)]
pub(super) fn write_channel_registry(registry: &ChannelRegistry) -> Result<(), String> {
    let _guard = CHANNEL_REGISTRY_MUTATION
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    persist_channel_registry(registry)
}

fn persist_channel_registry(registry: &ChannelRegistry) -> Result<(), String> {
    let mut registry = registry.clone();
    migrate_system_channel(&mut registry, &system_channel_url());
    let path = channel_registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&registry).map_err(|e| e.to_string())?;
    codeterm_util::atomic_write::write_atomic(&path, text)
        .map_err(|e| format!("write {}: {e}", path.display()))
}

pub(super) fn plugins_root() -> PathBuf {
    plugin_data_root().join("plugins")
}

pub(super) fn plugin_dir(id: &PluginId) -> PathBuf {
    plugins_root().join(id.as_str())
}

pub(super) fn ensure_plugin_path_inside(path: &Path) -> Result<(), String> {
    let root = plugins_root();
    fs::create_dir_all(&root)
        .map_err(|e| format!("create plugins root {}: {e}", root.display()))?;
    let root = root
        .canonicalize()
        .map_err(|e| format!("canonicalize plugins root: {e}"))?;
    let resolved = if path.exists() {
        path.canonicalize()
            .map_err(|e| format!("canonicalize plugin path {}: {e}", path.display()))?
    } else {
        root.join(
            path.file_name()
                .ok_or("plugin path has no final component")?,
        )
    };
    if resolved == root || !resolved.starts_with(&root) {
        return Err(format!(
            "plugin path {} is outside the plugins root",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn channels_root() -> PathBuf {
    plugin_data_root().join("channels")
}

pub(super) fn channel_registry_path() -> PathBuf {
    plugin_data_root().join("plugin-channels.json")
}

pub(super) fn plugin_data_root() -> PathBuf {
    data_root()
}

pub(super) fn audit_log_path() -> PathBuf {
    plugin_data_root().join("logs").join("plugin-events.log")
}

#[cfg(test)]
mod tests;
