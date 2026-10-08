use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use regex::Regex;

use super::*;
use crate::agent::provider::{ModelSwitchCommand, ModelSwitchTranscript};

const MAX_CONFIRMATION_BYTES: u64 = 1024 * 1024;

mod prompt;
use prompt::CommandPrompt;

#[derive(Clone)]
struct CommandTarget {
    tab_id: String,
    pty_id: String,
    provider: String,
    session_id: String,
    binding_nonce: String,
    cwd: Option<String>,
    model: String,
    display_name: String,
}

impl CommandTarget {
    fn current(&self, svc: &ServiceContext) -> bool {
        svc.mux
            .read()
            .get_tab(&self.tab_id)
            .is_some_and(|tab| self.matches(tab))
    }

    fn matches(&self, tab: &crate::multiplexer::types::Tab) -> bool {
        tab.pty_id.as_deref() == Some(self.pty_id.as_str())
            && tab.agent_type.as_deref() == Some(self.provider.as_str())
            && tab.agent_session_id.as_deref() == Some(self.session_id.as_str())
            && tab.session_binding_nonce.as_deref() == Some(self.binding_nonce.as_str())
            && tab.session_binding_nonce == tab.reported_session_binding_nonce
    }
}

fn record_confirmed_effort(tab: &mut crate::multiplexer::types::Tab, effort: &str) {
    tab.restorable_launch.requested_reasoning_effort = Some(effort.to_owned());
    tab.restorable_launch.resolved_reasoning_effort = Some(effort.to_owned());
    crate::service::reasoning_effort::persist_launch_identity(tab, Some(effort), Some(effort));
    tab.reasoning_effort_identity.actual = Some(effort.to_owned());
    tab.reasoning_effort_identity.actual_source = Some("session-command".into());
}

fn planned_commands<'a>(
    commands: &'a crate::agent::provider::ModelSwitchCommands,
    model_changed: bool,
    effort: Option<&'a str>,
) -> Vec<(&'a ModelSwitchCommand, Option<&'a str>)> {
    let mut plan = Vec::new();
    if model_changed || effort.is_none() {
        plan.push((&commands.model, None));
    }
    if let (Some(effort), Some(command)) = (effort, commands.effort.as_ref()) {
        plan.push((command, Some(effort)));
    }
    plan
}

fn command_prompt_absent(
    svc: &ServiceContext,
    target: &CommandTarget,
    pattern: Option<&Regex>,
) -> bool {
    let Some(pattern) = pattern else {
        return true;
    };
    svc.screen_state
        .lock()
        .get_screen_text(&target.pty_id)
        .is_some_and(|screen| !pattern.is_match(&screen))
}

fn render_command(template: &str, model: &str, effort: Option<&str>) -> Result<String, String> {
    let placeholder = if effort.is_some() {
        "{effort}"
    } else {
        "{model}"
    };
    let command = template
        .replace("{model}", model)
        .replace("{effort}", effort.unwrap_or_default());
    if !template.contains(placeholder)
        || !command.starts_with('/')
        || command.chars().any(char::is_control)
        || command.contains(['{', '}'])
        || model.chars().any(char::is_whitespace)
        || effort.is_some_and(|value| value.chars().any(char::is_whitespace))
    {
        return Err("invalid live model command template or selection".into());
    }
    Ok(command)
}

fn confirmation_pattern(
    template: &str,
    model: &str,
    display_name: &str,
    effort: Option<&str>,
) -> Result<Regex, String> {
    Regex::new(
        &template
            .replace("{model}", &regex::escape(model))
            .replace("{display_name}", &regex::escape(display_name))
            .replace("{effort}", &regex::escape(effort.unwrap_or_default())),
    )
    .map_err(|error| format!("invalid model confirmation template: {error}"))
}

fn transcript_confirms(bytes: &str, source: &ModelSwitchTranscript, pattern: &Regex) -> bool {
    bytes
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["type"].as_str() == Some(source.record_type.as_str()))
        .filter_map(|record| {
            source
                .content_path
                .iter()
                .try_fold(&record, |value, key| value.get(key))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter_map(|content| {
            content
                .strip_prefix(source.prefix.as_str())
                .and_then(|text| text.strip_suffix(source.suffix.as_str()))
                .map(str::to_owned)
        })
        .any(|stdout| pattern.is_match(stdout.trim()))
}

fn read_confirmation(
    path: &Path,
    boundary: u64,
    source: &ModelSwitchTranscript,
    pattern: &Regex,
) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    if file
        .metadata()
        .is_ok_and(|metadata| metadata.len().saturating_sub(boundary) > MAX_CONFIRMATION_BYTES)
        || file.seek(SeekFrom::Start(boundary)).is_err()
    {
        return false;
    }
    let mut bytes = String::new();
    file.take(MAX_CONFIRMATION_BYTES)
        .read_to_string(&mut bytes)
        .is_ok()
        && transcript_confirms(&bytes, source, pattern)
}

fn execute_command(
    idle: impl FnOnce() -> bool,
    write: impl FnOnce() -> Result<(), String>,
    confirmed: impl FnOnce() -> bool,
) -> Result<(), String> {
    if !idle() {
        return Err(
            "the agent is busy or waiting on a prompt; switch at its next idle prompt".into(),
        );
    }
    write()?;
    if !confirmed() {
        return Err("the command was sent but its own session did not confirm the change before the deadline; restart and resume is available as an explicit fallback".into());
    }
    Ok(())
}

impl ServiceContext {
    pub(super) fn switch_model_by_command(
        &self,
        snap: &TabRestartSnapshot,
        session_id: &str,
        model: &str,
        effort: Option<&str>,
        previous_model: Option<String>,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        let _claim = {
            let mut tracker = self.restart_tracker.lock();
            if !tracker.in_progress.insert(snap.tab_id.clone()) {
                return Err(ModelSwitchError::NotSwitchable(
                    "a model switch or restart is already in progress".into(),
                ));
            }
            super::super::RestartClaim {
                tracker: self.restart_tracker.clone(),
                tab_id: snap.tab_id.clone(),
            }
        };
        let commands = self
            .agent_registry
            .with_provider(&snap.provider, |provider| provider.model_switch_commands())
            .flatten()
            .ok_or_else(|| {
                ModelSwitchError::LiveCommandFailed(
                    "the provider has no live command declaration".into(),
                )
            })?;
        if effort.is_some() && commands.effort.is_none() {
            return Err(ModelSwitchError::LiveCommandFailed(
                "the provider has no live effort command".into(),
            ));
        }
        let display_name = self
            .agent_registry
            .with_provider(&snap.provider, |provider| {
                provider
                    .available_models()
                    .into_iter()
                    .find(|entry| entry.id == model)
                    .map(|entry| entry.display_name)
                    .filter(|name| !name.trim().is_empty())
            })
            .flatten()
            .unwrap_or_else(|| model.to_owned());
        let binding_nonce = self
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone())
            .ok_or_else(|| {
                ModelSwitchError::NotSwitchable("the session binding is unavailable".into())
            })?;
        if let (Some(effort), Some(command)) = (effort, commands.effort.as_ref()) {
            render_command(&command.command, model, Some(effort))
                .map_err(ModelSwitchError::LiveCommandFailed)?;
            confirmation_pattern(&command.confirmation, model, &display_name, Some(effort))
                .map_err(ModelSwitchError::LiveCommandFailed)?;
        }
        let target = CommandTarget {
            tab_id: snap.tab_id.clone(),
            pty_id: snap.pty_id.clone(),
            provider: snap.provider.clone(),
            session_id: session_id.to_owned(),
            binding_nonce,
            cwd: snap.cwd.clone(),
            model: model.to_owned(),
            display_name,
        };
        let model_changed = !previous_model.as_deref().is_some_and(|previous| {
            crate::service::session_model::session_model_matches(
                &self.agent_registry,
                &snap.provider,
                model,
                previous,
            )
        });
        for (command, command_effort) in planned_commands(&commands, model_changed, effort) {
            self.run_model_command(&target, command_effort, command, &commands.transcript)?;
            if let Some(effort) = command_effort {
                {
                    let mut mux = self.mux.write();
                    let Some(tab) = mux
                        .get_tab_mut(&snap.tab_id)
                        .filter(|tab| target.matches(tab))
                    else {
                        return Err(ModelSwitchError::LiveCommandFailed(
                            "the session changed before the effort confirmation could be recorded"
                                .into(),
                        ));
                    };
                    record_confirmed_effort(tab, effort);
                }
                if let Err(error) = crate::service::session_persist::persist_session_after_mutation(
                    self,
                    crate::service::session_persist::SessionPersistUrgency::Prompt,
                ) {
                    log::warn!("[agent-model] failed to persist the confirmed effort: {error}");
                }
                self.emit_state_changed();
            } else {
                let prior_effort = self
                    .mux
                    .read()
                    .get_tab(&snap.tab_id)
                    .and_then(|tab| tab.reasoning_effort_identity.resolved.clone());
                self.record_live_model_request(
                    &snap.tab_id,
                    &snap.provider,
                    session_id,
                    model,
                    prior_effort.as_deref(),
                )?;
                if !self.record_live_model_confirmation(
                    &snap.tab_id,
                    &snap.provider,
                    session_id,
                    model,
                    model,
                ) {
                    return Err(ModelSwitchError::LiveCommandFailed(
                        "the session changed before the model confirmation could be recorded"
                            .into(),
                    ));
                }
            }
        }
        Ok(ModelSwitchOutcome {
            tab_id: snap.tab_id.clone(),
            provider: snap.provider.clone(),
            session_id: session_id.to_owned(),
            restart: None,
            mechanism: ModelSwitchMode::LiveCommand,
            fallback_reason: None,
            previous_model,
            requested_model: model.to_owned(),
            launched_model: model.to_owned(),
            confirmation_detail: None,
            confirmation: ModelSwitchConfirmation::Observed {
                model: model.to_owned(),
                evidence: ModelSwitchEvidence::LiveSessionRecord,
            },
        })
    }

    fn run_model_command(
        &self,
        target: &CommandTarget,
        effort: Option<&str>,
        template: &ModelSwitchCommand,
        source: &ModelSwitchTranscript,
    ) -> Result<(), ModelSwitchError> {
        let model = target.model.as_str();
        let display_name = target.display_name.as_str();
        let command = render_command(&template.command, model, effort)
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let pattern = confirmation_pattern(&template.confirmation, model, display_name, effort)
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let mut prompt = template
            .prompt
            .as_ref()
            .map(|declaration| CommandPrompt::new(declaration, target, effort))
            .transpose()
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let prompt_pattern = template
            .prompt
            .as_ref()
            .map(|declaration| {
                confirmation_pattern(&declaration.screen_regex, model, display_name, effort)
            })
            .transpose()
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let path = self
            .agent_registry
            .with_provider(&target.provider, |provider| {
                target
                    .cwd
                    .as_deref()
                    .and_then(|cwd| provider.session_file_path(cwd, &target.session_id))
            })
            .flatten()
            .ok_or_else(|| {
                ModelSwitchError::LiveCommandFailed(
                    "the bound session has no transcript path".into(),
                )
            })?;
        let boundary = std::fs::metadata(&path)
            .map_err(|error| ModelSwitchError::LiveCommandFailed(error.to_string()))?
            .len();
        let pty_id = target.pty_id.as_str();
        let signal = self.screen_state.lock().arm_screen_change_signal(pty_id);
        let deadline = Instant::now() + Duration::from_secs(MODEL_OBSERVE_TIMEOUT_SECS);
        let binding_current = || target.current(self);
        let mut prompt_error = None;
        let result = execute_command(
            || {
                binding_current()
                    && self.tab_is_idle(&target.tab_id)
                    && command_prompt_absent(self, target, prompt_pattern.as_ref())
            },
            || {
                let permit = self.pty.lock().issue_guarded_input_permit(pty_id)?;
                let svc = self.clone();
                let expected = target.clone();
                let prompt_pattern = prompt_pattern.clone();
                let guard = std::sync::Arc::new(move || {
                    svc.tab_is_idle(&expected.tab_id)
                        && expected.current(&svc)
                        && command_prompt_absent(&svc, &expected, prompt_pattern.as_ref())
                });
                let claim = crate::service::pty_io::submit_transaction::claim_submit_cycle(pty_id);
                crate::service::pty_io::submit_transaction::submit_text_with_key_claimed_and_follow_up_revalidated(
                    self, pty_id, &command, None, true, b"\r",
                    crate::agent::provider::VerifySubmitStrategy::None,
                    crate::service::inject_verify::VerifyOpts {
                        require_prompt_first: false, verify_echo: false, verify_submit: false, ..Default::default()
                    }, Some(permit), false, claim, Some(guard), false, None,
                ).map(|_| ()).map_err(|error| error.to_string())
            },
            || {
                if let Some(prompt) = prompt.as_mut() {
                    prompt.injected();
                }
                loop {
                    let generation = signal.generation();
                    if !binding_current() {
                        break false;
                    }
                    if read_confirmation(&path, boundary, source, &pattern) {
                        break true;
                    }
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        break false;
                    };
                    if let Some(prompt) = prompt.as_mut() {
                        if let Err(error) = self.respond_to_command_prompt(target, prompt) {
                            prompt_error = Some(error);
                            break false;
                        }
                    }
                    if !signal.wait_after(generation, remaining) {
                        break binding_current()
                            && read_confirmation(&path, boundary, source, &pattern);
                    }
                }
            },
        );
        self.screen_state
            .lock()
            .unsubscribe_screen_changes(pty_id, &signal);
        result.map_err(|error| ModelSwitchError::LiveCommandFailed(prompt_error.unwrap_or(error)))
    }
}

#[cfg(test)]
mod tests;
