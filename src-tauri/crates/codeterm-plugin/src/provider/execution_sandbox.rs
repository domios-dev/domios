use codeterm_types::plan::ExecutionIntent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionSandboxCapability {
    pub platforms: Vec<String>,
    pub read_only: Option<ExecutionSandboxMode>,
    pub runtime_read_only_writable_roots_mode: Option<String>,
    pub mutating: Option<ExecutionSandboxMode>,
    pub unsupported_reason: Option<String>,
    pub forbidden_argument_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionSandboxMode {
    pub launch_args: Vec<String>,
    pub runtime_mode: Option<String>,
}

impl ExecutionSandboxCapability {
    pub fn resolve(
        &self,
        intent: &ExecutionIntent,
        platform: &str,
        runtime: bool,
    ) -> Result<Option<ExecutionSandboxMode>, String> {
        if *intent == ExecutionIntent::Unspecified {
            return Ok(None);
        }
        let mode = match intent {
            ExecutionIntent::ReadOnly => self.read_only.as_ref(),
            ExecutionIntent::Mutating => self.mutating.as_ref(),
            ExecutionIntent::Unspecified => None,
        };
        if let Some(mode) = mode.filter(|mode| {
            self.platforms.iter().any(|supported| supported == platform)
                && if runtime {
                    mode.runtime_mode.is_some()
                } else {
                    !mode.launch_args.is_empty()
                }
        }) {
            return Ok(Some(mode.clone()));
        }
        if *intent == ExecutionIntent::Mutating && self.mutating.is_none() {
            return Ok(None);
        }
        Err(format!(
            "Cannot enforce {intent:?} execution sandbox on {platform} ({}): {}",
            if runtime { "runtime" } else { "PTY" },
            self.unsupported_reason
                .as_deref()
                .unwrap_or("provider declares no filesystem confinement for this execution intent")
        ))
    }

    pub fn validate_arguments(&self, arguments: &[String]) -> Result<(), String> {
        if let Some(argument) = arguments.iter().find(|argument| {
            self.forbidden_argument_prefixes
                .iter()
                .any(|prefix| argument.starts_with(prefix))
        }) {
            return Err(format!(
                "Execution sandbox refuses overriding argument: {argument}"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
