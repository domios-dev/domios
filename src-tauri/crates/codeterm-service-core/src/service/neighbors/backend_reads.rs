use crate::service::ServiceContext;
use codeterm_neighbors::{supervisor::reconnect_delay, Error, Result};
use serde_json::Value;
use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

#[derive(Default)]
struct State {
    key: Option<(String, u64)>,
    result: Option<Result<Value>>,
    failures: usize,
    rearmed: u64,
    retry_at: Option<Instant>,
}

#[derive(Default)]
struct ReadGate {
    session_cached: bool,
    revision: AtomicU64,
    state: tokio::sync::Mutex<State>,
    rearmed: AtomicU64,
}

impl ReadGate {
    async fn read(
        &self,
        key: (String, u64),
        locked: bool,
        now: impl Fn() -> Instant,
        jitter: impl Fn() -> u64,
        load: impl Future<Output = Result<Value>>,
    ) -> Result<Value> {
        if locked {
            return Err(Error::Locked);
        }
        let revision = self.revision.load(Ordering::Acquire);
        let rearmed = self.rearmed.load(Ordering::Acquire);
        let mut state = self.state.lock().await;
        if state.key.as_ref() != Some(&key) {
            *state = State {
                key: Some(key),
                ..State::default()
            };
        } else if let Some(result) = &state.result {
            if self.session_cached || self.revision.load(Ordering::Acquire) != revision {
                return result.clone();
            }
            if let Err(error) = result {
                let permanent = !codeterm_neighbors::supervisor::retryable(*error);
                if permanent && rearmed == state.rearmed
                    || !permanent && state.retry_at.is_some_and(|at| now() < at)
                {
                    return Err(*error);
                }
            }
        }
        state.rearmed = rearmed;
        let result = load.await;
        match result {
            Ok(_) => {
                state.failures = 0;
                state.retry_at = None;
            }
            Err(_) => {
                state.failures = state.failures.saturating_add(1);
                state.retry_at = Some(now() + reconnect_delay(state.failures, jitter()));
            }
        }
        state.result = Some(result.clone());
        self.revision.fetch_add(1, Ordering::Release);
        result
    }

    fn rearm(&self) {
        self.rearmed.fetch_add(1, Ordering::AcqRel);
    }
}

struct Reads {
    locked: parking_lot::Mutex<Option<bool>>,
    list: ReadGate,
    receiver: ReadGate,
    keys: ReadGate,
    claims: ReadGate,
}

impl Default for Reads {
    fn default() -> Self {
        Self {
            locked: Default::default(),
            list: Default::default(),
            receiver: Default::default(),
            keys: Default::default(),
            claims: ReadGate {
                session_cached: true,
                ..Default::default()
            },
        }
    }
}

impl ServiceContext {
    pub(super) async fn neighbor_access_locked(&self) -> bool {
        let Some(billing) = self.ext::<crate::service::billing::BillingCell>().0.get() else {
            return false;
        };
        let Some(account) = self.ext::<crate::service::account::AccountCell>().0.get() else {
            return false;
        };
        let Some(session) = account.session().await else {
            return false;
        };
        billing.neighbors_locked(&session.identity.uid)
    }

    pub(crate) fn neighbor_entitlements_changed(&self, changed: bool) {
        let service = self.clone();
        crate::async_runtime::spawn(async move {
            let locked = service.neighbor_access_locked().await;
            {
                let mut previous = service.extensions.get_or_init(Reads::default).locked.lock();
                if !changed && *previous == Some(locked) {
                    return;
                }
                *previous = Some(locked);
            }
            if locked {
                service.stop_neighbor_updates();
                service.pause_neighbor_reception();
                if let Some(account) = service
                    .ext::<crate::service::account::AccountCell>()
                    .0
                    .get()
                {
                    if let Some(session) = account.session().await {
                        if service
                            .ext::<crate::service::billing::BillingCell>()
                            .0
                            .get()
                            .is_some_and(|billing| billing.neighbors_locked(&session.identity.uid))
                        {
                            service.ui.emit("neighbors-changed", serde_json::json!({"uid":session.identity.uid,"list":locked_list()}));
                        }
                    }
                }
            } else {
                service.rearm_neighbor_reads();
            }
        });
    }

    pub(super) async fn neighbor_backend_read(
        &self,
        route: &str,
        uid: &str,
        generation: u64,
        load: impl Future<Output = Result<Value>>,
    ) -> Result<Value> {
        let locked = self.neighbor_access_locked().await;
        let reads = self.extensions.get_or_init(Reads::default);
        let gate = match route {
            "friendsList" => &reads.list,
            "friendsReceiverStatus" => &reads.receiver,
            "friendsOwnKeys" => &reads.keys,
            "claimReferralTrial" => &reads.claims,
            _ => {
                return if locked {
                    Err(Error::Locked)
                } else {
                    load.await
                }
            }
        };
        gate.read(
            (uid.to_owned(), generation),
            locked,
            Instant::now,
            rand::random,
            load,
        )
        .await
    }

    pub(crate) fn rearm_neighbor_reads(&self) {
        let reads = self.extensions.get_or_init(Reads::default);
        reads.list.rearm();
        reads.receiver.rearm();
        reads.keys.rearm();
        self.rearm_neighbor_updates();
        self.rearm_neighbor_restoration();
    }
}

pub(super) fn locked_list() -> Value {
    serde_json::json!({"schema_version":1,"access":"locked","can_invite":false,"friends":[],"incoming":[],"outgoing":[]})
}

#[cfg(test)]
mod tests;
