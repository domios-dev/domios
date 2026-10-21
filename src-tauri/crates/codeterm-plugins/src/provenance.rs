use super::*;

pub(super) fn bundled_metadata(id: &str) -> Option<(&'static str, PluginManifest)> {
    let (_, directory, manifest) = BUNDLED_PLUGINS
        .iter()
        .find(|(plugin, _, _)| *plugin == id)?;
    Some((directory, serde_json::from_str(manifest).ok()?))
}

pub(super) struct PluginProvenance {
    pub source: Option<InstallSource>,
    pub version: Option<String>,
    pub system: bool,
}

pub(super) fn classify_plugin(
    recorded: Option<&InstallSource>,
    installed_version: Option<&str>,
    bundled: Option<(&str, Option<&str>)>,
) -> PluginProvenance {
    PluginProvenance {
        source: recorded.cloned().or_else(|| {
            bundled.map(|(path, _)| InstallSource::SystemChannel {
                channel: SYSTEM_CHANNEL_NAME.into(),
                path: path.into(),
            })
        }),
        version: installed_version
            .map(String::from)
            .or_else(|| bundled.and_then(|(_, version)| version).map(String::from)),
        system: bundled.is_some(),
    }
}

pub(super) fn plugin_provenance(
    id: &str,
    recorded: Option<&InstallSource>,
    installed_version: Option<&str>,
) -> PluginProvenance {
    let metadata = bundled_metadata(id);
    classify_plugin(
        recorded,
        installed_version,
        metadata
            .as_ref()
            .map(|(directory, manifest)| (*directory, manifest.version.as_deref())),
    )
}

pub(super) fn effective_install_source(
    id: &str,
    recorded: Option<&InstallSource>,
) -> Option<InstallSource> {
    plugin_provenance(id, recorded, None).source
}

#[cfg(test)]
mod tests;
