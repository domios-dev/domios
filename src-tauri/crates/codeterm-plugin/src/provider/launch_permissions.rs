//! Launch-scoped permission grants a provider declares. They reach the agent
//! only through its own launch arguments, never through the provider's global
//! settings files, so every CodeTerm instance launches with the same grants.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct LaunchPermissionCapability {
    /// Models whose default permission mode runs tool calls unattended without
    /// full access. Empty means the provider makes no claim about its models.
    pub unattended_mode_models: Vec<String>,
    /// Allow-rule template for one shell command prefix; `{command}` is substituted.
    pub command_rule_format: Option<String>,
    /// Launch args carrying one allow rule; `{rule}` is substituted.
    pub allow_rule_args_format: Vec<String>,
    /// Launch args denying one tool for the session; `{tool}` is substituted.
    pub deny_tool_args_format: Vec<String>,
    /// Tools that start a provider-native subagent inside the session.
    pub native_subagent_tools: Vec<String>,
}

impl LaunchPermissionCapability {
    /// Whether `model` runs unattended in the provider's default mode. `None`
    /// when the provider declares no per-model modes.
    pub fn model_runs_unattended(&self, model: &str) -> Option<bool> {
        if self.unattended_mode_models.is_empty() {
            return None;
        }
        let model = model.trim();
        Some(
            self.unattended_mode_models
                .iter()
                .any(|declared| declared.eq_ignore_ascii_case(model)),
        )
    }

    /// Raw argv elements that allow each command prefix; empty when the
    /// provider cannot grant command rules at launch.
    pub fn allow_command_args(&self, commands: &[&str]) -> Vec<String> {
        let Some(rule_format) = self.command_rule_format.as_deref() else {
            return Vec::new();
        };
        if self.allow_rule_args_format.is_empty() {
            return Vec::new();
        }
        commands
            .iter()
            .flat_map(|command| {
                let rule = rule_format.replace("{command}", command);
                self.allow_rule_args_format
                    .iter()
                    .map(move |arg| arg.replace("{rule}", &rule))
            })
            .collect()
    }

    /// Raw argv elements that remove the provider's native subagent tools.
    pub fn deny_native_subagent_args(&self) -> Vec<String> {
        if self.deny_tool_args_format.is_empty() {
            return Vec::new();
        }
        self.native_subagent_tools
            .iter()
            .flat_map(|tool| {
                self.deny_tool_args_format
                    .iter()
                    .map(move |arg| arg.replace("{tool}", tool))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability() -> LaunchPermissionCapability {
        LaunchPermissionCapability {
            unattended_mode_models: vec!["alpha".into()],
            command_rule_format: Some("Shell({command}:*)".into()),
            allow_rule_args_format: vec!["--allow".into(), "{rule}".into()],
            deny_tool_args_format: vec!["--deny".into(), "{tool}".into()],
            native_subagent_tools: vec!["Spawner".into()],
        }
    }

    #[test]
    fn declared_models_decide_unattended_mode() {
        let capability = capability();
        assert_eq!(capability.model_runs_unattended("ALPHA"), Some(true));
        assert_eq!(capability.model_runs_unattended("beta"), Some(false));
        assert_eq!(
            LaunchPermissionCapability::default().model_runs_unattended("beta"),
            None
        );
    }

    #[test]
    fn allow_args_substitute_each_command_into_the_rule() {
        assert_eq!(
            capability().allow_command_args(&["tool one", "tool two"]),
            vec![
                "--allow",
                "Shell(tool one:*)",
                "--allow",
                "Shell(tool two:*)"
            ]
        );
        let mut no_rule = capability();
        no_rule.command_rule_format = None;
        assert!(no_rule.allow_command_args(&["tool one"]).is_empty());
    }

    #[test]
    fn deny_args_cover_every_native_subagent_tool() {
        assert_eq!(
            capability().deny_native_subagent_args(),
            vec!["--deny", "Spawner"]
        );
        assert!(LaunchPermissionCapability::default()
            .deny_native_subagent_args()
            .is_empty());
    }
}
