use super::*;

pub(super) fn update_source(source: Option<&InstallSource>) -> UpdateSource<'_> {
    match source {
        Some(InstallSource::Dir { .. }) => UpdateSource::Local,
        Some(
            InstallSource::Channel { channel, .. } | InstallSource::SystemChannel { channel, .. },
        ) => UpdateSource::Channel(channel),
        None => UpdateSource::Unknown,
    }
}

pub(super) fn installed_update_status(
    id: &str,
    version: Option<&str>,
    source: Option<&InstallSource>,
) -> serde_json::Value {
    let source = effective_install_source(id, source);
    let source = source.as_ref();
    let mut basis = "channel_cache";
    let mut channel_name = None;
    let mut remote_version = None;
    let mut payload_version = None;
    let mut compatible = false;
    if let UpdateSource::Channel(channel) = update_source(source) {
        let registry = read_channel_registry().unwrap_or_default();
        if let Some(ch) = registry.channels.get(channel) {
            if let Ok(manifest) = read_channel_manifest(Path::new(&ch.path)) {
                if let Some(plugin) = manifest.plugins.iter().find(|plugin| plugin.id == id) {
                    if let Ok(path) = plugin_path(plugin) {
                        let dir = Path::new(&ch.path).join(path);
                        payload_version = manifest_version(&dir).ok().flatten();
                        remote_version = plugin.version.clone().or_else(|| payload_version.clone());
                        let payload_min = manifest_min_codeterm(&dir);
                        compatible = compat_check(plugin.min_codeterm.as_deref()).is_ok()
                            && compat_check(payload_min.as_deref()).is_ok();
                        channel_name = Some(channel);
                    }
                }
            }
        }
    }
    if channel_name.is_none()
        && matches!(source, Some(InstallSource::SystemChannel { channel, .. }) if channel == SYSTEM_CHANNEL_NAME)
    {
        if let Some((_, manifest)) = bundled_metadata(id) {
            payload_version = manifest.version.clone();
            remote_version = manifest.version;
            compatible = compat_check(manifest.min_codeterm.as_deref()).is_ok();
            channel_name = Some(SYSTEM_CHANNEL_NAME);
            basis = "bundled_manifest";
        }
    }
    let offer = channel_name.map(|channel| UpdateOffer {
        channel,
        version: remote_version.as_deref(),
        payload_version: payload_version.as_deref(),
        compatible,
    });
    let decision = update_availability(version, update_source(source), offer);
    serde_json::json!({
        "available": decision.available,
        "reason": decision.reason,
        "channel": match update_source(source) { UpdateSource::Channel(channel) => Some(channel), _ => None },
        "remote_version": remote_version,
        "basis": basis,
    })
}
