//! Owned lifecycle for the deferred provider-update waiter. The thread holds a
//! `Weak` execution context, so owner teardown alone decides how long it lives.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;
use tokio::sync::Notify;

use crate::api::events::{SeqEvent, SystemEvent};

use super::{ProviderUpdateRegistry, ServiceContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum WaiterPhase {
    #[default]
    Open,
    ShuttingDown,
    Closed,
}

struct WaiterRecord {
    cancel: Arc<Notify>,
    handle: std::thread::JoinHandle<()>,
}

/// Registration and shutdown are linearized by this one lock, so a waiter can
/// never be spawned into a lifecycle that has already begun shutting down.
#[derive(Default)]
pub(crate) struct WaiterLifecycle {
    phase: WaiterPhase,
    records: HashMap<String, WaiterRecord>,
}

impl ProviderUpdateRegistry {
    /// The owning `Arc<ServiceContext>` is the waiter's lifetime. The registry
    /// keeps only a `Weak`, so binding cannot create a self-sustaining cycle.
    pub(crate) fn bind_execution_owner(&self, owner: &Arc<ServiceContext>) {
        *self.exec.lock() = Arc::downgrade(owner);
    }

    #[cfg(test)]
    pub(super) fn waiter_count(&self) -> usize {
        self.waiters.lock().records.len()
    }

    #[cfg(test)]
    pub(super) fn execution_owner_strong_count(&self) -> usize {
        self.exec.lock().strong_count()
    }

    /// A waiter that has left its loop drops its own handle: joining a finished
    /// thread adds nothing, and the registry must not grow one entry per prompt.
    fn retire_waiter(&self, prompt_id: &str) {
        self.waiters.lock().records.remove(prompt_id);
    }
}

impl ServiceContext {
    /// Terminates and joins every provider-update waiter, returning how many it
    /// joined. Terminal and idempotent: a closed lifecycle starts no more.
    pub(crate) fn shutdown_provider_update_waiters(&self) -> usize {
        let drained: Vec<(String, WaiterRecord)> = {
            let mut lifecycle = self.provider_updates.waiters.lock();
            if lifecycle.phase == WaiterPhase::Closed {
                return 0;
            }
            lifecycle.phase = WaiterPhase::ShuttingDown;
            lifecycle.records.drain().collect()
        };
        let prompt_ids: Vec<String> = drained.iter().map(|(id, _)| id.clone()).collect();
        self.provider_updates.clear_waiter_started(&prompt_ids);
        for (_, record) in &drained {
            record.cancel.notify_one();
        }
        let joined = drained.len();
        for (_, record) in drained {
            let _ = record.handle.join();
        }
        self.provider_updates.waiters.lock().phase = WaiterPhase::Closed;
        joined
    }

    /// `true` when this call started and registered the waiter.
    pub(super) fn start_provider_update_waiter(
        &self,
        prompt_id: String,
        receiver: Receiver<SeqEvent>,
    ) -> bool {
        let exec = self.provider_updates.exec.lock().clone();
        if exec.strong_count() == 0 {
            log::warn!("[provider-update] no execution owner bound for prompt={prompt_id}");
            return false;
        }
        if !self.provider_updates.mark_waiter_started(&prompt_id) {
            return false;
        }
        #[cfg(test)]
        let completion = self.provider_updates.probe.lock().clone();
        let cancel = Arc::new(Notify::new());
        let waiter_cancel = cancel.clone();
        let waiter_prompt = prompt_id.clone();
        let started = {
            let mut lifecycle = self.provider_updates.waiters.lock();
            if lifecycle.phase == WaiterPhase::Open {
                // Keep events queued after consent but before the thread starts.
                let handle = std::thread::spawn(move || {
                    run_waiter(&waiter_prompt, receiver, waiter_cancel, &exec);
                    if let Some(service) = exec.upgrade() {
                        service.provider_updates.retire_waiter(&waiter_prompt);
                    }
                    #[cfg(test)]
                    if let Some(sender) = completion {
                        let _ = sender.send(super::ProviderUpdateTestEffect::WaiterFinished {
                            prompt_id: waiter_prompt,
                        });
                    }
                });
                lifecycle
                    .records
                    .insert(prompt_id.clone(), WaiterRecord { cancel, handle });
                true
            } else {
                false
            }
        };
        if !started {
            self.provider_updates
                .clear_waiter_started(std::slice::from_ref(&prompt_id));
        }
        started
    }
}

fn run_waiter(
    prompt_id: &str,
    mut receiver: Receiver<SeqEvent>,
    cancel: Arc<Notify>,
    exec: &Weak<ServiceContext>,
) {
    futures_executor::block_on(async {
        loop {
            let received = tokio::select! {
                biased;
                _ = cancel.notified() => return,
                received = receiver.recv() => received,
            };
            let recheck = match received {
                // A dropped wake frame must not strand a resolvable prompt, so
                // a lagged stream rechecks authoritative state instead.
                Err(RecvError::Lagged(_)) => true,
                Err(RecvError::Closed) => return,
                Ok(event) => matches!(
                    event.event,
                    SystemEvent::MuxStateChanged | SystemEvent::TabClosed { .. }
                ),
            };
            if !recheck {
                continue;
            }
            // The upgrade is held across the recheck only, never across a wait.
            let settled = match exec.upgrade() {
                None => true,
                Some(service) => {
                    !matches!(service.try_execute_provider_update(prompt_id), Ok(false))
                }
            };
            if settled {
                return;
            }
        }
    });
}
