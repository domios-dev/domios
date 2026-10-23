use super::{IncomingTransfer, OutgoingTransfer, RemoteTeamMember};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemoteTeamData {
    pub members: BTreeMap<String, RemoteTeamMember>,
    pub outgoing: BTreeMap<String, OutgoingTransfer>,
    pub incoming: BTreeMap<String, IncomingTransfer>,
    pub latest_incoming: BTreeMap<String, String>,
    pub closing_teams: BTreeSet<String>,
}

pub struct RemoteTeamStore {
    state: Mutex<Result<RemoteTeamData, String>>,
    path: Option<PathBuf>,
    #[cfg(test)]
    fail_next_write: std::sync::atomic::AtomicBool,
}

impl Default for RemoteTeamStore {
    fn default() -> Self {
        Self {
            state: Mutex::new(Ok(RemoteTeamData::default())),
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
            state: Mutex::new(state),
            path: Some(path),
            #[cfg(test)]
            fail_next_write: Default::default(),
        }
    }

    pub fn snapshot(&self) -> Result<RemoteTeamData, String> {
        self.state.lock().clone()
    }

    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut RemoteTeamData) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.state.lock();
        let mut next = guard.as_ref().map_err(Clone::clone)?.clone();
        let result = change(&mut next)?;
        #[cfg(test)]
        if self
            .fail_next_write
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err("injected remote Team persistence failure".into());
        }
        if let Some(path) = &self.path {
            let bytes = serde_json::to_vec(&next).map_err(|error| error.to_string())?;
            codeterm_util::atomic_write::write_atomic(path, bytes)
                .map_err(|error| format!("remote Team persist failed: {error}"))?;
        }
        *guard = Ok(next);
        Ok(result)
    }

    #[cfg(test)]
    pub(super) fn fail_next_persist(&self) {
        self.fail_next_write
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn members_for_team(&self, team: &str) -> Result<Vec<RemoteTeamMember>, String> {
        Ok(self
            .snapshot()?
            .members
            .into_values()
            .filter(|member| member.team_id == team)
            .collect())
    }
}
