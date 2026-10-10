use std::time::{Duration, Instant};

use codeterm_plugin::provider::DirectoryTrustPromptResponse;
use codeterm_pty::pty::screen_state::ScreenChangeSignal;

use super::{frame_id, ServiceContext, LEDGER};

const ANSWER_DEADLINE: Duration = Duration::from_secs(6);

struct Frame {
    generation: u64,
    prompt: Option<(String, DirectoryTrustPromptResponse)>,
}

trait AnswerIo {
    fn read(&mut self) -> Result<Frame, String>;
    fn write(&mut self, occurrence: &str, step: &[u8]) -> Result<u64, String>;
    fn wait_after(&mut self, generation: u64, deadline: Instant) -> bool;
    fn now(&self) -> Instant {
        Instant::now()
    }
}

fn next_step(frame: &Frame, occurrence: &str) -> Result<Vec<u8>, String> {
    match &frame.prompt {
        Some((current, DirectoryTrustPromptResponse::Answer { step }))
            if current == occurrence && !step.is_empty() =>
        {
            Ok(step.clone())
        }
        _ => Err("folder trust occurrence changed or became ambiguous".into()),
    }
}

fn write_before_deadline(
    io: &mut impl AnswerIo,
    occurrence: &str,
    step: &[u8],
    deadline: Instant,
) -> Result<u64, String> {
    if io.now() >= deadline {
        return Err("folder trust answer deadline expired before write".into());
    }
    io.write(occurrence, step)
}

fn answer(io: &mut impl AnswerIo, occurrence: &str, started: Instant) -> Result<(), String> {
    let deadline = started + ANSWER_DEADLINE;
    let focus_deadline = started + ANSWER_DEADLINE / 2;
    let mut frame = io.read()?;
    let mut step = next_step(&frame, occurrence)?;
    if step != b"\r" {
        let mut generation = write_before_deadline(io, occurrence, &step, deadline)?;
        let mut retried = false;
        loop {
            let focus_limit = if retried { deadline } else { focus_deadline };
            let changed = io.wait_after(generation, focus_limit);
            frame = io.read()?;
            step = next_step(&frame, occurrence)?;
            if step == b"\r" && frame.generation != generation {
                break;
            }
            generation = frame.generation;
            if !changed {
                if retried {
                    return Err("folder trust affirmative focus was not confirmed".into());
                }
                if step == b"\r" {
                    return Err("folder trust focus had no later frame".into());
                }
                generation = write_before_deadline(io, occurrence, &step, deadline)?;
                retried = true;
            }
        }
    }
    let mut generation = write_before_deadline(io, occurrence, b"\r", deadline)?;
    loop {
        if !io.wait_after(generation, deadline) {
            return Err("folder trust acceptance was not confirmed".into());
        }
        frame = io.read()?;
        if frame.generation == generation {
            return Err("folder trust acceptance had no later frame".into());
        }
        if frame.prompt.is_none() {
            return Ok(());
        }
        next_step(&frame, occurrence)?;
        generation = frame.generation;
    }
}

struct BoundAnswerIo {
    service: ServiceContext,
    tab: String,
    pty: String,
    agent: String,
    nonce: Option<String>,
    signal: std::sync::Arc<ScreenChangeSignal>,
}

impl AnswerIo for BoundAnswerIo {
    fn read(&mut self) -> Result<Frame, String> {
        let bound = self
            .service
            .mux
            .read()
            .get_tab(&self.tab)
            .is_some_and(|tab| {
                tab.pty_id.as_deref() == Some(self.pty.as_str())
                    && tab.agent_type.as_deref() == Some(self.agent.as_str())
                    && tab.session_binding_nonce == self.nonce
            });
        if !bound {
            return Err("folder trust PTY binding changed".into());
        }
        let (generation, screen) = {
            let state = self.service.screen_state.lock();
            let generation = self.signal.generation();
            let screen = state
                .get_screen_text(&self.pty)
                .ok_or("folder trust screen unavailable")?;
            (generation, screen)
        };
        let prompt = self
            .service
            .provider_directory_trust_prompt(&self.agent, &screen);
        let recognized = self
            .service
            .agent_registry
            .with_provider(&self.agent, |provider| {
                provider.directory_trust_prompt_response(&screen).is_some()
            })
            .ok_or("folder trust provider unavailable")?;
        if recognized && prompt.is_none() {
            return Err("folder trust prompt became ambiguous".into());
        }
        Ok(Frame { generation, prompt })
    }

    fn write(&mut self, occurrence: &str, step: &[u8]) -> Result<u64, String> {
        let frame = self.read()?;
        if next_step(&frame, occurrence)? != step {
            return Err("folder trust stale key withheld".into());
        }
        self.service.pty.lock().write(&self.pty, step)?;
        Ok(frame.generation)
    }

    fn wait_after(&mut self, generation: u64, deadline: Instant) -> bool {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| self.signal.wait_after(generation, remaining))
    }
}

impl ServiceContext {
    pub(crate) fn queue_directory_trust_answer<F>(
        &self,
        tab_id: &str,
        pty_id: &str,
        agent_type: &str,
        occurrence: &str,
        authorize: F,
    ) where
        F: FnOnce() -> bool + Send + 'static,
    {
        #[cfg(test)]
        crate::service::output::record_prompt_step_dispatch_for_test(pty_id);
        let service = self.clone();
        let tab = tab_id.to_string();
        let pty = pty_id.to_string();
        let agent = agent_type.to_string();
        let occurrence = occurrence.to_string();
        let nonce = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.session_binding_nonce.clone());
        let signal = self.screen_state.lock().subscribe_screen_changes(&pty);
        std::thread::spawn(move || {
            let result = if authorize() {
                let mut io = BoundAnswerIo {
                    service: service.clone(),
                    tab: tab.clone(),
                    pty: pty.clone(),
                    agent: agent.clone(),
                    nonce: nonce.clone(),
                    signal: signal.clone(),
                };
                answer(&mut io, &occurrence, Instant::now())
            } else {
                Err("folder trust authorization or binding changed".into())
            };
            service
                .screen_state
                .lock()
                .unsubscribe_screen_changes(&pty, &signal);
            LEDGER
                .lock()
                .finish_trust_answer(&tab, frame_id(&occurrence), result.is_ok());
            match result {
                Ok(()) => {
                    let pending = service.mux.read().get_tab(&tab).is_some_and(|tab| {
                        tab.pty_id.as_deref() == Some(pty.as_str())
                            && tab.session_binding_nonce == nonce
                            && tab.pending_prompt_kind
                                == Some(codeterm_types::pty::PromptKind::DirectoryTrust)
                            && tab.pending_prompt.as_ref().is_some_and(|prompt| {
                                super::directory_trust_occurrence(prompt) == occurrence
                            })
                    });
                    if pending {
                        service.prompt_detector.lock().mark_responded(&tab);
                    }
                    log::info!(
                        "[directory-trust] Accepted folder trust for Tab {}: dialog disappeared",
                        tab
                    );
                }
                Err(error) => {
                    let screen = service
                        .screen_state
                        .lock()
                        .get_screen_text(&pty)
                        .unwrap_or_default();
                    service.note_directory_trust(
                        &tab,
                        "folder trust transaction failed",
                        Some(&screen),
                    );
                    log::warn!(
                        "[directory-trust] Failed folder trust for Tab {}: {}",
                        tab,
                        error
                    );
                    let bound = service.mux.read().get_tab(&tab).is_some_and(|tab| {
                        tab.pty_id.as_deref() == Some(pty.as_str())
                            && tab.agent_type.as_deref() == Some(agent.as_str())
                            && tab.session_binding_nonce == nonce
                    });
                    if bound {
                        if let Some(prompt) = service
                            .agent_registry
                            .with_provider(&agent, |provider| {
                                provider.parse_directory_trust_prompt(&screen)
                            })
                            .flatten()
                        {
                            service.publish_unattended_launch_prompt(
                                &tab,
                                &screen,
                                prompt,
                                codeterm_types::pty::PromptKind::DirectoryTrust,
                                true,
                            );
                        }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;
