use crate::service::ServiceContext;
use codeterm_neighbors::{Error, Result};
use serde_json::Value;
use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct State {
    key: Option<(String, u64)>,
    result: Option<Result<Value>>,
    rearmed: u64,
}

#[derive(Default)]
struct ReadGate {
    state: tokio::sync::Mutex<State>,
    rearmed: AtomicU64,
}

impl ReadGate {
    async fn read(
        &self,
        key: (String, u64),
        locked: bool,
        load: impl Future<Output = Result<Value>>,
    ) -> Result<Value> {
        if locked {
            return Err(Error::Locked);
        }
        let mut state = self.state.lock().await;
        let rearmed = self.rearmed.load(Ordering::Acquire);
        if state.key.as_ref() != Some(&key) {
            *state = State {
                key: Some(key),
                ..State::default()
            };
        } else if let Some(result) = &state.result {
            if rearmed == state.rearmed { return result.clone(); }
        }
        state.rearmed = rearmed;
        let result = load.await;
        state.result = Some(result.clone());
        result
    }

    fn rearm(&self) {
        self.rearmed.fetch_add(1, Ordering::AcqRel);
    }
}

#[derive(Default)]
struct Reads {
    locked: parking_lot::Mutex<Option<bool>>,
    list: ReadGate,
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
        if let Some(value) = self.neighbor_projection_read(uid, generation, route).await? { return Ok(value); }
        if matches!(route, "friendsOwnKeys" | "friendsReceiverStatus") { return Err(Error::Transport); }
        let locked = self.neighbor_access_locked().await && route != "friendsList";
        let reads = self.extensions.get_or_init(Reads::default);
        let gate = match route {
            "friendsList" => &reads.list,
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
            load,
        )
        .await
    }

    pub(in crate::service::neighbors) fn invalidate_neighbor_list(&self) {
        self.extensions.get_or_init(Reads::default).list.rearm();
    }

    pub(crate) fn rearm_neighbor_reads(&self) {
        let reads = self.extensions.get_or_init(Reads::default);
        reads.list.rearm();
        self.rearm_neighbor_updates();
        self.rearm_neighbor_restoration();
    }
}

pub(super) fn locked_list() -> Value {
    serde_json::json!({"schema_version":1,"access":"locked","can_invite":false,"friends":[],"incoming":[],"outgoing":[]})
}

#[cfg(test)]
mod tests;
