//! Cron spawn routing — one builder shared by the scheduler's automatic fire and
//! the manual trigger, so a scheduled agent and a triggered one cannot disagree
//! about which runtime serves the job's provider.
//!
//! Verdict (a) for this path: nothing a cron run needs is PTY-only. Completion
//! keys off the run id carried in the brief (`ServiceContext::cron_complete`
//! tolerates a tab with no PTY), and the run-watcher registration at both fire
//! sites is already conditional on a PTY existing.

use crate::chatbackend::ChatBackendRegistry;
use crate::multiplexer::types::TabAudience;
use crate::service::spawn::{SpawnTabSpec, TabVisibility};
use crate::service::SpawnAgentParams;

#[cfg(test)]
#[path = "spawn_spec/tests.rs"]
mod tests;

pub(crate) struct CronSpawnInputs {
    pub(crate) job_id: String,
    pub(crate) job_name: String,
    pub(crate) agent: String,
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) task_text: String,
    pub(crate) role: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) team_id: Option<String>,
    pub(crate) creator_tab_id: Option<String>,
    pub(crate) report_target_tab_id: Option<String>,
}

/// Build the cron spawn spec through the registry-owned routing seam so a job
/// pinned to a spawnable chat backend reaches the chat runtime instead of the
/// hidden PTY arm rejecting an id the registry advertises.
pub(crate) fn build_cron_spawn_spec(
    chatbackends: &ChatBackendRegistry,
    inputs: CronSpawnInputs,
) -> SpawnTabSpec {
    let mut params = SpawnAgentParams {
        agent: Some(inputs.agent.clone()),
        model: inputs.model,
        reasoning_effort: inputs.reasoning_effort,
        task: Some(inputs.task_text),
        role: inputs.role,
        cwd: inputs.cwd,
        workspace_id: inputs.workspace_id,
        report_target_tab_id: inputs.report_target_tab_id,
        title: Some(format!("cron:{}", inputs.job_name)),
        audience: TabAudience::UserOwnedAutomation,
        spawned_by_tab_id: inputs.creator_tab_id.clone(),
        ..Default::default()
    };
    crate::service::move_chatbackend_task_to_system_prompt(
        chatbackends,
        &inputs.agent,
        &mut params,
    );

    let mut spec = SpawnTabSpec::from_agent_params_with_chatbackends(chatbackends, params);
    spec.team_id = inputs.team_id;
    spec.creator_tab_id = inputs.creator_tab_id;
    // Cron runs are observed through the run history viewer, never as a live tab.
    spec.visibility = TabVisibility::Hidden;
    if let Some(agent) = spec.agent_launch_mut() {
        agent.spawned_by = Some(format!("cron:{}", inputs.job_id));
        agent.launch_purpose = crate::service::unattended_launch::LaunchPurpose::CronRun;
    }
    spec
}
