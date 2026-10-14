pub use codeterm_sentinel::source::*;

use codeterm_pty::multiplexer::types::PendingPrompt;
use serde_json::{json, Value};
use tauri::Manager;

use crate::service::{supporter_tab_state, ServiceContext};

use super::probe::{process::ProbeFailure, Output};
use super::routes;
use super::store::Sentinel;

pub fn resolve_binding(
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

const MAX_SCREEN_BYTES: usize = 4 * 1024;
const MAX_OBSERVATION_BYTES: usize = 16 * 1024;
const MAX_PROMPT_OPTIONS: usize = 32;
const MAX_PROMPT_TEXT_BYTES: usize = 2048;
const MAX_OPTION_TEXT_BYTES: usize = 512;

pub fn observe(
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
    let status = codeterm_pty::multiplexer::state::tab_status_label(&tab, svc.resolved_tab_status(&tab));
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
