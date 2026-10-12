use crate::{multiplexer::types::Tab, service::boot_stage::BootStage};
use codeterm_analytics::{Event, ProviderId, RestoreOutcome};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Restores {
    attempts: HashMap<String, Attempt>,
}
struct Attempt {
    generation: u64,
    nonce: Option<String>,
    session: Option<String>,
    provider: ProviderId,
}
impl Restores {
    pub(crate) fn begin(
        &mut self,
        tab: &str,
        generation: u64,
        nonce: Option<String>,
        session: Option<String>,
        provider: Option<&str>,
    ) {
        self.attempts.remove(tab);
        if let Some(provider) = provider {
            self.attempts.insert(
                tab.into(),
                Attempt {
                    generation,
                    nonce,
                    session,
                    provider: ProviderId::from_catalog(provider).unwrap_or(ProviderId::Other),
                },
            );
        }
    }
    pub(crate) fn bind(&mut self, tab: &str, generation: u64, nonce: &str) {
        if let Some(attempt) = self
            .attempts
            .get_mut(tab)
            .filter(|attempt| attempt.generation == generation)
        {
            attempt.nonce = Some(nonce.into());
        }
    }
    pub(crate) fn settle(
        &mut self,
        tab: &str,
        nonce: Option<&str>,
        session: Option<&str>,
        authenticated: bool,
        stage: BootStage,
    ) -> Option<Event> {
        let attempt = self.attempts.get(tab)?;
        if attempt.nonce.as_deref() != nonce {
            return None;
        }
        let outcome = match stage {
            BootStage::SpawnFailed => RestoreOutcome::Failed,
            BootStage::AgentReady if authenticated => match attempt.session.as_deref() {
                Some(expected) if session == Some(expected) => RestoreOutcome::Resumed,
                None => RestoreOutcome::FreshLaunch,
                _ => return None,
            },
            _ => return None,
        };
        let provider_id = attempt.provider;
        self.attempts.remove(tab);
        Some(Event::AgentRestored {
            provider_id,
            outcome,
        })
    }
}
static RESTORES: std::sync::LazyLock<parking_lot::Mutex<Restores>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(Restores::default()));
pub(crate) fn begin(
    tab: &str,
    generation: u64,
    nonce: Option<String>,
    session: Option<String>,
    provider: Option<&str>,
) {
    RESTORES
        .lock()
        .begin(tab, generation, nonce, session, provider);
}
pub(crate) fn bind(tab: &str, generation: u64, nonce: &str) {
    RESTORES.lock().bind(tab, generation, nonce);
}
pub(crate) fn transition(tab_id: &str, tab: &Tab, stage: BootStage) -> Option<Event> {
    RESTORES.lock().settle(
        tab_id,
        tab.session_binding_nonce.as_deref(),
        tab.agent_session_id.as_deref(),
        tab.launch_session_authenticated(),
        stage,
    )
}

pub(crate) fn discard(tab: &str) {
    RESTORES.lock().attempts.remove(tab);
}
