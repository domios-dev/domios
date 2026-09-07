use crate::agent::provider::{
    observe_launch_binary, LaunchBinaryObservation, ProviderAvailability,
};
use crate::agent::registry::AgentRegistry;
use crate::multiplexer::state::MultiplexerState;

fn valid_cli_binary(binary: &str) -> bool {
    !binary.is_empty()
        && !binary.chars().any(|c| {
            matches!(
                c,
                '/' | '\\' | ' ' | ';' | '|' | '&' | '$' | '`' | '<' | '>' | '\n'
            )
        })
}

/// Installation probe for the Settings card. An explicit user re-check must
/// re-walk the ladder, so it drops that binary's memo first.
pub(crate) fn cli_binary_on_path(binary: &str) -> bool {
    if !valid_cli_binary(binary) {
        return false;
    }
    codeterm_plugin::js_runtime::forget_resolved_bin(binary);
    observe_launch_binary(Some(binary)).availability == ProviderAvailability::Available
}

/// Why a launch was refused before anything was created.
#[derive(Debug, Clone)]
pub(crate) enum ProviderPreflightError {
    UnknownProvider {
        provider_id: String,
        available: Vec<String>,
    },
    InvalidBinary {
        provider_id: String,
        binary: String,
    },
    /// The shared resolver searched and did not find the launch binary. This is
    /// the observed negative, never an unanswered probe.
    BinaryUnavailable {
        provider_id: String,
        display_name: String,
        binary: String,
        searched: Vec<std::path::PathBuf>,
        install_hint: Option<String>,
    },
}

impl ProviderPreflightError {
    #[cfg(test)]
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::UnknownProvider { .. } => "unknown_provider",
            Self::InvalidBinary { .. } => "invalid_provider_binary",
            Self::BinaryUnavailable { .. } => "provider_binary_unavailable",
        }
    }
}

impl std::fmt::Display for ProviderPreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProvider {
                provider_id,
                available,
            } => write!(
                f,
                "Unknown agent provider: '{}'. Available: [{}]",
                provider_id,
                available.join(", ")
            ),
            Self::InvalidBinary {
                provider_id,
                binary,
            } => write!(
                f,
                "Provider '{}' declares invalid CLI binary '{}'",
                provider_id, binary
            ),
            Self::BinaryUnavailable {
                provider_id,
                display_name,
                binary,
                searched,
                install_hint,
                ..
            } => write!(
                f,
                "Provider not installed: {} (`{}`) launches `{}`, which is not on this machine. Searched: [{}]. Install command: {}",
                display_name,
                provider_id,
                binary,
                searched
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
                install_hint
                    .as_deref()
                    .unwrap_or("No install command is available for this platform.")
            ),
        }
    }
}

/// Proof that a provider's launch binary was observed BEFORE a tab existed.
/// `create_cleared_agent_tab` takes one, so a refusal cannot leave a tab behind.
#[derive(Debug)]
pub(crate) struct LaunchClearance {
    pub(crate) warning: Option<String>,
}

/// The launch gate. Refuses only the observed negative: an unanswered probe is
/// not a veto, so an `unknown` provider is cleared with a warning that says so.
pub(crate) fn provider_launch_preflight(
    registry: &AgentRegistry,
    provider_id: &str,
) -> Result<LaunchClearance, ProviderPreflightError> {
    let Some((display_name, launch_binary, install_command)) =
        registry.with_provider(provider_id, |p| {
            (
                p.display_name().to_string(),
                p.binary_name().to_string(),
                p.install_command(crate::agent::provider::current_platform_tag()),
            )
        })
    else {
        return Err(ProviderPreflightError::UnknownProvider {
            provider_id: provider_id.to_string(),
            available: registry.provider_ids(),
        });
    };
    let binary = launch_binary;
    if !valid_cli_binary(&binary) {
        return Err(ProviderPreflightError::InvalidBinary {
            provider_id: provider_id.to_string(),
            binary,
        });
    }
    let observed = observe_launch_binary(Some(&binary));
    clearance_for(
        provider_id,
        &display_name,
        &binary,
        observed,
        install_command.filter(|s| !s.trim().is_empty()),
    )
}

pub(crate) fn clearance_for(
    provider_id: &str,
    display_name: &str,
    binary: &str,
    observed: LaunchBinaryObservation,
    install_hint: Option<String>,
) -> Result<LaunchClearance, ProviderPreflightError> {
    match observed.availability {
        ProviderAvailability::Available => Ok(LaunchClearance { warning: None }),
        ProviderAvailability::Unavailable => Err(ProviderPreflightError::BinaryUnavailable {
            provider_id: provider_id.to_string(),
            display_name: display_name.to_string(),
            binary: binary.to_string(),
            searched: observed.searched,
            install_hint,
        }),
        ProviderAvailability::Unknown => Ok(LaunchClearance {
            warning: Some(format!(
                "Provider availability unknown: could not check whether `{}` is installed for {}. Launching anyway.",
                binary, display_name
            )),
        }),
    }
}

/// The only tab-creation entry point on the agent spawn path. Taking the
/// clearance is what makes "refused before a tab exists" a compile-time fact
/// rather than a comment about statement order.
pub(crate) fn create_cleared_agent_tab(
    mux: &mut MultiplexerState,
    workspace_id: &str,
    _clearance: &LaunchClearance,
) -> Result<String, String> {
    mux.create_tab(workspace_id)
}
