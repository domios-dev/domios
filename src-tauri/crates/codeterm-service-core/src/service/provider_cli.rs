pub(crate) use codeterm_plugin::provider_cli::{observe_cli, ProviderCliStatus};
pub fn provider_cli_status(
    registry: &crate::provider::ProviderRegistry,
    provider_id: &str,
    platform: &str,
) -> Result<ProviderCliStatus, String> {
    registry.cli_status(provider_id, platform)
}

#[cfg(test)]
mod tests {
    use crate::agent::provider::ProviderAvailability;
    use codeterm_plugin::js_runtime::{BinProbe, BinResolution};
    use codeterm_plugin::provider_cli::status_from_probe;
    use std::path::PathBuf;

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
