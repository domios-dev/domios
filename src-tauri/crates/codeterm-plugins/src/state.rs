use super::*;

pub(super) fn read_install_record(id: &str) -> Result<InstallRecord, String> {
    let id = PluginId::parse(id.to_string())?;
    let path = plugin_dir(&id).join(".codeterm-install.json");
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
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
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

pub(super) fn write_channel_registry(registry: &ChannelRegistry) -> Result<(), String> {
    let path = channel_registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(registry).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
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
