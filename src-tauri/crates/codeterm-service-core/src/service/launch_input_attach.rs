//! A provider that declares an input-attach signal is input-ready only after its
//! own input loop owns the PTY: earlier keystrokes become draft text, and a resumed
//! REPL ignores Enter until its session-start hook has run, so a held send or
//! restart continue written earlier sits unsubmitted. Readiness comes from the
//! provider's own events; a deadline only makes the wait visible.

use super::ServiceContext;
use crate::agent::provider::InputAttachSignal;
use crate::multiplexer::types::TabBacking;
use crate::service::boot_stage::{binding_disproves_latched_failure, BootStage};
use crate::service::integration_silence::{IntegrationFacts, IntegrationState};
use crate::service::launch_readiness::LaunchAttempt;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "launch_input_attach/tests.rs"]
mod tests;

static WATCHING: LazyLock<parking_lot::Mutex<HashSet<LaunchAttempt>>> =
    LazyLock::new(Default::default);

/// What a launch has shown about its provider's own session start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionStart {
    /// The launch's own session-start hook reported its binding nonce.
    Reported,
    /// The exact provider process published the session in its launch registry.
    ObservedByRegistry,
    /// The provider's hooks were not in place at launch, so no report can arrive.
    HooksUnavailable,
    Awaited,
}

/// Session-start evidence per tab, each value keyed to the launch nonce it proves.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TabSessionStart {
    reported_nonce: Option<String>,
    registry_binding: Option<(String, String)>,
    availability: Option<(String, bool)>,
}

#[derive(Default)]
pub(crate) struct SessionStartHooks {
    tabs: parking_lot::Mutex<HashMap<String, TabSessionStart>>,
}

fn session_start(record: Option<&TabSessionStart>, attempt: &LaunchAttempt) -> SessionStart {
    let Some(record) = record else {
        return SessionStart::Awaited;
    };
    if record.reported_nonce.as_deref() == Some(attempt.binding_nonce.as_str()) {
        SessionStart::Reported
    } else if record
        .registry_binding
        .as_ref()
        .is_some_and(|(pty, nonce)| pty == &attempt.pty_id && nonce == &attempt.binding_nonce)
    {
        SessionStart::ObservedByRegistry
    } else if record
        .availability
        .as_ref()
        .is_some_and(|(nonce, available)| nonce == &attempt.binding_nonce && !available)
    {
        SessionStart::HooksUnavailable
    } else {
        SessionStart::Awaited
    }
}

/// A session-start report can arrive only when the provider registers sessions
/// through hooks and its own install report says those hooks are in place.
pub(crate) fn session_start_hooks_available(facts: &IntegrationFacts) -> bool {
    facts.registers_sessions_via_hooks && facts.state == Some(IntegrationState::Installed)
}

pub(crate) fn input_ready(
    signal: InputAttachSignal,
    bracketed_paste: Option<bool>,
    session_start: SessionStart,
) -> bool {
    let attached = bracketed_paste == Some(true);
    match signal {
        InputAttachSignal::None => true,
        InputAttachSignal::BracketedPaste => attached,
        InputAttachSignal::SessionStartAndBracketedPaste => {
            attached && session_start != SessionStart::Awaited
        }
    }
}

pub(crate) fn input_not_attached_reason(tab_id: &str) -> String {
    format!(
        "the agent has not attached its input to the terminal or started its session yet; \
         queued input stays held. Open the tab to check it, or run `domios agent restart --tab {tab_id}`"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum AttachOutcome {
    Attached,
    Superseded,
    ScreenClosed,
}

/// Waits on the PTY's own output events. The deadline records an honest
/// provisional stage once; only the attach itself, or the launch ending, stops it.
fn await_attach(
    attached: impl Fn() -> bool,
    current: impl Fn() -> bool,
    wait: impl Fn(Option<Duration>) -> bool,
    on_deadline: impl FnOnce(),
    deadline: Instant,
) -> AttachOutcome {
    let mut on_deadline = Some(on_deadline);
    loop {
        if !current() {
            return AttachOutcome::Superseded;
        }
        if attached() {
            return AttachOutcome::Attached;
        }
        let remaining = deadline.checked_duration_since(Instant::now());
        let bounded = on_deadline.is_some();
        if bounded && remaining.is_none_or(|left| left.is_zero()) {
            if let Some(record) = on_deadline.take() {
                record();
            }
            continue;
        }
        if !wait(remaining.filter(|_| bounded)) && (!bounded || Instant::now() < deadline) {
            return AttachOutcome::ScreenClosed;
        }
    }
}

impl ServiceContext {
    /// True when `AgentReady` must wait for this tab's declared input attach. The
    /// watcher it arms re-emits `AgentReady` once the provider's input owns the PTY.
    pub(crate) fn agent_ready_awaits_input_attach(&self, tab_id: &str) -> bool {
        let Some((attempt, agent)) = self.input_attach_candidate(tab_id) else {
            return false;
        };
        let (signal, timeout) = self
            .agent_registry
            .with_provider(&agent, |provider| {
                (
                    provider.input_attach_signal(),
                    provider.provider_readiness_strategy().timeout(),
                )
            })
            .unwrap_or((InputAttachSignal::None, Duration::ZERO));
        if self.launch_input_ready(&attempt, signal) {
            return false;
        }
        self.watch_input_attach(attempt, agent, signal, timeout);
        true
    }

    /// The tab's provider ran its own session-start hook carrying this launch's
    /// binding nonce. A launch held only for that report becomes ready now.
    pub fn observe_session_start_hook(&self, tab_id: &str, binding_nonce: &str) {
        let Some((attempt, _)) = self
            .input_attach_candidate(tab_id)
            .filter(|(attempt, _)| attempt.binding_nonce == binding_nonce)
        else {
            return;
        };
        {
            let mux = self.mux.read();
            if !mux
                .get_tab(tab_id)
                .is_some_and(|tab| super::launch_readiness::attempt_owns_tab(tab, &attempt))
            {
                return;
            }
            self.session_start_hooks()
                .tabs
                .lock()
                .entry(tab_id.to_string())
                .or_default()
                .reported_nonce = Some(binding_nonce.to_string());
        }
        self.screen_state
            .arm_screen_change_signal(&attempt.pty_id)
            .mark_changed();
        let held = WATCHING.lock().contains(&attempt);
        if held && self.attempt_awaits_ready(&attempt) {
            self.emit_boot_stage(tab_id, BootStage::AgentReady, None);
        }
    }

    pub(crate) fn observe_registry_session_start(&self, attempt: &LaunchAttempt, session_id: &str) {
        {
            let mux = self.mux.read();
            if !mux.get_tab(&attempt.tab_id).is_some_and(|tab| {
                super::launch_readiness::attempt_owns_tab(tab, attempt)
                    && tab.agent_session_id.as_deref() == Some(session_id)
                    && tab.launch_session_authenticated()
            }) {
                return;
            }
            self.session_start_hooks()
                .tabs
                .lock()
                .entry(attempt.tab_id.clone())
                .or_default()
                .registry_binding = Some((attempt.pty_id.clone(), attempt.binding_nonce.clone()));
        }
        self.screen_state
            .arm_screen_change_signal(&attempt.pty_id)
            .mark_changed();
    }

    /// Whether this tab's provider declares an input attach, so only `AgentReady`
    /// proves its input is accepted. Read before any mux guard is held.
    pub(crate) fn declares_input_attach(&self, tab_id: &str) -> bool {
        let agent = {
            let mux = self.mux.read();
            mux.resolve_tab_id(tab_id)
                .and_then(|id| mux.get_tab(&id))
                .and_then(|tab| tab.agent_type.clone())
        };
        agent
            .and_then(|agent| {
                self.agent_registry
                    .with_provider(&agent, |provider| provider.input_attach_signal())
            })
            .is_some_and(|signal| signal != InputAttachSignal::None)
    }

    fn session_start_hooks(&self) -> &SessionStartHooks {
        self.extensions.get_or_init(SessionStartHooks::default)
    }

    fn launch_input_ready(&self, attempt: &LaunchAttempt, signal: InputAttachSignal) -> bool {
        let started = session_start(
            self.session_start_hooks().tabs.lock().get(&attempt.tab_id),
            attempt,
        );
        if started == SessionStart::HooksUnavailable
            && signal == InputAttachSignal::SessionStartAndBracketedPaste
            && self.supports_exact_launch_detection(&attempt.tab_id)
        {
            return false;
        }
        input_ready(
            signal,
            self.screen_state.bracketed_paste_mode(&attempt.pty_id),
            started,
        )
    }

    pub(crate) fn prepare_launch_session_hook(
        &self,
        tab_id: &str,
        nonce: &str,
        invocation: &codeterm_plugin::provider_account::InvocationContext,
    ) -> Result<(), String> {
        let expected = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .filter(|tab| tab.session_binding_nonce.as_deref() == Some(nonce))
            .ok_or("launch_hook_superseded: original launch unavailable")?;
        let available = super::provider_accounts::prepare_named_session_hook(self, invocation)?;
        let mux = self.mux.read();
        if !mux.get_tab(tab_id).is_some_and(|current| {
            super::messenger::switch_handoff::same_binding(&expected, current)
        }) {
            return Err(
                "launch_hook_superseded: launch authority changed during provisioning".into(),
            );
        }
        if let Some(available) = available {
            self.store_launch_session_hook(tab_id, nonce, available);
        }
        Ok(())
    }

    pub(crate) fn hookless_launch_has_input(&self, attempt: &LaunchAttempt) -> bool {
        session_start(
            self.session_start_hooks().tabs.lock().get(&attempt.tab_id),
            attempt,
        ) == SessionStart::HooksUnavailable
            && self.screen_state.bracketed_paste_mode(&attempt.pty_id) == Some(true)
    }

    pub(crate) fn record_launch_session_hook(&self, tab_id: &str, nonce: &str, available: bool) {
        let mux = self.mux.read();
        if !mux
            .get_tab(tab_id)
            .is_some_and(|tab| tab.session_binding_nonce.as_deref() == Some(nonce))
        {
            return;
        }
        self.store_launch_session_hook(tab_id, nonce, available);
    }

    fn store_launch_session_hook(&self, tab_id: &str, nonce: &str, available: bool) {
        self.session_start_hooks()
            .tabs
            .lock()
            .entry(tab_id.to_string())
            .or_default()
            .availability = Some((nonce.to_string(), available));
    }

    fn settle_hookless_launch(&self, attempt: &LaunchAttempt, agent: &str) {
        let recorded = self
            .session_start_hooks()
            .tabs
            .lock()
            .get(&attempt.tab_id)
            .and_then(|record| record.availability.as_ref())
            .is_some_and(|(nonce, _)| nonce == &attempt.binding_nonce);
        if recorded {
            return;
        }
        let named = self.mux.read().get_tab(&attempt.tab_id).is_some_and(|tab| {
            super::provider_accounts::selected_account(tab)
                != codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT
        });
        let available = !named
            && super::integration_silence::read_provider_facts(&self.agent_registry, agent, false)
                .is_some_and(|(facts, _)| session_start_hooks_available(&facts));
        if !available {
            self.record_launch_session_hook(&attempt.tab_id, &attempt.binding_nonce, false);
        }
    }

    fn input_attach_candidate(&self, tab_id: &str) -> Option<(LaunchAttempt, String)> {
        let mux = self.mux.read();
        let tab = mux.get_tab(tab_id)?;
        if !matches!(tab.backing, TabBacking::Pty) {
            return None;
        }
        Some((
            LaunchAttempt {
                tab_id: tab_id.to_string(),
                pty_id: tab.pty_id.clone()?,
                binding_nonce: tab.session_binding_nonce.clone()?,
            },
            tab.agent_type.clone()?,
        ))
    }

    fn watch_input_attach(
        &self,
        attempt: LaunchAttempt,
        agent: String,
        signal: InputAttachSignal,
        timeout: Duration,
    ) {
        let key = attempt.clone();
        if !WATCHING.lock().insert(key.clone()) {
            return;
        }
        log::info!(
            "[launch-input-attach] tab={} pty={} AgentReady waits for {signal:?}",
            attempt.tab_id,
            attempt.pty_id
        );
        let service = self.clone();
        let cleanup = key.clone();
        let spawned = std::thread::Builder::new()
            .name("input-attach".into())
            .spawn(move || {
                if signal == InputAttachSignal::SessionStartAndBracketedPaste {
                    service.settle_hookless_launch(&attempt, &agent);
                }
                let outcome = service.await_input_attach(&attempt, signal, timeout);
                WATCHING.lock().remove(&key);
                log::info!(
                    "[launch-input-attach] tab={} pty={} outcome={outcome:?}",
                    attempt.tab_id,
                    attempt.pty_id
                );
                // A registry binding can revive a failed launch as its watcher exits.
                if outcome != AttachOutcome::ScreenClosed && service.attempt_awaits_ready(&attempt)
                {
                    service.complete_launch_input_attach(&attempt);
                }
            });
        if let Err(error) = spawned {
            WATCHING.lock().remove(&cleanup);
            log::error!("[launch-input-attach] watcher failed to start: {error}");
        }
    }

    fn await_input_attach(
        &self,
        attempt: &LaunchAttempt,
        signal: InputAttachSignal,
        timeout: Duration,
    ) -> AttachOutcome {
        let change = self.screen_state.arm_screen_change_signal(&attempt.pty_id);
        let generation = std::cell::Cell::new(change.generation());
        await_attach(
            || {
                generation.set(change.generation());
                if signal == InputAttachSignal::SessionStartAndBracketedPaste
                    && self.hookless_launch_has_input(attempt)
                {
                    self.detect_exact_launch_attempt(attempt);
                }
                self.launch_input_ready(attempt, signal)
            },
            || {
                LaunchAttempt::current(&self.mux, &attempt.tab_id, &attempt.pty_id).as_ref()
                    == Some(attempt)
                    && self.attempt_awaits_ready(attempt)
            },
            |remaining| match remaining {
                Some(left) => change.wait_after(generation.get(), left),
                None => change.wait_until_changed(generation.get()),
            },
            || {
                self.record_provisional_launch(
                    attempt,
                    BootStage::LaunchDeadlineElapsed,
                    &input_not_attached_reason(&attempt.tab_id),
                )
            },
            Instant::now() + timeout,
        )
    }

    fn attempt_awaits_ready(&self, attempt: &LaunchAttempt) -> bool {
        let mux = self.mux.read();
        mux.get_tab(&attempt.tab_id).is_some_and(|tab| {
            crate::service::launch_readiness::attempt_owns_tab(tab, attempt)
                && tab.boot_stage != Some(BootStage::AgentReady)
                && (tab.boot_stage != Some(BootStage::SpawnFailed)
                    || binding_disproves_latched_failure(tab))
        })
    }
}
