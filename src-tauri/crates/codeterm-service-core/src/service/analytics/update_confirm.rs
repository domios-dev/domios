use codeterm_analytics::{Event, UpdateKind, Worker};
use std::path::Path;
pub fn stage(path: &Path, target: &str) -> Result<(), String> {
    if target == codeterm_build_info::version() {
        return Ok(());
    }
    let staging = path.with_extension("pending");
    std::fs::write(&staging, target).map_err(|e| e.to_string())?;
    std::fs::rename(staging, path).map_err(|e| e.to_string())
}
pub fn stage_app(target: &str) {
    if stage(
        &crate::paths::data_dir().join("analytics-update.json"),
        target,
    )
    .is_err()
    {
        log::debug!("[analytics] update receipt unavailable");
    }
}
pub(crate) fn confirm(path: &Path, running: &str, worker: &Worker) -> Result<(), String> {
    let target = match std::fs::read_to_string(path) {
        Ok(target) => target,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if target != running {
        return Ok(());
    }
    tauri::async_runtime::block_on(worker.record(Event::UpdateInstalled {
        kind: UpdateKind::App,
    }))?;
    std::fs::remove_file(path).map_err(|e| e.to_string())
}
