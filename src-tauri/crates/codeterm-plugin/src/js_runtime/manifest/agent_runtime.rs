use super::PluginManifest;
use crate::agent_runtime::{AgentRuntimeCapability, LaunchAuthority, RuntimeOperation};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeManifestError {
    CapabilityRequired,
    ChatContractRequired,
    InvalidCommand,
    ProfileRequired,
    UnknownProfile,
    InvalidProfile,
    SubprocessDenied,
    InvalidEnvironmentExclusion,
    InvalidMethods,
    ConsentRequired,
    BinaryUnavailable,
    BinaryUnknown,
}

impl std::fmt::Display for RuntimeManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "agentRuntime: {self:?}")
    }
}
impl std::error::Error for RuntimeManifestError {}

impl PluginManifest {
    pub fn validate_agent_runtime(&self) -> Result<(), RuntimeManifestError> {
        let Some(runtime) = self.capabilities.agent_runtime.as_ref() else {
            return Ok(());
        };
        if !self.capabilities.agent_provider {
            return Err(RuntimeManifestError::CapabilityRequired);
        }
        let chat = &self.capabilities.chat_backend;
        if !(chat.enabled && chat.spawnable && chat.session_config && chat.agent_tasks) {
            return Err(RuntimeManifestError::ChatContractRequired);
        }
        runtime.validate_declaration()?;
        let binary = &runtime.command.binary;
        let basename = std::path::Path::new(binary)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(binary);
        if !self
            .permissions
            .subprocess
            .allow
            .iter()
            .any(|allowed| allowed == binary || allowed == basename)
        {
            return Err(RuntimeManifestError::SubprocessDenied);
        }
        for profile in runtime
            .launch_profiles
            .iter()
            .flat_map(|profiles| profiles.values())
        {
            if !self.permissions.subprocess.allow.contains(&profile.binary) {
                return Err(RuntimeManifestError::SubprocessDenied);
            }
        }
        Ok(())
    }

    pub fn authorize_agent_runtime(
        &self,
        authority: &LaunchAuthority,
        profile: Option<&str>,
    ) -> Result<PathBuf, RuntimeManifestError> {
        self.validate_agent_runtime()?;
        if !authority.authorizes_provider(&self.id) {
            return Err(RuntimeManifestError::ConsentRequired);
        }
        self.capabilities
            .agent_runtime
            .as_ref()
            .ok_or(RuntimeManifestError::CapabilityRequired)?
            .resolve_profile_executable(profile)
    }
}

impl AgentRuntimeCapability {
    pub fn profile_command(
        &self,
        profile: Option<&str>,
    ) -> Result<crate::agent_runtime::AgentRuntimeCommand, RuntimeManifestError> {
        self.validate_declaration()?;
        match (&self.launch_profiles, profile) {
            (None, None) => Ok(self.command.clone()),
            (None, Some(_)) => Err(RuntimeManifestError::UnknownProfile),
            (Some(_), None) => Err(RuntimeManifestError::ProfileRequired),
            (Some(profiles), Some(name)) => profiles
                .get(name)
                .map(|profile| crate::agent_runtime::AgentRuntimeCommand {
                    binary: profile.binary.clone(),
                    args: profile.args.clone(),
                })
                .ok_or(RuntimeManifestError::UnknownProfile),
        }
    }

    pub fn resolve_profile_executable(
        &self,
        profile: Option<&str>,
    ) -> Result<PathBuf, RuntimeManifestError> {
        let command = self.profile_command(profile)?;
        resolve_binary(&command.binary)
    }

    pub fn resolve_executable(&self) -> Result<PathBuf, RuntimeManifestError> {
        self.resolve_profile_executable(None)
    }

    pub fn validate_declaration(&self) -> Result<(), RuntimeManifestError> {
        if self.host_mcp.as_ref().is_some_and(|declaration| {
            declaration.param.is_empty()
                || declaration.param.len() > 128
                || !declaration
                    .param
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || declaration.methods.is_empty()
                || declaration
                    .methods
                    .iter()
                    .enumerate()
                    .any(|(index, method)| {
                        declaration.methods[..index].contains(method)
                            || !self
                                .request_methods
                                .values()
                                .any(|methods| methods.contains(method))
                    })
        }) {
            return Err(RuntimeManifestError::InvalidMethods);
        }
        if self.interrupt.as_ref().is_some_and(|interrupt| {
            !interrupt.before_first_turn
                || self
                    .request_methods
                    .get("interrupt")
                    .is_none_or(Vec::is_empty)
        }) {
            return Err(RuntimeManifestError::InvalidMethods);
        }
        if self
            .client_bridges
            .as_ref()
            .is_some_and(|bridges| !bridges.fs_read && !bridges.fs_write && !bridges.terminal)
        {
            return Err(RuntimeManifestError::InvalidMethods);
        }
        if self
            .reset_credits_min_version
            .as_ref()
            .is_some_and(|minimum| semver::Version::parse(minimum).is_err() || !self.usage)
        {
            return Err(RuntimeManifestError::InvalidMethods);
        }
        if self.goals.as_ref().is_some_and(|goals| {
            !goals
                .actions
                .contains(&crate::agent_runtime::RuntimeGoalAction::Read)
                || goals
                    .actions
                    .iter()
                    .enumerate()
                    .any(|(index, action)| goals.actions[..index].contains(action))
        }) {
            return Err(RuntimeManifestError::InvalidMethods);
        }
        if self.thread_history.as_ref().is_some_and(|history| {
            history.actions.is_empty()
                || history
                    .actions
                    .iter()
                    .enumerate()
                    .any(|(index, action)| history.actions[..index].contains(action))
        }) {
            return Err(RuntimeManifestError::InvalidMethods);
        }
        validate_command(&self.command)?;
        if !crate::agent_runtime::limits_source::valid_registry_defaults(&self.registry_defaults) {
            return Err(RuntimeManifestError::InvalidProfile);
        }
        if let Some(profiles) = &self.launch_profiles {
            if profiles.len() > 128 {
                return Err(RuntimeManifestError::InvalidProfile);
            }
            for (name, profile) in profiles {
                if profile
                    .icon
                    .as_deref()
                    .is_some_and(|icon| !crate::plugin_icon::valid_relative_svg_path(icon))
                    || profile
                        .icon_url
                        .as_deref()
                        .is_some_and(|url| !crate::plugin_icon::registry::valid_url(url))
                    || profile.full_access_mode.as_deref().is_some_and(|mode| {
                        !crate::agent_runtime::RuntimeLaunchProfile::valid_session_mode(mode)
                    })
                    || profile.registry_id.as_deref().is_some_and(|id| {
                        id.is_empty()
                            || id.len() > 128
                            || !id
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    })
                {
                    return Err(RuntimeManifestError::InvalidProfile);
                }
                if name.is_empty()
                    || name.len() > 128
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                {
                    return Err(RuntimeManifestError::InvalidProfile);
                }
                validate_command(&crate::agent_runtime::AgentRuntimeCommand {
                    binary: profile.binary.clone(),
                    args: profile.args.clone(),
                })?;
                if profile.env.len() > 128
                    || profile.env.iter().any(|(key, value)| {
                        key.is_empty()
                            || key.len() > 128
                            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                            || key.to_ascii_uppercase().starts_with("CODETERM")
                            || value.len() > 16384
                            || value.contains('\0')
                    })
                {
                    return Err(RuntimeManifestError::InvalidProfile);
                }
                if profile.secret_env.len() > 32
                    || profile.secret_env.iter().any(|(key, value)| {
                        key.is_empty()
                            || key.len() > 128
                            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                            || key.to_ascii_uppercase().starts_with("CODETERM")
                            || value.is_empty()
                            || value.len() > 128
                            || !value
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
                    })
                {
                    return Err(RuntimeManifestError::InvalidProfile);
                }
                if !valid_auth_env(&profile.auth_env)
                    || profile
                        .limits_source
                        .as_ref()
                        .is_some_and(|source| !source.valid_for(profile))
                {
                    return Err(RuntimeManifestError::InvalidProfile);
                }
                if !crate::agent_runtime::install_validation::validate(profile) {
                    return Err(RuntimeManifestError::InvalidProfile);
                }
                if let Some(detection) = &profile.detection {
                    validate_command(&crate::agent_runtime::AgentRuntimeCommand {
                        binary: profile.binary.clone(),
                        args: detection.args.clone(),
                    })?;
                }
            }
        }
        let command = &self.command;
        let exclusions = &self.inherited_environment_exclusions;
        if exclusions.len() > 128
            || exclusions.iter().any(|name| {
                name.is_empty()
                    || name.len() > 128
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    || name.starts_with("CODETERM_")
                    || name == "CODETERM"
            })
            || has_duplicates(exclusions)
        {
            return Err(RuntimeManifestError::InvalidEnvironmentExclusion);
        }
        if let Some(login) = &self.login {
            if login.methods.is_empty()
                || login.methods.len() > 3
                || login
                    .methods
                    .iter()
                    .enumerate()
                    .any(|(index, method)| login.methods[..index].contains(method))
                || login.terminal_command.as_ref().is_some_and(|terminal| {
                    terminal.len() > 4096
                        || terminal.split_whitespace().next() != Some(command.binary.as_str())
                        || !terminal
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b" -_./\\:".contains(&byte))
                })
            {
                return Err(RuntimeManifestError::InvalidCommand);
            }
        }
        let kinds = [
            "backgroundProcesses",
            "initialize",
            "openThread",
            "forkThread",
            "resumeThread",
            "readHistory",
            "submitTurn",
            "interrupt",
            "answer",
            "answerQuestion",
            "control",
        ];
        if (self.background_processes
            && self
                .request_methods
                .get("backgroundProcesses")
                .is_none_or(|methods| methods.is_empty()))
            || has_duplicates(&self.turn_scoped_requests)
            || self.turn_scoped_requests.iter().any(|kind| {
                kind != "submitTurn"
                    || self
                        .request_methods
                        .get(kind)
                        .is_none_or(|methods| methods.is_empty())
            })
            || self.request_methods.is_empty()
            || self
                .request_methods
                .iter()
                .any(|(kind, methods)| !kinds.contains(&kind.as_str()) || !valid_methods(methods))
            || !valid_methods(&self.notification_methods)
        {
            return Err(RuntimeManifestError::InvalidMethods);
        }
        Ok(())
    }

    pub fn permits_method(&self, operation: &RuntimeOperation, method: &str) -> bool {
        self.request_methods
            .get(operation.kind())
            .is_some_and(|methods| methods.iter().any(|allowed| allowed == method))
    }
}

fn has_duplicates(values: &[String]) -> bool {
    let mut seen = std::collections::HashSet::new();
    values.iter().any(|value| !seen.insert(value))
}

fn valid_methods(methods: &[String]) -> bool {
    methods.len() <= 128
        && !has_duplicates(methods)
        && methods.iter().all(|method| {
            !method.is_empty()
                && method.len() <= 256
                && !method
                    .bytes()
                    .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        })
}

fn valid_auth_env(
    auth_env: &std::collections::BTreeMap<String, crate::agent_runtime::RuntimeProfileAuthEnv>,
) -> bool {
    let mut methods = std::collections::HashSet::new();
    auth_env.len() <= 32
        && auth_env.iter().all(|(key, entry)| {
            !key.is_empty()
                && key.len() <= 128
                && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && !key.to_ascii_uppercase().starts_with("CODETERM")
                && !entry.secret.is_empty()
                && entry.secret.len() <= 128
                && entry
                    .secret
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
                && !entry.methods.is_empty()
                && entry.methods.len() <= 8
                && valid_methods(&entry.methods)
                && entry.methods.iter().all(|method| methods.insert(method))
        })
}

#[cfg(test)]
mod tests;

fn validate_command(
    command: &crate::agent_runtime::AgentRuntimeCommand,
) -> Result<(), RuntimeManifestError> {
    let executable = command
        .binary
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&command.binary)
        .to_ascii_lowercase();
    let executable = executable.strip_suffix(".exe").unwrap_or(&executable);
    if [
        "sh",
        "bash",
        "zsh",
        "fish",
        "dash",
        "csh",
        "tcsh",
        "ksh",
        "cmd",
        "powershell",
        "pwsh",
    ]
    .contains(&executable)
    {
        return Err(RuntimeManifestError::InvalidCommand);
    }

    if command.binary.is_empty()
        || command.binary.len() > 4096
        || command.binary.contains(['\0', '\r', '\n'])
        || command.args.len() > 128
        || command
            .args
            .iter()
            .any(|s| s.len() > 16384 || s.contains('\0'))
    {
        return Err(RuntimeManifestError::InvalidCommand);
    }
    Ok(())
}

fn resolve_binary(binary: &str) -> Result<PathBuf, RuntimeManifestError> {
    match super::super::probe_bin(binary).resolution {
        super::super::BinResolution::Found(path) => Ok(path),
        super::super::BinResolution::Missing => Err(RuntimeManifestError::BinaryUnavailable),
        super::super::BinResolution::Unknown => Err(RuntimeManifestError::BinaryUnknown),
    }
}
