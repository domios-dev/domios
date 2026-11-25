use crate::agent::provider::{LaunchBinaryObservation, ProviderAvailability};
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

/// Why a launch was refused before anything was created.
#[derive(Debug, Clone)]
pub enum ProviderPreflightError {
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
                "Provider not installed: {} (`{}`) launches `{}`, which is not on this machine. Searched: [{}]. Install command: {}. fix: domios agent cli install {}",
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
                    .unwrap_or("No install command is available for this platform."),
                provider_id
            ),
        }
    }
}

/// Proof that a provider's launch binary was observed BEFORE a tab existed.
/// `create_cleared_agent_tab` takes one, so a refusal cannot leave a tab behind.
#[derive(Debug)]
pub struct LaunchClearance {
    pub(crate) warning: Option<String>,
}

/// The launch gate checks provider registration and the launch binary.
pub fn provider_launch_preflight(
    registry: &AgentRegistry,
    provider_id: &str,
    permission_bypassed: bool,
) -> Result<LaunchClearance, ProviderPreflightError> {
    provider_launch_preflight_for_platform(
        registry,
        provider_id,
        permission_bypassed,
        crate::agent::provider::current_platform_tag(),
    )
}

fn valid_runtime_binary(binary: &str) -> bool {
    !binary.is_empty() && !binary.chars().any(|c| matches!(c, '\0' | '\r' | '\n'))
}

pub(crate) fn provider_launch_preflight_for_platform(
    registry: &AgentRegistry,
    provider_id: &str,
    _permission_bypassed: bool,
    platform: &str,
) -> Result<LaunchClearance, ProviderPreflightError> {
    let Some((display_name, launch_binary, is_runtime, install_command, install_dirs)) =
        registry.with_provider(provider_id, |p| {
            let runtime_binary = p
                .agent_runtime()
                .filter(|runtime| runtime.launch_profiles.is_none())
                .map(|runtime| runtime.command.binary);
            let is_runtime = runtime_binary.is_some();
            let binary = runtime_binary.unwrap_or_else(|| p.binary_name().to_string());
            (
                p.display_name().to_string(),
                binary,
                is_runtime,
                p.install_command(platform),
                p.cli_install_dirs(platform),
            )
        })
    else {
        return Err(ProviderPreflightError::UnknownProvider {
            provider_id: provider_id.to_string(),
            available: registry.provider_ids(),
        });
    };
    let binary = launch_binary;
    let valid = if is_runtime {
        valid_runtime_binary(&binary)
    } else {
        valid_cli_binary(&binary)
    };
    if !valid {
        return Err(ProviderPreflightError::InvalidBinary {
            provider_id: provider_id.to_string(),
            binary,
        });
    }
    let probe = codeterm_plugin::js_runtime::probe_bin_with_extra_dirs(&binary, &install_dirs);
    let status = codeterm_plugin::provider_cli::status_from_probe(
        binary.clone(),
        probe,
        install_command,
        None,
    );
    let observed = LaunchBinaryObservation {
        availability: status.availability,
        binary: status.binary,
        searched: status.searched,
    };
    clearance_for(
        provider_id,
        &display_name,
        &binary,
        observed,
        status.installer,
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
pub fn create_cleared_agent_tab(
    mux: &mut MultiplexerState,
    workspace_id: &str,
    _clearance: &LaunchClearance,
    gate: &super::billing::LimitGate,
) -> Result<String, String> {
    super::billing::admit_new_tab(gate, mux)?;
    let tab_id = mux.create_tab(workspace_id)?;
    crate::startup::boot_timing::bind_request(&tab_id);
    Ok(tab_id)
}
