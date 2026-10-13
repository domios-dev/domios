use super::PluginManifest;
use crate::agent_runtime::{AgentRuntimeCapability, LaunchAuthority, RuntimeOperation};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeManifestError {
    CapabilityRequired,
    ChatContractRequired,
    InvalidCommand,
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
            .and_then(|s| s.to_str())
            .unwrap_or(binary);
        if !self
            .permissions
            .subprocess
            .allow
            .iter()
            .any(|s| s == binary || s == basename)
        {
            return Err(RuntimeManifestError::SubprocessDenied);
        }
        Ok(())
    }

    pub fn authorize_agent_runtime(
        &self,
        authority: &LaunchAuthority,
    ) -> Result<PathBuf, RuntimeManifestError> {
        self.validate_agent_runtime()?;
        if !authority.authorizes_provider(&self.id) {
            return Err(RuntimeManifestError::ConsentRequired);
        }
        self.capabilities
            .agent_runtime
            .as_ref()
            .ok_or(RuntimeManifestError::CapabilityRequired)?
            .resolve_executable()
    }
}

impl AgentRuntimeCapability {
    pub fn resolve_executable(&self) -> Result<PathBuf, RuntimeManifestError> {
        self.validate_declaration()?;
        match super::super::probe_bin(&self.command.binary).resolution {
            super::super::BinResolution::Found(path) => Ok(path),
            super::super::BinResolution::Missing => Err(RuntimeManifestError::BinaryUnavailable),
            super::super::BinResolution::Unknown => Err(RuntimeManifestError::BinaryUnknown),
        }
    }

    pub fn validate_declaration(&self) -> Result<(), RuntimeManifestError> {
        let command = &self.command;
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
        let kinds = [
            "initialize",
            "openThread",
            "resumeThread",
            "readHistory",
            "submitTurn",
            "interrupt",
            "answer",
            "answerQuestion",
            "control",
        ];
        if self.request_methods.is_empty()
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

#[cfg(test)]
mod tests;
