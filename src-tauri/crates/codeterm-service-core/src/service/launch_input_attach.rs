//! A provider that declares an input-attach signal is input-ready only after its
//! own input loop owns the PTY: earlier keystrokes become draft text, so a held
//! send or restart continue written then sits unsubmitted. The attach is the
//! provider's own terminal-mode event; a deadline only makes the wait visible.

use super::ServiceContext;
use crate::agent::provider::InputAttachSignal;
use crate::multiplexer::types::TabBacking;
use crate::service::boot_stage::BootStage;
use crate::service::launch_readiness::LaunchAttempt;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "launch_input_attach/tests.rs"]
mod tests;

static WATCHING: LazyLock<parking_lot::Mutex<HashSet<(String, String)>>> =
    LazyLock::new(Default::default);

pub(crate) fn input_attached(signal: InputAttachSignal, bracketed_paste: Option<bool>) -> bool {
    match signal {
        InputAttachSignal::None => true,
        InputAttachSignal::BracketedPaste => bracketed_paste == Some(true),
    }
}

pub(crate) fn input_not_attached_reason(tab_id: &str) -> String {
    format!(
        "the agent has not attached its input to the terminal yet; queued input stays held. \
         Open the tab to check it, or run `domios agent restart --tab {tab_id}`"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum AttachOutcome {
    Attached,
    Superseded,
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
            return AttachOutcome::Superseded;
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
        if input_attached(
            signal,
            self.screen_state.bracketed_paste_mode(&attempt.pty_id),
        ) {
            return false;
        }
        self.watch_input_attach(attempt, signal, timeout);
        true
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
        signal: InputAttachSignal,
        timeout: Duration,
    ) {
        let key = (attempt.tab_id.clone(), attempt.pty_id.clone());
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
                let outcome = service.await_input_attach(&attempt, signal, timeout);
                WATCHING.lock().remove(&key);
                log::info!(
                    "[launch-input-attach] tab={} pty={} outcome={outcome:?}",
                    attempt.tab_id,
                    attempt.pty_id
                );
                if outcome == AttachOutcome::Attached && service.attempt_awaits_ready(&attempt) {
                    service.emit_boot_stage(&attempt.tab_id, BootStage::AgentReady, None);
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
                input_attached(
                    signal,
                    self.screen_state.bracketed_paste_mode(&attempt.pty_id),
                )
            },
            || {
                LaunchAttempt::current(&self.mux, &attempt.tab_id, &attempt.pty_id).as_ref()
                    == Some(attempt)
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
                && !matches!(
                    tab.boot_stage,
                    Some(BootStage::AgentReady | BootStage::SpawnFailed)
                )
        })
    }
}
