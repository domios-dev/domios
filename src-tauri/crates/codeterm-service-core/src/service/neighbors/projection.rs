use crate::service::ServiceContext;
use codeterm_neighbors::{
    state::{AccountSnapshot, Snapshot},
    Error, Result,
};
use serde_json::Value;

#[derive(Default)]
struct State {
    snapshots: AccountSnapshot,
    unavailable: bool,
    selected: Option<(String, u64)>,
    fallback_started: bool,
    reconciled: Option<u64>,
    activated: Option<u64>,
    queued_version: Option<u64>,
    authoritative: bool,
    list_dirty: bool,
}
#[derive(Default)]
struct Cell {
    state: parking_lot::Mutex<State>,
    changed: tokio::sync::Notify,
}

#[derive(Clone)]
pub(super) struct ActivationTicket {
    uid: String,
    generation: u64,
    version: u64,
    epoch: super::backend_reads::ListEpoch,
}

impl ServiceContext {
    fn queue_neighbor_projection_version(&self, uid: &str, generation: u64, version: u64) -> bool {
        let mut state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.selected.as_ref() != Some(&(uid.to_owned(), generation))
            || state.queued_version.is_some_and(|queued| version < queued) {
            return false;
        }
        state.queued_version = Some(version);
        true
    }
    async fn require_neighbor_reconciliation_account(&self, uid: &str, generation: u64) -> Result<()> {
        self.require_neighbors_enabled()?;
        if let Some(account) = self.ext::<crate::service::account::AccountCell>().0.get() {
            let session = account.session().await.ok_or(Error::Revoked)?;
            if session.identity.uid != uid || session.login_generation != generation { return Err(Error::Revoked); }
        }
        Ok(())
    }

    pub(super) async fn require_neighbor_activation_account(&self, ticket: &ActivationTicket) -> Result<()> {
        self.require_neighbor_reconciliation_account(&ticket.uid, ticket.generation).await
    }

    pub(super) async fn pause_neighbor_projection_reception(&self, uid: &str, generation: u64, version: u64) -> Result<()> {
        let (ticket, list) = self.neighbor_activation_ticket(uid, generation)?;
        if list["version"] != version { return Err(Error::Revoked); }
        let service = self.clone();
        self.neighbor_reconciliation().submit(super::reconciliation::Kind::Activation, move || async move {
            service.require_neighbor_activation_account(&ticket).await?;
            service.require_neighbor_activation_ticket(&ticket)?;
            service.pause_neighbor_reception();
            Ok(())
        })?.wait().await
    }
    pub(super) fn neighbor_projection_activated(&self, uid: &str, generation: u64, version: u64) -> bool {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        state.selected.as_ref() == Some(&(uid.to_owned(), generation)) && state.activated == Some(version)
    }

    pub(super) fn remember_neighbor_projection_activation(&self, uid: &str, generation: u64, version: u64) -> Result<()> {
        let mut state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.list_dirty || state.selected.as_ref() != Some(&(uid.to_owned(), generation))
            || state.snapshots.version(uid, generation) != Some(version) {
            return Err(Error::Revoked);
        }
        state.activated = Some(version);
        Ok(())
    }
    pub(super) fn neighbor_activation_ticket(&self, uid: &str, generation: u64) -> Result<(ActivationTicket, Value)> {
        self.neighbor_reconciliation().require_readable()?;
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if !state.authoritative || state.list_dirty || state.selected.as_ref() != Some(&(uid.to_owned(), generation)) {
            return Err(Error::Revoked);
        }
        let version = state.snapshots.version(uid, generation).ok_or(Error::Revoked)?;
        if state.queued_version.is_some_and(|queued| queued > version) { return Err(Error::Revoked); }
        let list = state.snapshots.read(uid, generation, "friendsList", "", now_seconds()).ok_or(Error::Revoked)?;
        Ok((ActivationTicket { uid: uid.to_owned(), generation, version, epoch: self.neighbor_list_epoch(uid, generation) }, list))
    }

    pub(super) fn require_neighbor_activation_ticket(&self, ticket: &ActivationTicket) -> Result<()> {
        self.require_neighbors_enabled()?;
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if !state.authoritative || state.list_dirty
            || state.selected.as_ref() != Some(&(ticket.uid.clone(), ticket.generation))
            || state.snapshots.version(&ticket.uid, ticket.generation) != Some(ticket.version)
            || state.queued_version.is_some_and(|queued| queued > ticket.version)
            || !self.neighbor_list_epoch_current(&ticket.epoch)
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }
    pub(super) async fn select_neighbor_projection(&self, uid: &str, generation: u64) -> Result<()> {
        let service = self.clone();
        let uid = uid.to_owned();
        self.neighbor_reconciliation().submit(super::reconciliation::Kind::Selection, move || async move {
            service.require_neighbor_reconciliation_account(&uid, generation).await?;
            service.finish_neighbor_identity_change();
            service.select_neighbor_projection_owned(&uid, generation);
            Ok(())
        })?.wait().await
    }

    fn select_neighbor_projection_owned(&self, uid: &str, generation: u64) {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut state = cell.state.lock();
        if state.selected.as_ref() != Some(&(uid.to_owned(), generation)) {
            self.forget_neighbor_registry();
            *state = State {
                selected: Some((uid.to_owned(), generation)),
                ..State::default()
            };
        }
        state.snapshots.select(uid, generation);
        self.neighbor_list_epoch(uid, generation);
        drop(state);
        cell.changed.notify_waiters();
    }
    pub(super) fn invalidate_neighbor_projection_list(&self) {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut state = cell.state.lock();
        state.list_dirty = true;
        state.activated = None;
        state.fallback_started = false;
        self.retire_neighbor_list_reads();
        self.forget_neighbor_registry();
        drop(state);
        cell.changed.notify_waiters();
    }

    pub(super) fn publish_neighbor_fallback(
        &self,
        uid: &str,
        generation: u64,
        epoch: &super::backend_reads::ListEpoch,
        list: Value,
    ) -> bool {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.selected.as_ref() != Some(&(uid.to_owned(), generation))
            || (state.authoritative && !state.list_dirty)
            || !self.neighbor_list_epoch_current(epoch)
        {
            return false;
        }
        self.ui.emit("neighbors-changed", serde_json::json!({"uid":uid,"list":list}));
        true
    }

    pub(super) fn remember_neighbor_list_read(
        &self,
        uid: &str,
        generation: u64,
        epoch: &super::backend_reads::ListEpoch,
        list: &Value,
    ) -> Result<()> {
        self.neighbor_reconciliation().require_readable()?;
        self.commit_neighbor_list_read(uid, generation, epoch, || {
            self.remember_neighbor_names(list);
            self.remember_neighbor_registry(list);
        })
    }

    fn commit_neighbor_list_read<T>(
        &self,
        uid: &str,
        generation: u64,
        epoch: &super::backend_reads::ListEpoch,
        commit: impl FnOnce() -> T,
    ) -> Result<T> {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.selected.as_ref().is_some_and(|selected| selected != &(uid.to_owned(), generation)) {
            return Err(Error::Revoked);
        }
        self.commit_neighbor_list_epoch(epoch, commit)
    }

    pub(super) fn remember_neighbor_projection_registry(
        &self,
        uid: &str,
        generation: u64,
        list: &Value,
    ) -> Result<()> {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.selected.as_ref() != Some(&(uid.to_owned(), generation))
            || !state.authoritative || state.list_dirty
            || state.snapshots.version(uid, generation) != list["version"].as_u64()
        {
            return Err(Error::Revoked);
        }
        self.remember_neighbor_names(list);
        self.remember_neighbor_registry(list);
        Ok(())
    }

    pub(super) fn require_neighbor_projection_account(&self, uid: &str, generation: u64) -> Result<()> {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.selected.as_ref().is_some_and(|selected| selected != &(uid.to_owned(), generation)) {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    pub(super) fn neighbor_projection_read_epoch(
        &self,
        uid: &str,
        generation: u64,
    ) -> Result<super::backend_reads::ListEpoch> {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.selected.as_ref().is_some_and(|selected| selected != &(uid.to_owned(), generation)) {
            return Err(Error::Revoked);
        }
        Ok(self.neighbor_list_epoch(uid, generation))
    }

    pub(super) fn publish_neighbor_projection_list(
        &self,
        uid: &str,
        generation: u64,
        list: Value,
    ) -> bool {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if !state.authoritative || state.list_dirty
            || state.snapshots.version(uid, generation) != list["version"].as_u64()
            || state.selected.as_ref() != Some(&(uid.to_owned(), generation))
        {
            return false;
        }
        self.ui.emit("neighbors-changed", serde_json::json!({"uid":uid,"list":list}));
        true
    }
    pub(super) fn begin_neighbor_fallback(&self) -> bool {
        let mut state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.fallback_started {
            return false;
        }
        state.fallback_started = true;
        true
    }
    pub(super) fn reset_neighbor_projection_owned(&self) {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut state = cell.state.lock();
        *state = State::default();
        self.retire_neighbor_list_reads();
        self.forget_neighbor_registry();
        drop(state);
        cell.changed.notify_waiters();
    }
    pub(super) fn neighbor_projection_unavailable(&self) {
        let cell = self.extensions.get_or_init(Cell::default);
        cell.state.lock().unavailable = true;
        cell.changed.notify_waiters();
    }
    pub(super) async fn neighbor_projection_missing(&self) -> Result<()> {
        let service = self.clone();
        self.neighbor_reconciliation().submit(super::reconciliation::Kind::Retirement, move || async move {
            service.neighbor_projection_missing_owned();
            Ok(())
        })?.wait().await
    }

    fn neighbor_projection_missing_owned(&self) {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut state = cell.state.lock();
        state.unavailable = true;
        state.reconciled = None;
        state.authoritative = false;
        drop(state);
        cell.changed.notify_waiters();
    }
    #[cfg(test)]
    pub(super) async fn apply_neighbor_projection(
        &self,
        uid: &str,
        generation: u64,
        snapshot: Snapshot,
    ) -> bool {
        if !self.queue_neighbor_projection_version(uid, generation, snapshot.version) { return false; }
        let service = self.clone();
        let uid = uid.to_owned();
        match self.neighbor_reconciliation().submit(super::reconciliation::Kind::Projection, move || async move {
            Ok(service.apply_neighbor_projection_owned(&uid, generation, snapshot))
        }) {
            Ok(receipt) => receipt.wait().await.unwrap_or(false),
            Err(_) => false,
        }
    }

    fn apply_neighbor_projection_owned(
        &self,
        uid: &str,
        generation: u64,
        snapshot: Snapshot,
    ) -> bool {
        let cell = self.extensions.get_or_init(Cell::default);
        let applied = {
            let mut state = cell.state.lock();
            let applied = state.snapshots.apply(uid, generation, snapshot);
            if applied {
                state.unavailable = false;
                state.authoritative = true;
                state.list_dirty = false;
            }
            applied
        };
        if applied {
            cell.changed.notify_waiters();
        }
        applied
    }
    pub(super) async fn consume_neighbor_projection<F, R>(
        &self,
        uid: &str,
        generation: u64,
        snapshot: Snapshot,
        reconcile: F,
    ) -> Result<bool>
    where
        F: FnOnce(Value) -> R + Send + 'static,
        R: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        if !self.queue_neighbor_projection_version(uid, generation, snapshot.version) { return Ok(false); }
        let service = self.clone();
        let uid = uid.to_owned();
        self.neighbor_reconciliation().submit(super::reconciliation::Kind::Projection, move || async move {
            service.consume_neighbor_projection_owned(&uid, generation, snapshot, reconcile).await
        })?.wait().await
    }

    async fn consume_neighbor_projection_owned<F, R>(
        &self,
        uid: &str,
        generation: u64,
        snapshot: Snapshot,
        reconcile: F,
    ) -> Result<bool>
    where
        F: FnOnce(Value) -> R,
        R: std::future::Future<Output = Result<()>>,
    {
        self.require_neighbor_reconciliation_account(uid, generation).await?;
        let version = snapshot.version;
        let mut list = snapshot.list(now_seconds());
        if !self.apply_neighbor_projection_owned(uid, generation, snapshot) {
            let state = self.extensions.get_or_init(Cell::default).state.lock();
            if !state.authoritative
                || state.list_dirty
                || state.snapshots.version(uid, generation) != Some(version)
                || state.reconciled == Some(version)
            {
                return Ok(false);
            }
            list = state
                .snapshots
                .read(uid, generation, "friendsList", "", now_seconds())
                .ok_or(Error::Revoked)?;
        }
        reconcile(list).await?;
        let mut state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.selected.as_ref() == Some(&(uid.to_owned(), generation))
            && state.snapshots.version(uid, generation) == Some(version)
            && !state.list_dirty
        {
            state.reconciled = Some(version);
        }
        Ok(true)
    }
    pub(super) fn peek_neighbor_projection(
        &self,
        uid: &str,
        generation: u64,
        route: &str,
        installation: &str,
    ) -> Option<Value> {
        let state = self.extensions.get_or_init(Cell::default).state.lock();
        if !state.authoritative || (route == "friendsList" && state.list_dirty) {
            return None;
        }
        state
            .snapshots
            .read(uid, generation, route, installation, now_seconds())
    }
    pub(super) fn neighbor_projection_version(&self, uid: &str, generation: u64) -> Option<u64> {
        self.extensions
            .get_or_init(Cell::default)
            .state
            .lock()
            .snapshots
            .version(uid, generation)
    }
    pub(super) async fn neighbor_projection_read(
        &self,
        uid: &str,
        generation: u64,
        route: &str,
    ) -> Result<Option<Value>> {
        if !matches!(
            route,
            "friendsList" | "friendsOwnKeys" | "friendsReceiverStatus"
        ) {
            return Ok(None);
        }
        self.require_neighbors_enabled()?;
        self.restore_neighbor_updates_once();
        let installation = self
            .installation_id()
            .await
            .map_err(|_| Error::Transport)?
            .as_str()
            .to_owned();
        if let Some(value) = self.peek_neighbor_projection(uid, generation, route, &installation) {
            return Ok(Some(value));
        }
        if self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .is_none()
        {
            return Ok(None);
        }
        let cell = self.extensions.get_or_init(Cell::default);
        let read = async {
            loop {
                let changed = cell.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                {
                    let state = cell.state.lock();
                    if state.selected.as_ref().is_some_and(|selected| selected != &(uid.to_owned(), generation)) {
                        return None;
                    }
                    if state.authoritative && !(route == "friendsList" && state.list_dirty) {
                        if let Some(value) = state.snapshots.read(
                            uid,
                            generation,
                            route,
                            &installation,
                            now_seconds(),
                        ) {
                            return Some(value);
                        }
                    }
                    if state.unavailable || (route == "friendsList" && state.list_dirty) {
                        return None;
                    }
                }
                changed.await;
                if self.require_neighbors_enabled().is_err() {
                    return None;
                }
            }
        };
        Ok(
            tokio::time::timeout(std::time::Duration::from_secs(15), read)
                .await
                .unwrap_or(None),
        )
    }
    pub(super) async fn wait_neighbor_projection_receiver(
        &self,
        uid: &str,
        login_generation: u64,
        friendship: &str,
        revision: u64,
    ) -> Result<()> {
        let cell = self.extensions.get_or_init(Cell::default);
        let installation = self
            .installation_id()
            .await
            .map_err(|_| Error::Transport)?
            .as_str()
            .to_owned();
        let wait = async {
            loop {
                let changed = cell.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                {
                    let state = cell.state.lock();
                    if !state.authoritative
                        || state.unavailable
                        || state.snapshots.version(uid, login_generation).is_none()
                    {
                        return Ok(());
                    }
                    let list = state
                        .snapshots
                        .read(
                            uid,
                            login_generation,
                            "friendsList",
                            &installation,
                            now_seconds(),
                        )
                        .ok_or(Error::Revoked)?;
                    if list["friends"].as_array().is_some_and(|rows| {
                        rows.iter().any(|row| {
                            row["friendship_id"] == friendship
                                && row["my_receiver"]["revision"]
                                    .as_u64()
                                    .is_some_and(|current| current >= revision)
                        })
                    }) {
                        return Ok(());
                    }
                }
                changed.await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(15), wait)
            .await
            .map_err(|_| Error::ReceiverRevisionStale)?
    }
}

#[cfg(test)]
mod tests;

pub(super) fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
