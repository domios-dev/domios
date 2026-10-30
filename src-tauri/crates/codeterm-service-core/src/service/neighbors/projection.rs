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
    authoritative: bool,
}
#[derive(Default)]
struct Cell {
    state: parking_lot::Mutex<State>,
    changed: tokio::sync::Notify,
}

impl ServiceContext {
    pub(super) fn select_neighbor_projection(&self, uid: &str, generation: u64) {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut state = cell.state.lock();
        if state.selected.as_ref() != Some(&(uid.to_owned(), generation)) {
            *state = State {
                selected: Some((uid.to_owned(), generation)),
                ..State::default()
            };
        }
        state.snapshots.select(uid, generation);
    }
    pub(super) fn begin_neighbor_fallback(&self) -> bool {
        let mut state = self.extensions.get_or_init(Cell::default).state.lock();
        if state.fallback_started {
            return false;
        }
        state.fallback_started = true;
        true
    }
    pub(super) fn reset_neighbor_projection(&self) {
        let cell = self.extensions.get_or_init(Cell::default);
        *cell.state.lock() = State::default();
        cell.changed.notify_waiters();
    }
    pub(super) fn neighbor_projection_unavailable(&self) {
        let cell = self.extensions.get_or_init(Cell::default);
        cell.state.lock().unavailable = true;
        cell.changed.notify_waiters();
    }
    pub(super) fn neighbor_projection_missing(&self) {
        let cell = self.extensions.get_or_init(Cell::default);
        let mut state = cell.state.lock();
        state.unavailable = true;
        state.reconciled = None;
        state.authoritative = false;
        drop(state);
        cell.changed.notify_waiters();
    }
    pub(super) fn apply_neighbor_projection(
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
        F: FnOnce(Value) -> R,
        R: std::future::Future<Output = Result<()>>,
    {
        let version = snapshot.version;
        let mut list = snapshot.list(now_seconds());
        if !self.apply_neighbor_projection(uid, generation, snapshot) {
            let state = self.extensions.get_or_init(Cell::default).state.lock();
            if !state.authoritative
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
        if state.selected.as_ref() == Some(&(uid.to_owned(), generation)) {
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
        if !state.authoritative {
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
                    if state.authoritative {
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
                    if state.unavailable {
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
