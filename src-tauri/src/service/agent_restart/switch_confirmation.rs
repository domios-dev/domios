use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent::provider::{ModelSwitchConfirmationSource, ModelSwitchTranscript};
use crate::pty::screen_state::ScreenChangeSignal;
use crate::service::ServiceContext;

const MAX_EVIDENCE_BYTES: u64 = 1024 * 1024;

#[derive(Default)]
struct Attempt {
    answered: bool,
    confirmed: bool,
}

impl Attempt {
    fn observe(&mut self, evidence: bool, prompt_present: bool) -> bool {
        if evidence {
            self.confirmed = true;
        }
        if self.confirmed || self.answered || !prompt_present {
            return false;
        }
        self.answered = true;
        true
    }
}

pub(crate) struct PreparedSwitchConfirmation {
    service: ServiceContext,
    tab: String,
    pty: String,
    provider: String,
    session: String,
    nonce: String,
    selection: String,
    path: std::path::PathBuf,
    boundary: u64,
    source: ModelSwitchTranscript,
    confirmation: regex::Regex,
    signal: Arc<ScreenChangeSignal>,
    _watcher: notify::RecommendedWatcher,
}

impl PreparedSwitchConfirmation {
    pub(crate) fn start(self) {
        std::thread::spawn(move || self.run());
    }

    fn current(&self, tab: &crate::multiplexer::types::Tab) -> bool {
        tab.pty_id.as_deref() == Some(&self.pty)
            && tab.agent_type.as_deref() == Some(&self.provider)
            && tab.agent_session_id.as_deref() == Some(&self.session)
            && tab.session_binding_nonce.as_deref() == Some(&self.nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(&self.nonce)
    }

    fn evidence(&self) -> bool {
        let Ok(mut file) = File::open(&self.path) else {
            return false;
        };
        let Ok(metadata) = file.metadata() else {
            return false;
        };
        if metadata.len() < self.boundary
            || metadata.len() - self.boundary > MAX_EVIDENCE_BYTES
            || file.seek(SeekFrom::Start(self.boundary)).is_err()
        {
            return false;
        }
        let mut bytes = String::new();
        if file
            .take(MAX_EVIDENCE_BYTES)
            .read_to_string(&mut bytes)
            .is_err()
        {
            return false;
        }
        self.source
            .records(&bytes)
            .filter_map(Result::ok)
            .filter_map(|record| self.source.record_stdout(&record))
            .any(|stdout| {
                self.confirmation
                    .is_match(crate::pty::ansi::strip_ansi(&stdout).trim())
            })
    }

    fn run(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut attempt = Attempt::default();
        while Instant::now() < deadline {
            let generation = self.signal.generation();
            let evidence = self.evidence();
            let mux = self.service.mux.read();
            let Some(tab) = mux.get_tab(&self.tab).filter(|tab| self.current(tab)) else {
                break;
            };
            let _ = tab;
            let screens = self.service.screen_state.lock();
            let Some(screen) = screens.get_screen_text(&self.pty) else {
                break;
            };
            let mut tracker = self.service.restart_tracker.lock();
            if !tracker
                .switch_confirmations
                .get(&self.pty)
                .is_some_and(|signal| Arc::ptr_eq(signal, &self.signal))
            {
                break;
            }
            let response = self
                .service
                .agent_registry
                .with_provider(&self.provider, |provider| {
                    let prompt =
                        provider.parse_switch_confirmation_prompt(&screen, &self.selection)?;
                    let parsed = provider.parse_prompt(&screen)?;
                    if prompt.options != parsed.options || prompt.options.len() != 2 {
                        return None;
                    }
                    let frame_start = screen.rfind(&prompt.question)?;
                    provider
                        .live_prompt_response(0, &screen[frame_start..])
                        .filter(|steps| steps.len() == 1)
                        .map(|steps| steps.concat())
                        .filter(|keys| !keys.is_empty() && keys.len() <= 64)
                })
                .flatten();
            if attempt.observe(evidence, response.is_some()) {
                let pty = self.service.pty.lock();
                let result = pty
                    .issue_guarded_input_permit(&self.pty)
                    .and_then(|permit| {
                        let session = pty
                            .get_session(&self.pty)
                            .ok_or_else(|| "the switch PTY closed".to_owned())?;
                        session
                            .write_queue
                            .send_and_wait_guarded(
                                response.unwrap_or_default(),
                                permit,
                                Duration::ZERO,
                            )
                            .or_else(|error| {
                                if crate::pty::write_queue::is_write_ack_timeout(&error) {
                                    Ok(())
                                } else {
                                    Err(error)
                                }
                            })
                    });
                log::debug!(
                    "[switch-confirmation] answered tab={} result={:?}",
                    self.tab,
                    result
                );
                if result.is_err() {
                    break;
                }
            }
            if attempt.confirmed {
                tracker.switch_confirmations.remove(&self.pty);
                log::debug!(
                    "[switch-confirmation] own session confirmed tab={}",
                    self.tab
                );
                break;
            }
            drop(tracker);
            drop(screens);
            drop(mux);
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            if !self.signal.wait_after(generation, remaining) {
                break;
            }
        }
    }
}

impl Drop for PreparedSwitchConfirmation {
    fn drop(&mut self) {
        let mut tracker = self.service.restart_tracker.lock();
        if tracker
            .switch_confirmations
            .get(&self.pty)
            .is_some_and(|signal| Arc::ptr_eq(signal, &self.signal))
        {
            tracker.switch_confirmations.remove(&self.pty);
        }
        drop(tracker);
        self.service
            .screen_state
            .lock()
            .unsubscribe_screen_changes(&self.pty, &self.signal);
    }
}

impl ServiceContext {
    pub(crate) fn replace_switch_confirmation(
        &self,
        pty: &str,
        signal: Option<Arc<ScreenChangeSignal>>,
    ) {
        let mut tracker = self.restart_tracker.lock();
        if let Some(old) = tracker.switch_confirmations.remove(pty) {
            old.mark_changed();
        }
        if let Some(signal) = signal {
            tracker.switch_confirmations.insert(pty.to_owned(), signal);
        }
    }

    pub(crate) fn prepare_switch_confirmation(
        &self,
        pty: &str,
        text: &str,
        picker_selection: Option<&str>,
    ) -> Option<PreparedSwitchConfirmation> {
        if !text.trim_start().starts_with('/') {
            return None;
        }
        let words = text.split_whitespace().collect::<Vec<_>>();
        if !(1..=2).contains(&words.len()) || text.contains(['\n', '\r']) {
            return None;
        }
        let (tab, provider, session, nonce, cwd) = {
            let mux = self.mux.read();
            let tab_id = mux.tab_id_for_pty(pty)?;
            let tab = mux.get_tab(&tab_id)?;
            if tab.session_binding_nonce != tab.reported_session_binding_nonce {
                return None;
            }
            (
                tab_id,
                tab.agent_type.clone()?,
                tab.agent_session_id.clone()?,
                tab.session_binding_nonce.clone()?,
                tab.cwd.clone()?,
            )
        };
        let selection = words.get(1).copied().or(picker_selection)?.to_owned();
        let (source, confirmation, path) = self
            .agent_registry
            .with_provider(&provider, |provider| {
                let commands = provider.model_switch_commands()?;
                if commands.confirmation_source != ModelSwitchConfirmationSource::Transcript {
                    return None;
                }
                let template = [&commands.model]
                    .into_iter()
                    .chain(commands.effort.iter())
                    .find(|template| {
                        template.confirmation_prompt_kind.is_some()
                            && template.command.split_whitespace().next() == words.first().copied()
                    })?;
                let display = provider
                    .available_models()
                    .into_iter()
                    .find(|model| model.id == selection)
                    .map(|model| model.display_name)
                    .unwrap_or_else(|| selection.clone());
                let pattern = template
                    .confirmation
                    .replace("{model}", &regex::escape(&selection))
                    .replace("{display_name}", &regex::escape(&display))
                    .replace("{effort}", &regex::escape(&selection));
                let confirmation = regex::Regex::new(&pattern).ok()?;
                if template.confirmation.trim().is_empty() || confirmation.is_match("") {
                    return None;
                }
                Some((
                    commands.transcript,
                    confirmation,
                    provider.session_file_path(&cwd, &session)?,
                ))
            })
            .flatten()?;
        let screen = self.screen_state.lock().get_screen_text(pty)?;
        if self
            .agent_registry
            .with_provider(&provider, |provider| provider.parse_prompt(&screen))
            .flatten()
            .is_some()
        {
            return None;
        }
        let signal = self.screen_state.lock().subscribe_screen_changes(pty);
        let result = (|| {
            let wake = signal.clone();
            let own_path = path.clone();
            let mut watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if event.is_ok_and(|event| event.paths.iter().any(|path| path == &own_path)) {
                        wake.mark_changed();
                    }
                })
                .ok()?;
            notify::Watcher::watch(
                &mut watcher,
                path.parent()?,
                notify::RecursiveMode::NonRecursive,
            )
            .ok()?;
            let boundary = std::fs::metadata(&path).ok()?.len();
            Some((watcher, boundary))
        })();
        let Some((watcher, boundary)) = result else {
            self.screen_state
                .lock()
                .unsubscribe_screen_changes(pty, &signal);
            return None;
        };
        self.replace_switch_confirmation(pty, Some(signal.clone()));
        Some(PreparedSwitchConfirmation {
            service: self.clone(),
            tab,
            pty: pty.to_owned(),
            provider,
            session,
            nonce,
            selection,
            path,
            boundary,
            source,
            confirmation,
            signal,
            _watcher: watcher,
        })
    }
}

#[cfg(test)]
mod tests;
