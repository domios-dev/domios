use crate::js_runtime::{probe_bin, BinProbe, BinResolution};
use crate::provider::ProviderAvailability;
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ProviderCliStatus {
    pub installed: Option<bool>,
    pub required: bool,
    pub binary: Option<String>,
    pub resolved_path: Option<PathBuf>,
    pub availability: ProviderAvailability,
    pub searched: Vec<PathBuf>,
    pub installer: Option<String>,
    pub updater: Option<String>,
}

pub fn observe_cli(
    binary: String,
    installer: Option<String>,
    updater: Option<String>,
) -> ProviderCliStatus {
    let probe = probe_bin(&binary);
    status_from_probe(binary, probe, installer, updater)
}

pub fn status_from_probe(
    binary: String,
    probe: BinProbe,
    installer: Option<String>,
    updater: Option<String>,
) -> ProviderCliStatus {
    let availability = ProviderAvailability::from_resolution(&probe.resolution);
    let (installed, resolved_path) = match probe.resolution {
        BinResolution::Found(path) => (Some(true), Some(path)),
        BinResolution::Missing => (Some(false), None),
        BinResolution::Unknown => (None, None),
    };
    ProviderCliStatus {
        installed,
        required: true,
        binary: Some(binary),
        resolved_path,
        availability,
        searched: probe.searched,
        installer: installer.filter(|command| !command.trim().is_empty()),
        updater: updater.filter(|command| !command.trim().is_empty()),
    }
}

pub fn no_cli() -> ProviderCliStatus {
    ProviderCliStatus {
        required: false,
        installed: None,
        binary: None,
        resolved_path: None,
        availability: ProviderAvailability::Available,
        searched: Vec::new(),
        installer: None,
        updater: None,
    }
}

pub fn manifest_cli_status(
    manifest: &crate::js_runtime::manifest::PluginManifest,
    platform: &str,
) -> Option<ProviderCliStatus> {
    if manifest.capabilities.chat_backend.enabled() {
        return Some(no_cli());
    }
    if !manifest.capabilities.agent_provider {
        return None;
    }
    Some(observe_cli(
        manifest
            .cli_binary
            .as_deref()
            .or(manifest.binary_name.as_deref())
            .unwrap_or(&manifest.id)
            .to_owned(),
        manifest.installer.get(platform).cloned(),
        manifest.updater.get(platform).cloned(),
    ))
}
