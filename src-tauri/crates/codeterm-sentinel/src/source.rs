use codeterm_pty::multiplexer::{state::MultiplexerState, types::Tab};

use serde::{Deserialize, Serialize};

use crate::machine::Route;
use crate::store::Sentinel;

/// What a graph stage observes instead of running a shell probe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TabBinding {
    Tab(String),
    CronRun(String),
}

impl TabBinding {
    pub fn id(&self) -> &str {
        match self {
            Self::Tab(id) | Self::CronRun(id) => id,
        }
    }
}

/// The cron run whose spawned tab a `CronRun` stage observes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    pub run_id: String,
    pub tab_id: String,
    pub bound_at: u64,
}

pub fn bounded_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}

/// A stage evaluates exactly one source: a shell probe, a tab observation or an input
/// definition. Only a cron-run source binds a run, and prompt answers require a tab source.
pub fn validate(job: &Sentinel) -> Result<(), String> {
    super::terminal::validate(job)?;
    if job.pipeline().is_some() {
        return Ok(());
    }
    let sources = (!job.probe().is_empty() as u8)
        + (job.observe().is_some() as u8)
        + (job.input().is_some() as u8);
    if sources != 1 {
        return Err("invalid blueprint source: exactly one of probe, observe or input".into());
    }
    if let Some(binding) = &job.observe().cloned() {
        if !bounded_id(binding.id()) {
            return Err("invalid blueprint observation source".into());
        }
    }
    if let Some(input) = &job.input().cloned() {
        codeterm_blueprint_types::sentinel_input::validate(input)
            .map_err(|error| format!("invalid blueprint input: {error}"))?;
        if !job.rearm_on_change.is_empty() {
            return Err("input stages cannot rearm_on_change".into());
        }
    }
    if let Some(run) = &job.run {
        if !matches!(job.observe(), Some(TabBinding::CronRun(_)))
            || !bounded_id(&run.run_id)
            || !bounded_id(&run.tab_id)
        {
            return Err("invalid blueprint run binding".into());
        }
    }
    let answers = job
        .routes
        .values()
        .flatten()
        .any(|handler| matches!(handler.action, Route::RespondPrompt(_)));
    if answers && job.observe().is_none() {
        return Err("respond_prompt handlers require a tab observation source".into());
    }
    Ok(())
}

pub fn validate_tab_scope(
    mux: &MultiplexerState,
    job: &Sentinel,
    tab_id: &str,
) -> Result<Tab, String> {
    let tab = mux.get_tab(tab_id).ok_or("observation tab not found")?;
    let exact_cron_run = matches!(job.observe(), Some(TabBinding::CronRun(_)))
        && job.run.as_ref().is_some_and(|run| run.tab_id == tab_id)
        && tab.audience == codeterm_pty::multiplexer::types::TabAudience::UserOwnedAutomation;
    let creator_joined_team =
        job.team_id.is_none() && job.creator_tab_id == tab_id && tab.team_id.is_some();
    if (tab.agent_type.is_none() && job.terminal().is_none())
        || (tab.team_id != job.team_id && !exact_cron_run && !creator_joined_team)
        || mux.find_workspace_for_tab(tab_id).as_deref() != Some(job.workspace_id.as_str())
    {
        return Err("observation tab must be in the creator's Team and workspace (shells require terminal options)".into());
    }
    Ok(tab.clone())
}

pub fn validate_pending_cron_run_scope(
    mux: &MultiplexerState,
    job: &Sentinel,
    cron_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    let tab = mux.get_tab(tab_id).ok_or("observation tab not found")?;
    if job.observe().cloned() != Some(TabBinding::CronRun(cron_id.into()))
        || tab.agent_type.is_none()
        || tab.audience != codeterm_pty::multiplexer::types::TabAudience::UserOwnedAutomation
        || mux.find_workspace_for_tab(tab_id).as_deref() != Some(job.workspace_id.as_str())
    {
        return Err("cron run tab is outside the verified observation scope".into());
    }
    Ok(())
}


#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunBindOutcome {
    pub changed: bool,
    pub replaced: Option<RunBinding>,
}
