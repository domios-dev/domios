use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Migration {
    pub(super) removed: usize,
    pub(super) retained: usize,
    pub(super) notified: bool,
}

fn path(root: &Path, provider_id: &str) -> Result<PathBuf, String> {
    if provider_id.is_empty()
        || !provider_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Invalid provider migration identity".into());
    }
    Ok(root
        .join("launch-profile-migrations-v1")
        .join(format!("{provider_id}.json")))
}

pub(super) fn load(root: &Path, provider_id: &str) -> Result<Option<Migration>, String> {
    load_at(&path(root, provider_id)?)
}

fn load_at(path: &Path) -> Result<Option<Migration>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub(super) fn save(root: &Path, provider_id: &str, state: &Migration) -> Result<(), String> {
    let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
    codeterm_util::atomic_write::write_atomic(&path(root, provider_id)?, bytes)
        .map_err(|error| error.to_string())
}

pub(super) fn should_migrate(state: Result<Option<&Migration>, &str>) -> bool {
    matches!(state, Ok(None))
}

pub(super) fn should_notify(state: &Migration) -> bool {
    !state.notified
}

pub(super) fn continue_sweep(candidates: usize, removed: usize) -> bool {
    removed > 0 && candidates > removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_migration_runs_once_and_unknown_state_fails_closed() {
        assert!(should_migrate(Ok(None)));
        let state = Migration::default();
        assert!(!should_migrate(Ok(Some(&state))));
        assert!(!should_migrate(Err("unreadable")));
    }

    #[test]
    fn legacy_notice_is_pending_until_recorded() {
        let mut state = Migration {
            removed: 287,
            retained: 2,
            notified: false,
        };
        assert!(should_notify(&state));
        state.notified = true;
        assert!(!should_notify(&state));
    }

    #[test]
    fn capped_sweeps_continue_only_on_observed_progress() {
        assert!(continue_sweep(318, 200));
        assert!(!continue_sweep(118, 118));
        assert!(!continue_sweep(20, 0));
        assert!(!continue_sweep(0, 0));
    }

    #[test]
    fn migration_completion_survives_reload() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("migration.json");
        assert!(load_at(&path).unwrap().is_none());
        let state = Migration {
            removed: 287,
            retained: 1,
            notified: true,
        };
        std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
        let restored = load_at(&path).unwrap().unwrap();
        assert!(!should_migrate(Ok(Some(&restored))));
        assert!(!should_notify(&restored));
        assert_eq!((restored.removed, restored.retained), (287, 1));
    }
}
