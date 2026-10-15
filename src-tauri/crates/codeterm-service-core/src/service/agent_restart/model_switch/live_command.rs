use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use regex::Regex;

use super::*;
use crate::agent::provider::{ModelSwitchCommand, ModelSwitchTranscript};

const MAX_CONFIRMATION_BYTES: u64 = 1024 * 1024;

mod picker;
mod prompt;
mod readiness;
mod sequence;
mod session_identity;
mod transport;
use picker::CommandPicker;
use prompt::CommandPrompt;

fn command_signal(
    screens: &mut crate::pty::screen_state::ScreenState,
    pty_id: &str,
) -> std::sync::Arc<crate::pty::screen_state::ScreenChangeSignal> {
    screens.subscribe_screen_changes(pty_id)
}

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
    readiness: Option<std::sync::Arc<readiness::CommandReadiness>>,
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

fn record_target_effort(
    target: &CommandTarget,
    tab: &mut crate::multiplexer::types::Tab,
    effort: &str,
) -> Result<(), ModelSwitchError> {
    if !target.matches(tab) {
        return Err(ModelSwitchError::LiveCommandFailed(
            "the session changed before the effort confirmation could be recorded".into(),
        ));
    }
    record_confirmed_effort(tab, effort);
    Ok(())
}

fn planned_commands<'a>(
    commands: &'a crate::agent::provider::ModelSwitchCommands,
    model_changed: bool,
    effort: Option<&'a str>,
    confirmed_effort: Option<&str>,
) -> Vec<(&'a ModelSwitchCommand, Option<&'a str>)> {
    let mut plan = Vec::new();
    if model_changed {
        plan.push((&commands.model, None));
    }
    if let (Some(effort), Some(command)) = (
        effort.filter(|effort| Some(*effort) != confirmed_effort),
        commands.effort.as_ref(),
    ) {
        plan.push((command, Some(effort)));
    }
    plan
}

fn model_command_required(
    source: crate::agent::provider::ModelSwitchConfirmationSource,
    observed_matches: bool,
    recorded_matches: bool,
) -> bool {
    !observed_matches
        || (source == crate::agent::provider::ModelSwitchConfirmationSource::SessionIdentity
            && !recorded_matches)
}

fn planning_model(
    source: crate::agent::provider::ModelSwitchConfirmationSource,
    recorded: Option<String>,
    observe: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if source == crate::agent::provider::ModelSwitchConfirmationSource::SessionIdentity {
        observe()
    } else {
        recorded
    }
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

fn command_dialogs_absent(
    svc: &ServiceContext,
    target: &CommandTarget,
    prompt: Option<&Regex>,
    picker: Option<&CommandPicker>,
) -> bool {
    command_prompt_absent(svc, target, prompt)
        && picker.is_none_or(|picker| {
            svc.screen_state
                .lock()
                .get_screen_text(&target.pty_id)
                .is_some_and(|screen| !picker.present(&screen))
        })
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
    if (!template.contains(placeholder) && template.contains(' '))
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
    source
        .records(bytes)
        .filter_map(Result::ok)
        .filter_map(|record| source.record_stdout(&record))
        .any(|stdout| pattern.is_match(crate::pty::ansi::strip_ansi(&stdout).trim()))
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
    fn command_tab_idle(&self, target: &CommandTarget) -> bool {
        self.command_input_ready(target).0
    }

    fn command_input_ready(&self, target: &CommandTarget) -> (bool, String) {
        let (current, status) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(&target.tab_id) else {
                return (false, "missing".into());
            };
            (
                target.matches(tab) && tab.pending_prompt.is_none(),
                self.resolved_tab_status(tab),
            )
        };
        let observed = format!("{status:?}");
        if !current {
            return (false, observed);
        }
        let frame = target.readiness.as_ref().and_then(|_| {
            let screens = self.screen_state.lock();
            screens
                .get_screen_rows(&target.pty_id)
                .map(|rows| rows.join("\n"))
                .zip(screens.composer_input_state(&target.pty_id))
        });
        let no_dialog = frame.as_ref().is_none_or(|(screen, _)| {
            self.agent_registry
                .with_provider(&target.provider, |provider| {
                    provider.parse_prompt(screen).is_none()
                })
                .unwrap_or(false)
        });
        let ready = no_dialog
            && readiness::input_ready(
                target.readiness.as_deref(),
                frame
                    .as_ref()
                    .map(|(screen, input)| (screen.as_str(), input)),
                status == Some(codeterm_types::pty::TabStatus::Idle),
            );
        (ready, observed)
    }

    pub(super) fn switch_model_by_command(
        &self,
        snap: &TabRestartSnapshot,
        session_id: &str,
        model: &str,
        effort: Option<&str>,
        after_turn: bool,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        let _claim = {
            let mut tracker = self
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock();
            if !tracker.in_progress.insert(snap.tab_id.clone()) {
                return Err(ModelSwitchError::NotSwitchable(
                    "a model switch or restart is already in progress".into(),
                ));
            }
            super::super::RestartClaim {
                tracker: self
                    .ext::<crate::service::agent_restart::RestartTracker>()
                    .clone(),
                tab_id: snap.tab_id.clone(),
            }
        };
        let deadline = Instant::now() + Duration::from_secs(MODEL_OBSERVE_TIMEOUT_SECS);
        let completion_service = self.clone();
        let completion_tab = snap.tab_id.clone();
        let _input = crate::service::pty_io::submit_transaction::reserve_command_input(
            &snap.pty_id,
            deadline,
        )
        .map_err(|reason| ModelSwitchError::InputNotReady(reason.into()))?
        .on_completion(move || {
            completion_service.wake_agent_deliveries_on_state_change(&completion_tab);
            completion_service.wake_user_owner(
                &completion_tab,
                crate::service::parked_user_delivery::UserWake::Readiness,
            );
        });
        log::debug!(
            "[agent-model] reserved command input tab={} pty={}",
            snap.tab_id,
            snap.pty_id
        );
        let commands = self
            .agent_registry
            .with_provider(&snap.provider, |provider| provider.model_switch_commands())
            .flatten()
            .ok_or_else(|| {
                ModelSwitchError::LiveCommandFailed(
                    "the provider has no live command declaration".into(),
                )
            })?;
        if commands.confirmation_source
            == crate::agent::provider::ModelSwitchConfirmationSource::Transcript
            && (commands.transcript.record_type.is_empty()
                || commands.transcript.content_path.is_empty()
                || commands.model.confirmation.trim().is_empty()
                || commands
                    .effort
                    .as_ref()
                    .is_some_and(|command| command.confirmation.trim().is_empty()))
        {
            return Err(ModelSwitchError::LiveCommandFailed(
                "the transcript confirmation declaration is incomplete".into(),
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
        let mut target = CommandTarget {
            tab_id: snap.tab_id.clone(),
            pty_id: snap.pty_id.clone(),
            provider: snap.provider.clone(),
            session_id: session_id.to_owned(),
            binding_nonce,
            cwd: snap.cwd.clone(),
            model: model.to_owned(),
            display_name,
            readiness: None,
        };
        if !target.current(self) {
            return Err(ModelSwitchError::LiveCommandFailed(
                "the session changed before reading its current model".into(),
            ));
        }
        let _frame_timing =
            crate::service::pty_frame_timing::Scope::begin(&target.pty_id, &target.tab_id);
        self.ext::<crate::service::agent_restart::RestartTracker>()
            .lock()
            .command_ptys
            .insert(
                target.tab_id.clone(),
                (target.pty_id.clone(), target.binding_nonce.clone()),
            );
        let recorded_model = self.mux.read().get_tab(&target.tab_id).and_then(|tab| {
            tab.agent_session_model_identity
                .as_ref()
                .filter(|identity| {
                    identity.session_id == target.session_id
                        && identity.session_binding_nonce == target.binding_nonce
                })
                .and(tab.agent_session_model.clone())
        });
        let previous_model =
            planning_model(commands.confirmation_source, recorded_model.clone(), || {
                target.cwd.as_deref().and_then(|cwd| {
                    self.agent_registry
                        .with_provider(&target.provider, |provider| {
                            provider.detect_session_model(cwd, &target.session_id)
                        })
                        .flatten()
                })
            });
        if !target.current(self) {
            return Err(ModelSwitchError::LiveCommandFailed(
                "the session changed while reading its current model".into(),
            ));
        }
        let matches_requested = |previous: &str| {
            crate::service::session_model::session_model_matches(
                &self.agent_registry,
                &snap.provider,
                model,
                previous,
            )
        };
        let model_changed = model_command_required(
            commands.confirmation_source,
            previous_model.as_deref().is_some_and(matches_requested),
            recorded_model.as_deref().is_some_and(matches_requested),
        );
        let current_effort = planning_model(
            commands.confirmation_source,
            self.mux
                .read()
                .get_tab(&target.tab_id)
                .and_then(|tab| tab.reasoning_effort_identity.actual.clone()),
            || {
                target.cwd.as_deref().and_then(|cwd| {
                    self.agent_registry
                        .with_provider(&target.provider, |provider| {
                            provider.detect_session_reasoning_effort(cwd, &target.session_id)
                        })
                        .flatten()
                })
            },
        );
        let changed_effort = effort.filter(|effort| current_effort.as_deref() != Some(*effort));
        if !target.current(self) {
            return Err(ModelSwitchError::LiveCommandFailed(
                "the session changed while reading its current effort".into(),
            ));
        }
        if changed_effort.is_some() && commands.effort.is_none() {
            return Err(ModelSwitchError::LiveCommandFailed(
                "the provider has no live effort command".into(),
            ));
        }
        let provider = self
            .agent_registry
            .provider_arc(&target.provider)
            .ok_or_else(|| {
                ModelSwitchError::LiveCommandFailed("the command provider is unavailable".into())
            })?;
        let readiness =
            readiness::CommandReadiness::from_declaration(&commands, &target, provider)?;
        let sequence =
            sequence::CommandSequence::new(self, &target, deadline, &commands.transcript);
        if after_turn {
            target.readiness = readiness.clone();
            sequence.wait_for_idle(self, &target, true)?;
        }
        for (index, (command, command_effort)) in
            planned_commands(&commands, model_changed, effort, current_effort.as_deref())
                .into_iter()
                .enumerate()
        {
            if index > 0 {
                target.readiness = readiness.clone();
                sequence.wait_for_idle(self, &target, false)?;
            }
            self.run_declared_model_command(&target, command_effort, command, &commands, deadline)
                .map_err(|error| match error {
                    ModelSwitchError::InputNotReady(reason) if index > 0 => {
                        ModelSwitchError::LiveCommandFailed(reason)
                    }
                    error => error,
                })?;
            if !target.current(self) {
                return Err(ModelSwitchError::LiveCommandFailed(
                    "the command session identity changed".into(),
                ));
            }
            if let Some(effort) = command_effort {
                {
                    let mut mux = self.mux.write();
                    let Some(tab) = mux.get_tab_mut(&snap.tab_id) else {
                        return Err(ModelSwitchError::LiveCommandFailed(
                            "the session changed before the effort confirmation could be recorded"
                                .into(),
                        ));
                    };
                    record_target_effort(&target, tab, effort)?;
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

    fn run_declared_model_command(
        &self,
        target: &CommandTarget,
        effort: Option<&str>,
        template: &ModelSwitchCommand,
        commands: &crate::agent::provider::ModelSwitchCommands,
        deadline: Instant,
    ) -> Result<(), ModelSwitchError> {
        if commands.confirmation_source
            == crate::agent::provider::ModelSwitchConfirmationSource::SessionIdentity
        {
            self.run_session_identity_command(target, effort, template, deadline)
        } else {
            self.run_model_command(target, effort, template, &commands.transcript, deadline)
        }
    }

    fn run_model_command(
        &self,
        target: &CommandTarget,
        effort: Option<&str>,
        template: &ModelSwitchCommand,
        source: &ModelSwitchTranscript,
        deadline: Instant,
    ) -> Result<(), ModelSwitchError> {
        let model = target.model.as_str();
        let display_name = target.display_name.as_str();
        let command = render_command(&template.command, model, effort)
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let pattern = confirmation_pattern(&template.confirmation, model, display_name, effort)
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let structural_confirmation = template.confirmation_prompt_kind.is_some();
        let mut prompt = template
            .prompt
            .as_ref()
            .filter(|_| !structural_confirmation)
            .map(|declaration| CommandPrompt::new(declaration, target, effort))
            .transpose()
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let mut picker = template
            .picker
            .as_ref()
            .map(|declaration| CommandPicker::new(declaration, target, effort))
            .transpose()
            .map_err(ModelSwitchError::LiveCommandFailed)?;
        let prompt_pattern = template
            .prompt
            .as_ref()
            .filter(|_| !structural_confirmation)
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
        let boundary = std::cell::Cell::new(
            std::fs::metadata(&path)
                .map_err(|error| ModelSwitchError::LiveCommandFailed(error.to_string()))?
                .len(),
        );
        let pty_id = target.pty_id.as_str();
        let signal = command_signal(&mut self.screen_state.lock(), pty_id);
        let transcript_signal = signal.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|event| {
                    matches!(
                        event.kind,
                        notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                    )
                }) {
                    transcript_signal.mark_changed();
                }
            })
            .map_err(|error| {
                self.screen_state
                    .lock()
                    .unsubscribe_screen_changes(pty_id, &signal);
                ModelSwitchError::LiveCommandFailed(error.to_string())
            })?;
        if let Err(error) =
            notify::Watcher::watch(&mut watcher, &path, notify::RecursiveMode::NonRecursive)
        {
            self.screen_state
                .lock()
                .unsubscribe_screen_changes(pty_id, &signal);
            return Err(ModelSwitchError::LiveCommandFailed(error.to_string()));
        }
        let started = Instant::now();
        let binding_current = || target.current(self);
        let picker_absent = !picker.as_ref().is_some_and(|picker| {
            self.screen_state
                .lock()
                .get_screen_text(pty_id)
                .is_some_and(|screen| picker.present(&screen))
        });
        let mut prompt_error = None;
        let injected = std::cell::Cell::new(false);
        let not_ready = std::cell::Cell::new(false);
        let result = execute_command(
            || {
                let ready = binding_current()
                    && self.command_tab_idle(target)
                    && command_prompt_absent(self, target, prompt_pattern.as_ref())
                    && picker_absent;
                not_ready.set(!ready);
                ready
            },
            || {
                self.replace_switch_confirmation(pty_id, None);
                let switch_confirmation = self.prepare_switch_confirmation(
                    pty_id,
                    &command,
                    Some(effort.unwrap_or(model)),
                );
                let _claim =
                    crate::service::pty_io::submit_transaction::claim_command_submit_cycle(pty_id);
                let (session, permit, input) = {
                    let mux = self.mux.read();
                    if !mux
                        .get_tab(&target.tab_id)
                        .is_some_and(|tab| target.matches(tab))
                    {
                        return Err("the session changed before command injection".into());
                    }
                    let screen = self.screen_state.lock();
                    if template.picker.as_ref().is_some_and(|declaration| {
                        confirmation_pattern(&declaration.screen_regex, model, display_name, effort)
                            .is_ok_and(|pattern| {
                                screen
                                    .get_screen_text(pty_id)
                                    .is_none_or(|text| pattern.is_match(&text))
                            })
                    }) {
                        return Err("a picker was already present before command injection".into());
                    }
                    if prompt_pattern.as_ref().is_some_and(|pattern| {
                        screen
                            .get_screen_text(pty_id)
                            .is_none_or(|text| pattern.is_match(&text))
                    }) {
                        return Err(
                            "a confirmation was already present before command injection".into(),
                        );
                    }
                    let input = crate::service::pty_io::submit_transaction::live_command_input(
                        &command,
                        screen.bracketed_paste_mode(pty_id),
                    );
                    let pty = self.pty.lock();
                    let session = pty
                        .get_session(pty_id)
                        .ok_or_else(|| "the command PTY closed".to_owned())?;
                    let permit = pty.issue_guarded_input_permit(pty_id)?;
                    (session, permit, input)
                };
                boundary.set(
                    std::fs::metadata(&path)
                        .map_err(|error| error.to_string())?
                        .len(),
                );
                transport::enqueue_command_observed(
                    &session.write_queue,
                    permit,
                    input.clone(),
                    deadline.saturating_duration_since(Instant::now()),
                    &injected,
                    move || {
                        if let Some(confirmation) = switch_confirmation {
                            confirmation.start();
                        }
                    },
                )?;
                crate::pty::capture::CaptureRegistry::global().tap_input(
                    pty_id,
                    "pty.write",
                    &input,
                );
                log::info!(
                    "[agent-model/timing] tab={} command={} phase=injected elapsed_ms={} at_ms={}",
                    target.tab_id,
                    command,
                    started.elapsed().as_millis(),
                    crate::service::pty_frame_timing::unix_ms()
                );
                Ok(())
            },
            || {
                if let Some(prompt) = prompt.as_mut() {
                    prompt.injected();
                }
                if let Some(picker) = picker.as_mut() {
                    picker.injected();
                }
                loop {
                    let observed = Instant::now();
                    let generation = signal.generation();
                    if !binding_current() {
                        break false;
                    }
                    if read_confirmation(&path, boundary.get(), source, &pattern)
                        && command_dialogs_absent(
                            self,
                            target,
                            prompt_pattern.as_ref(),
                            picker.as_ref(),
                        )
                    {
                        log::info!("[agent-model/timing] tab={} command={} phase=transcript_confirmed elapsed_ms={}", target.tab_id, command, started.elapsed().as_millis());
                        break true;
                    }
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        break false;
                    };
                    if let Some(prompt) = prompt.as_mut() {
                        let answered = prompt.answered();
                        if let Err(error) = self.respond_to_command_prompt(target, prompt) {
                            prompt_error = Some(error);
                            break false;
                        }
                        if !answered && prompt.answered() {
                            log::info!(
                                "[agent-model] own command dialog answered tab={} elapsed_ms={}",
                                target.tab_id,
                                started.elapsed().as_millis()
                            );
                        }
                    }
                    if let Some(picker) = picker.as_mut() {
                        if let Err(error) = self.respond_to_command_picker(target, picker) {
                            prompt_error = Some(error);
                            break false;
                        }
                    }
                    let observe_ms = observed.elapsed().as_millis();
                    if observe_ms >= 50 {
                        log::info!("[agent-model/timing] tab={} command={} phase=frame_observation phase_ms={} elapsed_ms={}", target.tab_id, command, observe_ms, started.elapsed().as_millis());
                    }
                    let waiting = Instant::now();
                    let changed = signal.wait_after(generation, remaining);
                    let wait_ms = waiting.elapsed().as_millis();
                    if wait_ms >= 50 {
                        log::info!("[agent-model/timing] tab={} command={} phase=frame_event_wait phase_ms={} changed={} elapsed_ms={}", target.tab_id, command, wait_ms, changed, started.elapsed().as_millis());
                    }
                    if !changed {
                        break binding_current()
                            && read_confirmation(&path, boundary.get(), source, &pattern)
                            && command_dialogs_absent(
                                self,
                                target,
                                prompt_pattern.as_ref(),
                                picker.as_ref(),
                            );
                    }
                }
            },
        );
        self.screen_state
            .lock()
            .unsubscribe_screen_changes(pty_id, &signal);
        let result = result.map_err(|error| prompt_error.unwrap_or(error));
        transport::finish_command(
            result,
            injected.get(),
            || {
                if let Some(prompt) = prompt.as_mut() {
                    prompt.injected();
                    self.cancel_command_prompt(target, prompt)?;
                }
                if let Some(picker) = picker.as_ref() {
                    self.cancel_command_picker(target, picker)?;
                }
                Ok(())
            },
            || {
                let mux = self.mux.read();
                if !mux
                    .get_tab(&target.tab_id)
                    .is_some_and(|tab| target.matches(tab) && tab.launch_session_authenticated())
                {
                    return Err("the session changed before input could be rearmed".into());
                }
                self.pty.lock().rearm_guarded_input(pty_id)
            },
        )
        .map_err(|error| {
            if not_ready.get() && !injected.get() {
                ModelSwitchError::InputNotReady(error)
            } else {
                ModelSwitchError::LiveCommandFailed(error)
            }
        })
    }
}

#[cfg(test)]
mod tests;
