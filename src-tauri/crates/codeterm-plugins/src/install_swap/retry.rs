use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

pub(super) struct ChangeRetry {
    _watcher: RecommendedWatcher,
    events: mpsc::Receiver<notify::Result<notify::Event>>,
    deadline: Instant,
}

impl ChangeRetry {
    pub(super) fn new(staging: &Path) -> notify::Result<Self> {
        let (sender, events) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        watcher.watch(staging, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            events,
            deadline: Instant::now() + Duration::from_secs(5),
        })
    }

    pub(super) fn changed(&self, staging: &Path, destination: &Path) -> bool {
        changed(&self.events, self.deadline, staging, destination)
    }
}

fn changed(
    events: &mpsc::Receiver<notify::Result<notify::Event>>,
    deadline: Instant,
    staging: &Path,
    destination: &Path,
) -> bool {
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        if !staging.is_dir() || !matches!(destination.try_exists(), Ok(false)) {
            return false;
        }
        match events.recv_timeout(remaining) {
            Ok(Ok(event)) if !event.kind.is_access() && !event.paths.is_empty() => return true,
            Ok(Ok(_)) => continue,
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests;
