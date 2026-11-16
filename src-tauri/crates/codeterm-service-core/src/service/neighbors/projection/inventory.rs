use super::*;
use std::{future::Future, sync::Arc};

type Attempt = Arc<tokio::sync::Mutex<Option<Result<()>>>>;

struct RecoveryAttempt {
    uid: String,
    generation: u64,
    epoch: Arc<()>,
    result: Attempt,
}

#[derive(Default)]
struct Recovery(parking_lot::Mutex<Option<RecoveryAttempt>>);

#[cfg(test)]
pub(in crate::service::neighbors) struct InventoryLoader(
    pub Arc<dyn Fn() -> Result<Snapshot> + Send + Sync>,
);

impl ServiceContext {
    pub(in crate::service::neighbors) async fn recover_neighbor_key_inventory(
        &self,
        uid: &str,
        generation: u64,
    ) -> Result<()> {
        self.recover_neighbor_inventory_with(uid, generation, async {
            #[cfg(test)]
            if let Some(loader) = self.try_ext::<InventoryLoader>() {
                return (loader.0)();
            }
            let installation = self.installation_id().await.map_err(|_| Error::Transport)?;
            let calls =
                super::super::runtime::transport::calls(super::super::runtime::callable::Calls {
                    service: self.clone(),
                    uid: uid.to_owned(),
                    generation,
                })?;
            let transport = super::super::runtime::transport::new(
                calls,
                uid.to_owned(),
                installation.as_str().to_owned(),
            )?;
            let mut listener = transport.listen_state().await?;
            match listener.next().await? {
                Some(codeterm_neighbors::state::StateEvent::Snapshot(snapshot)) => Ok(snapshot),
                _ => Err(Error::KeyInventoryPending),
            }
        })
        .await
    }

    async fn recover_neighbor_inventory_with(
        &self,
        uid: &str,
        generation: u64,
        load: impl Future<Output = Result<Snapshot>>,
    ) -> Result<()> {
        self.require_neighbor_reconciliation_account(uid, generation)
            .await?;
        self.neighbor_reconciliation().require_readable()?;
        let epoch = {
            let state = self.extensions.get_or_init(Cell::default).state.lock();
            if state.selected.as_ref() != Some(&(uid.to_owned(), generation)) {
                return Err(Error::KeyInventoryPending);
            }
            if state.key_inventory_pending.is_empty() {
                return Ok(());
            }
            state.inventory_epoch.clone()
        };
        let attempt = {
            let mut recovery = self.extensions.get_or_init(Recovery::default).0.lock();
            if recovery.as_ref().is_none_or(|previous| {
                previous.uid != uid
                    || previous.generation != generation
                    || !Arc::ptr_eq(&previous.epoch, &epoch)
            }) {
                *recovery = Some(RecoveryAttempt {
                    uid: uid.to_owned(),
                    generation,
                    epoch: epoch.clone(),
                    result: Attempt::default(),
                });
            }
            recovery
                .as_ref()
                .ok_or(Error::KeyInventoryPending)?
                .result
                .clone()
        };
        let mut result = attempt.lock().await;
        self.require_neighbor_reconciliation_account(uid, generation)
            .await?;
        {
            let state = self.extensions.get_or_init(Cell::default).state.lock();
            if state.selected.as_ref() != Some(&(uid.to_owned(), generation))
                || !Arc::ptr_eq(&state.inventory_epoch, &epoch)
            {
                return Err(Error::KeyInventoryPending);
            }
        }
        if let Some(result) = result.as_ref() {
            return *result;
        }
        *result = Some(Err(Error::KeyInventoryPending));
        let reconcile = async {
            let snapshot = load.await?;
            self.require_neighbor_reconciliation_account(uid, generation)
                .await?;
            let service = self.clone();
            let uid_owned = uid.to_owned();
            self.neighbor_reconciliation()
                .submit(
                    super::super::reconciliation::Kind::Projection,
                    move || async move {
                        service
                            .require_neighbor_reconciliation_account(&uid_owned, generation)
                            .await?;
                        let mut state = service.extensions.get_or_init(Cell::default).state.lock();
                        if state.selected.as_ref() != Some(&(uid_owned.clone(), generation))
                            || !Arc::ptr_eq(&state.inventory_epoch, &epoch)
                            || state
                                .queued_version
                                .is_some_and(|queued| snapshot.version < queued)
                        {
                            return Err(Error::KeyInventoryPending);
                        }
                        state.queued_version = Some(snapshot.version);
                        drop(state);
                        service.apply_neighbor_projection_owned(&uid_owned, generation, snapshot);
                        Ok(())
                    },
                )?
                .wait()
                .await?;
            self.require_neighbor_reconciliation_account(uid, generation)
                .await?;
            self.require_neighbor_projection_account(uid, generation)?;
            if self.neighbor_key_inventory_pending(uid, generation) {
                return Err(Error::KeyInventoryPending);
            }
            Ok(())
        };
        let confirmed = tokio::time::timeout(std::time::Duration::from_secs(15), reconcile)
            .await
            .unwrap_or(Err(Error::KeyInventoryPending));
        *result = Some(confirmed);
        confirmed
    }

    pub(in crate::service::neighbors) fn remember_neighbor_watch_failure(
        &self,
        failure: codeterm_neighbors::responses::ListenerFailure,
    ) {
        let mut state = self.extensions.get_or_init(Cell::default).state.lock();
        state.last_watch_failure = Some(failure);
    }

    pub(in crate::service::neighbors) async fn neighbor_projection_diagnostics(&self) -> Value {
        let session = match self.ext::<crate::service::account::AccountCell>().0.get() {
            Some(account) => account.session().await,
            None => None,
        };
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        let version = state
            .selected
            .as_ref()
            .and_then(|(uid, generation)| state.snapshots.version(uid, *generation));
        serde_json::json!({
            "pending_verifier_ids": state.key_inventory_pending,
            "snapshot_version": version,
            "available": !state.unavailable,
            "authoritative": state.authoritative,
            "dirty": state.list_dirty,
            "selected_generation": state.selected.as_ref().map(|(_, generation)| generation),
            "session_generation": session.as_ref().map(|session| session.login_generation),
            "session_matches": session.as_ref().is_some_and(|session| {
                state.selected.as_ref() == Some(&(session.identity.uid.clone(), session.login_generation))
            }),
            "last_watch_failure": state.last_watch_failure,
        })
    }
}

#[cfg(test)]
mod tests;
