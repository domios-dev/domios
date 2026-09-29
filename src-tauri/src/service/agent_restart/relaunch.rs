//! The command an agent restart writes into its tab, built from the launch
//! arguments the original spawn recorded so a restart relaunches what ran.

use super::{build_relaunch_command, TabRestartSnapshot};
use crate::service::restored_launch::{
    restore_permission_bypass_authorization, restored_fresh_launch_input,
};
use crate::service::ServiceContext;

pub(super) struct Relaunch {
    pub(super) command: String,
    pub(super) task: Option<String>,
}

/// Resume the tab's session when `session_present`, otherwise launch fresh.
pub(super) fn build_relaunch(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    session_present: bool,
) -> Option<Relaunch> {
    if let Some(relaunch) = recorded_launch_relaunch(svc, snap, session_present) {
        return Some(relaunch);
    }
    let task = (!session_present)
        .then(|| {
            crate::service::orchestrator::provider_launch_task(
                &svc.agent_registry,
                &snap.provider,
                None,
                snap.role.as_deref(),
                snap.role_profile.as_deref(),
            )
        })
        .flatten();
    build_relaunch_command(svc, snap, session_present, task.as_deref()).map(|command| Relaunch {
        command: crate::agent::stub_launch::apply(&snap.provider, command),
        task,
    })
}

/// The restore seam: model, effort, args and identity instructions come from
/// the tab's recorded spawn. `None` when the spawn recorded nothing or the
/// provider resumes through a structured prompt server of its own.
fn recorded_launch_relaunch(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    session_present: bool,
) -> Option<Relaunch> {
    let structured_prompt_server = svc
        .agent_registry
        .with_provider(&snap.provider, |provider| {
            provider.needs_structured_prompt_server()
        })
        .unwrap_or(false);
    let (launch_parameters, resolved_model) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(&snap.tab_id)?;
        (
            tab.restorable_launch.clone(),
            tab.agent_provider_resolved_model.clone(),
        )
    };
    if structured_prompt_server || !launch_parameters.captured {
        return None;
    }
    let permission_bypass_authorization = match restore_permission_bypass_authorization(
        &svc.agent_registry,
        &snap.tab_id,
        &snap.provider,
        &launch_parameters,
    ) {
        Ok(authorization) => authorization,
        Err(error) => {
            log::error!(
                "[agent-restart] refusing policy downgrade for provider={}: {error}",
                snap.provider
            );
            return None;
        }
    };
    let input = restored_fresh_launch_input(
        &svc.agent_registry,
        crate::service::SpawnAgentParams {
            agent: Some(snap.provider.clone()),
            cwd: snap.cwd.clone(),
            resume_session_id: session_present.then(|| snap.session_id.clone()).flatten(),
            skip_permissions: Some(permission_bypass_authorization.is_some()),
            permission_bypass_authorization,
            model: launch_parameters.model.clone().or(resolved_model),
            role: snap.role.clone(),
            role_profile: snap.role_profile.clone(),
            ..Default::default()
        },
        launch_parameters,
    );
    let command = input.command(svc, &snap.tab_id)?;
    // The fresh builder already wrapped its command in the dev stub.
    let command = if input.params.resume_session_id.is_some() {
        crate::agent::stub_launch::apply(&snap.provider, command)
    } else {
        command
    };
    Some(Relaunch {
        command,
        task: input.launch_task,
    })
}

#[cfg(test)]
mod tests;
