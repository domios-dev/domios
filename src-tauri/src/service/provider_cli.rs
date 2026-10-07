use std::path::PathBuf;

use crate::agent::provider::ProviderAvailability;
use crate::agent::registry::AgentRegistry;
use codeterm_plugin::js_runtime::{probe_bin, BinProbe, BinResolution};

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ProviderCliStatus {
    pub installed: Option<bool>,
    pub binary: String,
    pub resolved_path: Option<PathBuf>,
    pub availability: ProviderAvailability,
    pub searched: Vec<PathBuf>,
    pub installer: Option<String>,
    pub updater: Option<String>,
}

pub(crate) fn provider_cli_status(
    registry: &AgentRegistry,
    provider_id: &str,
    platform: &str,
) -> Result<ProviderCliStatus, String> {
    let (binary, installer, updater) = registry
        .with_provider(provider_id, |provider| {
            (
                provider
                    .cli_binary()
                    .unwrap_or(provider.binary_name())
                    .to_owned(),
                provider.install_command(platform),
                provider.update_command(platform),
            )
        })
        .ok_or_else(|| format!("Unknown provider: {provider_id}"))?;
    Ok(observe_cli(binary, installer, updater))
}

pub(crate) fn observe_cli(
    binary: String,
    installer: Option<String>,
    updater: Option<String>,
) -> ProviderCliStatus {
    let probe = probe_bin(&binary);
    status_from_probe(binary, probe, installer, updater)
}

fn status_from_probe(
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
        binary,
        resolved_path,
        availability,
        searched: probe.searched,
        installer: installer.filter(|command| !command.trim().is_empty()),
        updater: updater.filter(|command| !command.trim().is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_cli_preserves_path_and_exact_commands() {
        let path = PathBuf::from("/tools/agent");
        let searched = vec![PathBuf::from("/tools")];
        let status = status_from_probe(
            "agent".into(),
            BinProbe {
                resolution: BinResolution::Found(path.clone()),
                searched: searched.clone(),
            },
            Some("  install --flag 'opaque token'  ".into()),
            Some("update --flag".into()),
        );
        assert_eq!(status.installed, Some(true));
        assert_eq!(status.resolved_path, Some(path));
        assert_eq!(status.availability, ProviderAvailability::Available);
        assert_eq!(status.searched, searched);
        assert_eq!(
            status.installer.as_deref(),
            Some("  install --flag 'opaque token'  ")
        );
        assert_eq!(status.updater.as_deref(), Some("update --flag"));
    }

    #[test]
    fn missing_and_unknown_are_distinct() {
        for (resolution, installed, availability) in [
            (
                BinResolution::Missing,
                Some(false),
                ProviderAvailability::Unavailable,
            ),
            (BinResolution::Unknown, None, ProviderAvailability::Unknown),
        ] {
            let status = status_from_probe(
                "agent".into(),
                BinProbe {
                    resolution,
                    searched: Vec::new(),
                },
                Some(" \n ".into()),
                None,
            );
            assert_eq!(status.installed, installed);
            assert_eq!(status.availability, availability);
            assert_eq!(status.resolved_path, None);
            assert_eq!(status.installer, None);
            assert_eq!(status.updater, None);
        }
    }
}
