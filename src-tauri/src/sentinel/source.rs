use serde_json::{json, Value};
use tauri::Manager;

use crate::multiplexer::{
    state::MultiplexerState,
    types::{PendingPrompt, Tab},
};
use crate::service::{supporter_tab_state, ServiceContext};

use super::probe::{process::ProbeFailure, Output};
use serde::{Deserialize, Serialize};

use super::machine::Route;
use super::routes;
use super::store::Sentinel;

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

pub(crate) fn resolve_binding(
    svc: &ServiceContext,
    job: &Sentinel,
    binding: &TabBinding,
) -> Result<TabBinding, String> {
    match binding {
        TabBinding::Tab(_) => Ok(binding.clone()),
        TabBinding::CronRun(target) => {
            let cron = svc
                .app_handle
                .try_state::<crate::CronState>()
                .ok_or("cron not initialized")?;
            let cron_id = routes::resolve_cron_target(&cron.lock(), job, target)?;
            Ok(TabBinding::CronRun(cron_id))
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

pub(super) fn bounded_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}

/// A stage evaluates exactly one source: a shell probe, a tab observation or an input
/// definition. Only a cron-run source binds a run, and prompt answers require a tab source.
pub(crate) fn validate(job: &Sentinel) -> Result<(), String> {
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
        codeterm_types::sentinel_input::validate(input)
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

pub(crate) fn validate_tab_scope(
    mux: &MultiplexerState,
    job: &Sentinel,
    tab_id: &str,
) -> Result<Tab, String> {
    let tab = mux.get_tab(tab_id).ok_or("observation tab not found")?;
    let exact_cron_run = matches!(job.observe(), Some(TabBinding::CronRun(_)))
        && job.run.as_ref().is_some_and(|run| run.tab_id == tab_id)
        && tab.audience == crate::multiplexer::types::TabAudience::UserOwnedAutomation;
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

pub(super) fn validate_pending_cron_run_scope(
    mux: &MultiplexerState,
    job: &Sentinel,
    cron_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    let tab = mux.get_tab(tab_id).ok_or("observation tab not found")?;
    if job.observe().cloned() != Some(TabBinding::CronRun(cron_id.into()))
        || tab.agent_type.is_none()
        || tab.audience != crate::multiplexer::types::TabAudience::UserOwnedAutomation
        || mux.find_workspace_for_tab(tab_id).as_deref() != Some(job.workspace_id.as_str())
    {
        return Err("cron run tab is outside the verified observation scope".into());
    }
    Ok(())
}

const MAX_SCREEN_BYTES: usize = 4 * 1024;
const MAX_OBSERVATION_BYTES: usize = 16 * 1024;
const MAX_PROMPT_OPTIONS: usize = 32;
const MAX_PROMPT_TEXT_BYTES: usize = 2048;
const MAX_OPTION_TEXT_BYTES: usize = 512;

pub(super) fn observe(
    svc: &ServiceContext,
    job: &Sentinel,
    binding: &TabBinding,
) -> Result<Output, ProbeFailure> {
    if job.observe() != Some(binding) {
        return Err(ProbeFailure::new("blueprint_observation_binding_changed"));
    }
    let (tab_id, run_id) = match binding {
        TabBinding::Tab(tab_id) => (tab_id.as_str(), None),
        TabBinding::CronRun(_) => {
            let Some(run) = job.run.as_ref() else {
                return Err(ProbeFailure::new(
                    "blueprint_observation_source_unavailable",
                ));
            };
            (run.tab_id.as_str(), Some(run.run_id.as_str()))
        }
    };
    let tab = {
        let mux = svc.mux.read();
        validate_tab_scope(&mux, job, tab_id).map_err(|error| {
            if error == "observation tab not found" {
                ProbeFailure::new("blueprint_observation_tab_removed")
            } else {
                ProbeFailure::new("blueprint_observation_scope_changed")
            }
        })?
    };
    let status = crate::multiplexer::state::tab_status_label(&tab, svc.resolved_tab_status(&tab));
    let screen = if tab.pending_prompt.is_none()
        && status == codeterm_types::pty::TabStatus::Waiting.label()
    {
        tab.pty_id
            .as_deref()
            .and_then(|pty_id| svc.screen_state.lock().get_screen_text(pty_id))
    } else {
        None
    };
    let value = build_observation(
        tab.pending_prompt.as_ref(),
        tab.pending_prompt_kind,
        status,
        tab.agent_booting,
        run_id,
        screen.as_deref(),
    )
    .map_err(|_| ProbeFailure::new("blueprint_observation_invalid"))?;
    let text = serde_json::to_string(&value)
        .map_err(|_| ProbeFailure::new("blueprint_observation_encoding_failed"))?;
    if text.len() > MAX_OBSERVATION_BYTES {
        return Err(ProbeFailure::new("blueprint_observation_too_large"));
    }
    Ok(Output {
        bytes: text.as_bytes().to_vec(),
        text,
        success: true,
        exit_code: Some(0),
        ..Output::default()
    })
}

fn build_observation(
    prompt: Option<&PendingPrompt>,
    prompt_kind: Option<codeterm_types::pty::PromptKind>,
    status: &str,
    agent_booting: bool,
    run_id: Option<&str>,
    screen: Option<&str>,
) -> Result<Value, String> {
    if status.len() > 64 || run_id.is_some_and(|id| !bounded_id(id)) {
        return Err("observation identity is out of bounds".into());
    }
    let prompt = prompt
        .map(|prompt| prompt_value(prompt, prompt_kind))
        .transpose()?;
    let screen_tail = screen.map(redacted_screen_tail);
    let raw = json!({
        "prompt": prompt,
        "tab": {"status": status, "agent_booting": agent_booting},
        "run_id": run_id,
        "screen_tail": screen_tail
    });
    Ok(supporter_tab_state::redact_decision_diagnostic(&raw))
}

fn prompt_value(
    prompt: &PendingPrompt,
    prompt_kind: Option<codeterm_types::pty::PromptKind>,
) -> Result<Value, String> {
    if prompt.question.len() > MAX_PROMPT_TEXT_BYTES
        || prompt.options.len() > MAX_PROMPT_OPTIONS
        || prompt.options.iter().any(|option| {
            option.label.len() > MAX_OPTION_TEXT_BYTES || option.value.len() > MAX_OPTION_TEXT_BYTES
        })
    {
        return Err("prompt exceeds the observation bound".into());
    }
    Ok(json!({
        "kind": prompt_kind,
        "question": &prompt.question,
        "options": prompt.options.iter().map(|option| {
        json!({"label": &option.label, "value": &option.value})
        }).collect::<Vec<_>>(),
        "multi_select": prompt.multi_select
    }))
}

fn redacted_screen_tail(input: &str) -> String {
    let screen = supporter_tab_state::redact_screen(input);
    let mut start = screen.len().saturating_sub(MAX_SCREEN_BYTES);
    while !screen.is_char_boundary(start) {
        start += 1;
    }
    screen[start..].to_string()
}

#[cfg(test)]
#[path = "observe_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "launch_recipe_tests.rs"]
mod launch_recipe_tests;
