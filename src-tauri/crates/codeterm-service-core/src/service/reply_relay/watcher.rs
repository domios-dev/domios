use crate::service::tab_chat::file_events::{self, WatchGuard};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

pub(super) fn watch(
    history: &Path,
) -> Result<(Vec<WatchGuard>, SyncSender<()>, Receiver<()>), String> {
    let (tx, rx) = sync_channel(1);
    let watchers = watched_sources(history)?
        .iter()
        .map(|path| file_events::watch(path, tx.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((watchers, tx, rx))
}

fn watched_sources(history: &Path) -> Result<[PathBuf; 2], String> {
    let sidecar = history.with_file_name(format!(
        "{}.domios-turns.jsonl",
        history
            .file_name()
            .ok_or("history has no file name")?
            .to_string_lossy()
    ));
    Ok([history.to_path_buf(), sidecar])
}

#[cfg(test)]
mod tests;
