use crate::service::ServiceContext;
use codeterm_neighbors::{Error, Result};
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;

#[derive(Clone, Default)]
pub(super) struct ListEpoch(Arc<tokio::sync::Mutex<()>>);

impl ListEpoch {
    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Default)]
struct State {
    key: Option<(String, u64)>,
    result: Option<Result<Value>>,
    epoch: ListEpoch,
}

#[derive(Default)]
struct ReadGate {
    state: parking_lot::Mutex<State>,
}

impl ReadGate {
    fn select(&self, key: (String, u64)) -> ListEpoch {
        let mut state = self.state.lock();
        if state.key.as_ref() != Some(&key) {
            *state = State {
                key: Some(key),
                ..State::default()
            };
        }
        state.epoch.clone()
    }

    fn invalidate(&self) {
        let mut state = self.state.lock();
        state.result = None;
        state.epoch = ListEpoch::default();
    }

    #[cfg(test)]
    async fn read(
        &self,
        key: (String, u64),
        locked: bool,
        load: impl Future<Output = Result<Value>>,
    ) -> Result<Value> {
        self.read_epoch(self.select(key), locked, load).await
    }

    async fn read_epoch(
        &self,
        epoch: ListEpoch,
        locked: bool,
        load: impl Future<Output = Result<Value>>,
    ) -> Result<Value> {
        if locked {
            return Err(Error::Locked);
        }
        let _load = epoch.0.lock().await;
        {
            let state = self.state.lock();
            if !state.epoch.matches(&epoch) {
                return Err(Error::Revoked);
            }
            if let Some(result) = &state.result {
                return result.clone();
            }
        }
        let result = load.await;
        let mut state = self.state.lock();
        if !state.epoch.matches(&epoch) {
            return Err(Error::Revoked);
        }
        state.result = Some(result.clone());
        result
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
        self.neighbor_reconciliation().require_readable()?;
        self.require_neighbor_projection_account(uid, generation)?;
        let epoch = if route == "friendsList" {
            Some(self.neighbor_projection_read_epoch(uid, generation)?)
        } else {
            None
        };
        if let Some(value) = self
            .neighbor_projection_read(uid, generation, route)
            .await?
        {
            return Ok(value);
        }
        if matches!(route, "friendsOwnKeys" | "friendsReceiverStatus") {
            return Err(Error::Transport);
        }
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
        let result = match epoch {
            Some(epoch) => gate.read_epoch(epoch, locked, load).await,
            None => return Err(Error::Transport),
        };
        self.require_neighbor_projection_account(uid, generation)?;
        self.neighbor_reconciliation().require_readable()?;
        if let Some(value) = self.peek_neighbor_projection(uid, generation, route, "") {
            return Ok(value);
        }
        result
    }

    pub(in crate::service::neighbors) async fn invalidate_neighbor_list(&self) -> Result<()> {
        let service = self.clone();
        self.neighbor_reconciliation()
            .submit(
                super::reconciliation::Kind::Retirement,
                move || async move {
                    service.invalidate_neighbor_projection_list();
                    Ok(())
                },
            )?
            .wait()
            .await
    }

    pub(super) fn commit_neighbor_list_epoch<T>(
        &self,
        epoch: &ListEpoch,
        commit: impl FnOnce() -> T,
    ) -> Result<T> {
        let state = self
            .extensions
            .get_or_init(Reads::default)
            .list
            .state
            .lock();
        if !state.epoch.matches(epoch) {
            return Err(Error::Revoked);
        }
        Ok(commit())
    }

    pub(super) fn retire_neighbor_list_reads(&self) {
        self.extensions
            .get_or_init(Reads::default)
            .list
            .invalidate();
    }

    pub(super) fn neighbor_list_epoch(&self, uid: &str, generation: u64) -> ListEpoch {
        self.extensions
            .get_or_init(Reads::default)
            .list
            .select((uid.to_owned(), generation))
    }

    pub(super) fn neighbor_list_epoch_current(&self, epoch: &ListEpoch) -> bool {
        self.extensions
            .get_or_init(Reads::default)
            .list
            .state
            .lock()
            .epoch
            .matches(epoch)
    }

    pub(super) async fn refresh_neighbor_list(&self) -> Result<()> {
        self.invalidate_neighbor_list().await?;
        self.rearm_neighbor_reads();
        Ok(())
    }

    pub(crate) fn rearm_neighbor_reads(&self) {
        self.rearm_neighbor_updates();
        self.rearm_neighbor_restoration();
    }
}

pub(super) fn locked_list() -> Value {
    serde_json::json!({"schema_version":1,"access":"locked","can_invite":false,"friends":[],"incoming":[],"outgoing":[]})
}

#[cfg(test)]
mod tests;
