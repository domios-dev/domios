//! One persisted limit on automatic availability checks: startup, focus, timers and channels all pass here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discovery_policy::automatic_check_due;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckTrigger {
    Automatic,
    Explicit,
}

impl CheckTrigger {
    pub fn from_force(force: bool) -> Self {
        if force {
            Self::Explicit
        } else {
            Self::Automatic
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    last_attempt_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CheckThrottle {
    path: PathBuf,
}

impl CheckThrottle {
    pub fn in_root(root: &Path) -> Self {
        Self {
            path: root.join("check-throttle.json"),
        }
    }

    pub fn due(&self, now_ms: u64) -> bool {
        automatic_check_due(self.read().last_attempt_ms, now_ms)
    }

    /// A failed attempt counts too, and an unwritable file costs persistence, never the check.
    pub fn admit(&self, trigger: CheckTrigger, now_ms: u64) -> bool {
        let lock = self.lock();
        let admitted = trigger == CheckTrigger::Explicit
            || automatic_check_due(self.read().last_attempt_ms, now_ms);
        if admitted {
            if let Err(error) = self.write(now_ms) {
                log::warn!("[updater] update check time not saved: {error}");
            }
        }
        drop(lock);
        admitted
    }

    fn lock(&self) -> Option<std::fs::File> {
        std::fs::create_dir_all(self.path.parent()?).ok()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.path.with_extension("lock"))
            .ok()?;
        file.lock().ok()?;
        Some(file)
    }

    fn read(&self) -> Stored {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, now_ms: u64) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or("update check path has no parent")?;
        let bytes = serde_json::to_vec(&Stored {
            last_attempt_ms: Some(now_ms),
        })
        .map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        std::io::Write::write_all(&mut temporary, &bytes).map_err(|error| error.to_string())?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
