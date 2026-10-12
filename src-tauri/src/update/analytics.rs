use codeterm_analytics::{Event, UpdateKind, Version, Worker};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Pending {
    from: String,
    to: String,
    attempts: u32,
}

pub(crate) fn stage(path: &Path, from: &str, to: &str) -> Result<(), String> {
    if Version::from_release(from).is_none() || Version::from_release(to).is_none() || from == to {
        return Ok(());
    }
    let previous = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Pending>(&bytes).ok());
    let attempts = previous
        .filter(|old| old.from == from && old.to == to)
        .map_or(1, |old| old.attempts.saturating_add(1));
    let value = Pending {
        from: from.into(),
        to: to.into(),
        attempts,
    };
    let staged = path.with_extension("pending");
    std::fs::write(
        &staged,
        serde_json::to_vec(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(staged, path).map_err(|e| e.to_string())
}

pub(crate) fn stage_app(to: &str) {
    if stage(
        &crate::paths::data_dir().join("analytics-update.json"),
        env!("CARGO_PKG_VERSION"),
        to,
    )
    .is_err()
    {
        log::debug!("[analytics] update receipt unavailable");
    }
}

pub(crate) fn confirm(path: &Path, running: &str, worker: &Worker) -> Result<(), String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let pending: Pending = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if pending.to != running || pending.from == running {
        return Ok(());
    }
    let event = Event::UpdateInstalled {
        from_version: Version::from_release(&pending.from).ok_or("Invalid update version")?,
        to_version: Version::from_release(running).ok_or("Invalid running version")?,
        kind: UpdateKind::App,
        attempt_count: pending.attempts,
    };
    tauri::async_runtime::block_on(worker.record(event))?;
    std::fs::remove_file(path).map_err(|e| e.to_string())
}
