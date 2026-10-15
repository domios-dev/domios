use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::recovery::MAX_AUTOMATIC_RESTARTS;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Episode {
    pub(crate) restarts: u32,
    pub(crate) owner_required: bool,
    labels_restarted: BTreeSet<String>,
    events: BTreeSet<String>,
}

impl Episode {
    pub(crate) fn restarted_for_label(&self, label: &str) -> bool {
        self.labels_restarted.contains(label)
    }
}

pub(crate) struct EpisodeStore {
    path: PathBuf,
    state: Mutex<Option<Episode>>,
}

impl EpisodeStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        let state = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Episode::default()),
            Err(_) => None,
        };
        Self {
            path,
            state: Mutex::new(state),
        }
    }

    fn save(&self, state: &Episode) -> Result<(), String> {
        codeterm_util::atomic_write::write_atomic(
            &self.path,
            serde_json::to_vec(state).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    pub(crate) fn snapshot(&self) -> Episode {
        self.state.lock().clone().unwrap_or_else(|| Episode {
            owner_required: true,
            ..Episode::default()
        })
    }

    pub(crate) fn claim_restart(
        &self,
        event: &str,
        label: &str,
        configured_limit: u32,
    ) -> Result<bool, String> {
        let mut state = self.state.lock();
        let current = state
            .as_ref()
            .ok_or("recovery episode storage is unavailable")?;
        if current.owner_required
            || current.restarts >= configured_limit.min(MAX_AUTOMATIC_RESTARTS)
            || current.events.contains(event)
        {
            return Ok(false);
        }
        let mut next = current.clone();
        next.restarts += 1;
        next.events.insert(event.to_string());
        next.labels_restarted.insert(label.to_string());
        self.save(&next)?;
        *state = Some(next);
        Ok(true)
    }

    pub(crate) fn require_owner(&self) -> Result<bool, String> {
        let mut state = self.state.lock();
        let current = state
            .as_ref()
            .ok_or("recovery episode storage is unavailable")?;
        if current.owner_required {
            return Ok(false);
        }
        let mut next = current.clone();
        next.owner_required = true;
        self.save(&next)?;
        *state = Some(next);
        Ok(true)
    }

    pub(crate) fn reset_by_owner(&self) -> Result<(), String> {
        let mut state = self.state.lock();
        let next = Episode::default();
        self.save(&next)?;
        *state = Some(next);
        Ok(())
    }
}

pub(crate) fn store() -> &'static EpisodeStore {
    static STORE: LazyLock<EpisodeStore> = LazyLock::new(|| {
        EpisodeStore::new(
            super::general_dir()
                .join("state")
                .join("recovery-episode.json"),
        )
    });
    &STORE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ga_corrupt_recovery_episode_refuses_automatic_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("episode.json");
        std::fs::write(&path, b"invalid").unwrap();
        let store = EpisodeStore::new(path);
        assert!(store.snapshot().owner_required);
        assert!(store.claim_restart("exit", "crashed_to_shell", 2).is_err());
    }

    #[test]
    fn ga_restart_reservations_survive_restart_and_reset_only_by_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("episode.json");
        let first = EpisodeStore::new(path.clone());
        assert!(first
            .claim_restart("launch-1", "crashed_to_shell", 99)
            .unwrap());
        assert!(!first
            .claim_restart("launch-1", "crashed_to_shell", 99)
            .unwrap());
        let resumed = EpisodeStore::new(path);
        assert!(resumed
            .claim_restart("launch-2", "crashed_to_shell", 99)
            .unwrap());
        assert!(!resumed.claim_restart("launch-3", "error_loop", 99).unwrap());
        assert!(resumed.require_owner().unwrap());
        assert!(!resumed.require_owner().unwrap());
        resumed.reset_by_owner().unwrap();
        assert!(resumed.claim_restart("launch-3", "error_loop", 99).unwrap());
    }
}
