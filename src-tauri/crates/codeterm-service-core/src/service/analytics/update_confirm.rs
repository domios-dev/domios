use codeterm_analytics::{Event, UpdateKind, Worker};
use std::path::Path;
pub(crate) fn confirm(path: &Path, running: &str, worker: &Worker) -> Result<(), String> {
    let target = match std::fs::read_to_string(path) {
        Ok(target) => target,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if target != running {
        return Ok(());
    }
    crate::async_runtime::block_on(worker.record(Event::UpdateInstalled {
        kind: UpdateKind::App,
    }))?;
    std::fs::remove_file(path).map_err(|e| e.to_string())
}
