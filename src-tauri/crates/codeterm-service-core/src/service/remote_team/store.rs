use super::{IncomingTransfer, OutgoingTransfer, RemoteTeamFinalization, RemoteTeamMember};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemoteTeamData {
    pub members: BTreeMap<String, RemoteTeamMember>,
    pub outgoing: BTreeMap<String, OutgoingTransfer>,
    pub incoming: BTreeMap<String, IncomingTransfer>,
    pub latest_incoming: BTreeMap<String, String>,
    pub closing_teams: BTreeSet<String>,
    #[serde(default)]
    pub finalizations: BTreeMap<String, RemoteTeamFinalization>,
}

struct Published {
    data: Result<Arc<RemoteTeamData>, String>,
    pending: Option<Arc<RemoteTeamData>>,
}

pub struct RemoteTeamStore {
    state: Mutex<Published>,
    writer: Mutex<()>,
    path: Option<PathBuf>,
    #[cfg(test)]
    fail_next_write: std::sync::atomic::AtomicBool,
}

impl Default for RemoteTeamStore {
    fn default() -> Self {
        Self {
            state: Mutex::new(Published {
                data: Ok(Arc::new(RemoteTeamData::default())),
                pending: None,
            }),
            writer: Mutex::new(()),
            path: None,
            #[cfg(test)]
            fail_next_write: Default::default(),
        }
    }
}

impl RemoteTeamStore {
    pub fn open(path: PathBuf) -> Self {
        let state = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("remote Team store is unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(RemoteTeamData::default())
            }
            Err(error) => Err(format!("remote Team store is unavailable: {error}")),
        };
        if let Err(error) = &state {
            log::error!("{error}");
        }
        Self {
            state: Mutex::new(Published {
                data: state.map(Arc::new),
                pending: None,
            }),
            writer: Mutex::new(()),
            path: Some(path),
            #[cfg(test)]
            fail_next_write: Default::default(),
        }
    }

    fn read(&self) -> Result<Arc<RemoteTeamData>, String> {
        self.state.lock().data.clone()
    }

    pub fn snapshot(&self) -> Result<RemoteTeamData, String> {
        Ok(self.read()?.as_ref().clone())
    }

    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut RemoteTeamData) -> Result<T, String>,
    ) -> Result<T, String> {
        self.update_with(change, |next| self.persist(next))
    }

    fn update_with<T>(
        &self,
        change: impl FnOnce(&mut RemoteTeamData) -> Result<T, String>,
        persist: impl FnOnce(&RemoteTeamData) -> Result<(), String>,
    ) -> Result<T, String> {
        let _writer = self.writer.lock();
        let mut next = self.read()?.as_ref().clone();
        let result = change(&mut next)?;
        let next = Arc::new(next);
        self.state.lock().pending = Some(next.clone());
        let persisted = persist(&next);
        let (retired, pending) = {
            let mut state = self.state.lock();
            let pending = state.pending.take();
            let retired = if persisted.is_ok() {
                Some(std::mem::replace(&mut state.data, Ok(next)))
            } else {
                None
            };
            (retired, pending)
        };
        drop((retired, pending));
        persisted?;
        Ok(result)
    }

    fn persist(&self, next: &RemoteTeamData) -> Result<(), String> {
        #[cfg(test)]
        if self
            .fail_next_write
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err("injected remote Team persistence failure".into());
        }
        if let Some(path) = &self.path {
            let bytes = serde_json::to_vec(next).map_err(|error| error.to_string())?;
            codeterm_util::atomic_write::write_atomic(path, bytes)
                .map_err(|error| format!("remote Team persist failed: {error}"))?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn fail_next_persist(&self) {
        self.fail_next_write
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn members_for_team(&self, team: &str) -> Result<Vec<RemoteTeamMember>, String> {
        Ok(self
            .members()?
            .into_iter()
            .filter(|member| member.team_id == team)
            .collect())
    }

    pub fn members(&self) -> Result<Vec<RemoteTeamMember>, String> {
        let guard = self.read()?;
        Ok(guard.members.values().cloned().collect())
    }

    pub fn was_finalized(&self, peer: &str, tab: &str, provider: &str, session: &str) -> bool {
        self.read().ok().is_some_and(|data| {
            data.finalizations
                .get(&super::member_key(peer, tab))
                .is_some_and(|receipt| {
                    receipt.session_id == session && receipt.provider == provider
                })
        })
    }

    pub fn ensure_tab_lifecycle_available(&self, tab_id: &str) -> Result<(), String> {
        let (committed, pending) = {
            let state = self.state.lock();
            (state.data.clone()?, state.pending.clone())
        };
        if std::iter::once(committed).chain(pending).any(|data| {
            data.outgoing.values().any(|operation| {
                !operation.completed
                    && (operation.member.orchestrator_tab_id == tab_id
                        || operation.actor_tab_id.as_deref() == Some(tab_id))
            })
        }) {
            return Err("tab has an unresolved remote Worker transfer; recover that operation before changing its lifecycle".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
