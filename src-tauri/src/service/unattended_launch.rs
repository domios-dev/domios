//! Whether a launch can run its own CodeTerm commands without an owner prompt,
//! and the provider launch args that make it so. Decided from the provider's
//! declared launch-permission capability and model metadata only.

use crate::agent::registry::AgentRegistry;
use crate::multiplexer::types::AgentRole;
use codeterm_types::agent::role_kind::builtins::ORCHESTRATOR;

/// CodeTerm commands a role or task launch runs to acknowledge and report.
pub(crate) const LIFECYCLE_COMMANDS: &[&str] = &[
    "codeterm agent startup-ack",
    "codeterm agent ack",
    "codeterm agent report",
    "codeterm pad path",
];

/// Commands a scheduled run owes on top of the lifecycle set.
pub(crate) const CRON_RUN_COMMANDS: &[&str] = &["codeterm cron complete"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LaunchPurpose {
    #[default]
    Agent,
    CronRun,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UnattendedLaunch<'a> {
    pub(crate) provider_id: &'a str,
    pub(crate) model: Option<&'a str>,
    pub(crate) role: Option<&'a str>,
    pub(crate) role_profile: Option<&'a str>,
    pub(crate) has_task: bool,
    pub(crate) purpose: LaunchPurpose,
    pub(crate) full_access: bool,
}

impl UnattendedLaunch<'_> {
    fn role_kind(&self) -> Option<(AgentRole, Option<String>)> {
        crate::service::ServiceContext::parse_role_kind(self.role?, self.role_profile).ok()
    }

    fn managing(&self) -> bool {
        self.role_kind()
            .is_some_and(|(role, _)| role == AgentRole::Manager)
    }

    fn orchestrator(&self) -> bool {
        self.role_kind()
            .is_some_and(|(_, profile)| profile.as_deref() == Some(ORCHESTRATOR))
    }

    fn role_label(&self) -> &str {
        self.role_profile.or(self.role).unwrap_or("agent")
    }

    fn owed_commands(&self) -> Vec<&'static str> {
        let cron = self.purpose == LaunchPurpose::CronRun;
        let mut commands = Vec::new();
        if self.role.is_some() || self.has_task || cron {
            commands.extend_from_slice(LIFECYCLE_COMMANDS);
        }
        if cron {
            commands.extend_from_slice(CRON_RUN_COMMANDS);
        }
        commands
    }
}

impl crate::service::spawn::AgentSpawnSpec {
    pub(crate) fn unattended_launch<'a>(
        &'a self,
        provider_id: &'a str,
        model: Option<&'a str>,
        full_access: bool,
    ) -> UnattendedLaunch<'a> {
        UnattendedLaunch {
            provider_id,
            model,
            role: self.role.as_deref(),
            role_profile: self.role_profile.as_deref(),
            has_task: self.task.is_some(),
            purpose: self.launch_purpose,
            full_access,
        }
    }
}

struct ModelMode {
    capability: codeterm_plugin::provider::LaunchPermissionCapability,
    model: String,
    display_name: String,
}

fn model_mode(registry: &AgentRegistry, launch: &UnattendedLaunch<'_>) -> Option<ModelMode> {
    registry
        .with_provider(launch.provider_id, |provider| {
            let capability = provider.launch_permission_capability()?;
            let model = launch
                .model
                .map(str::to_string)
                .filter(|model| !model.trim().is_empty())
                .or_else(|| provider.default_model())
                .unwrap_or_default();
            Some(ModelMode {
                capability,
                model,
                display_name: provider.display_name().to_string(),
            })
        })
        .flatten()
}

fn lacks_unattended_mode(mode: &ModelMode, launch: &UnattendedLaunch<'_>) -> bool {
    !launch.full_access && mode.capability.model_runs_unattended(&mode.model) == Some(false)
}

/// Refuse, before any tab exists, a launch whose role cannot run its CodeTerm
/// commands unattended on the requested model.
pub(crate) fn preflight(
    registry: &AgentRegistry,
    launch: &UnattendedLaunch<'_>,
) -> Result<(), String> {
    let Some(mode) = model_mode(registry, launch) else {
        return Ok(());
    };
    if !lacks_unattended_mode(&mode, launch) {
        return Ok(());
    }
    let owed = launch.owed_commands();
    let grantable = !mode.capability.allow_command_args(&owed).is_empty();
    if !launch.managing() && (owed.is_empty() || grantable) {
        return Ok(());
    }
    let supported = mode.capability.unattended_mode_models.join(", ");
    let need = if launch.managing() {
        "coordinates through CodeTerm commands on every turn".to_string()
    } else {
        format!("must run {} unattended", owed.join(", "))
    };
    Err(format!(
        "{} model '{}' has no unattended permission mode and agent access is not Full: a {} launch {need}. \
         Choose a model with that mode ({supported}) or turn on Settings > Agents > Full access for agents.",
        mode.display_name,
        mode.model,
        launch.role_label(),
    ))
}

/// Raw argv elements this launch needs: an Orchestrator loses native subagent
/// tools, and a launch without an unattended mode gets its owed CodeTerm commands.
pub(crate) fn grant_args(registry: &AgentRegistry, launch: &UnattendedLaunch<'_>) -> Vec<String> {
    let Some(mode) = model_mode(registry, launch) else {
        return Vec::new();
    };
    let mut args = if launch.orchestrator() {
        mode.capability.deny_native_subagent_args()
    } else {
        Vec::new()
    };
    if lacks_unattended_mode(&mode, launch) {
        args.extend(mode.capability.allow_command_args(&launch.owed_commands()));
    }
    args
}

#[cfg(test)]
#[path = "unattended_launch/tests.rs"]
mod tests;
