use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::RateHeaders;

/// GitHub allows 60 anonymous API requests an hour per IP; the rest stay free for other clients on it.
pub const API_REQUESTS_PER_HOUR: u32 = 50;
const WINDOW_MS: u64 = 60 * 60 * 1000;
// A moved clock must not park the fallback past GitHub's own hourly reset.
const MAX_BLOCK_MS: u64 = 2 * WINDOW_MS;

fn retry_message(until_ms: u64) -> String {
    let clock = chrono::DateTime::from_timestamp_millis(until_ms as i64)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "later".into());
    format!("GitHub rate limit, retry at {clock}")
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiBudget {
    window_start_ms: u64,
    used: u32,
    blocked_until_ms: u64,
}

impl ApiBudget {
    pub fn admit(&mut self, now_ms: u64) -> Result<(), String> {
        if self.blocked_until_ms > now_ms
            && self.blocked_until_ms.saturating_sub(now_ms) <= MAX_BLOCK_MS
        {
            return Err(retry_message(self.blocked_until_ms));
        }
        if now_ms < self.window_start_ms || now_ms.saturating_sub(self.window_start_ms) >= WINDOW_MS
        {
            self.window_start_ms = now_ms;
            self.used = 0;
        }
        if self.used >= API_REQUESTS_PER_HOUR {
            return Err(retry_message(
                self.window_start_ms.saturating_add(WINDOW_MS),
            ));
        }
        self.used += 1;
        Ok(())
    }

    /// Records a reply; returns the retry message when it put the API out of reach.
    pub fn observe(&mut self, status: u16, rate: &RateHeaders, now_ms: u64) -> Option<String> {
        let limited = matches!(status, 403 | 429);
        let until = if limited && rate.retry_after_secs.is_some() {
            rate.retry_after_secs
                .map(|secs| now_ms.saturating_add(secs.saturating_mul(1000)))
        } else if rate.remaining == Some(0) {
            rate.reset_unix.map(|reset| reset.saturating_mul(1000))
        } else {
            None
        };
        let until = until?.min(now_ms.saturating_add(MAX_BLOCK_MS));
        if until <= now_ms {
            return None;
        }
        self.blocked_until_ms = until;
        Some(retry_message(until))
    }
}

/// The budget persists per install, so a restart cannot spend the hour twice.
#[derive(Debug, Clone)]
pub struct ApiBudgetStore {
    path: PathBuf,
}

impl ApiBudgetStore {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn official() -> Self {
        Self::at(
            codeterm_paths::data_dir_path()
                .join("update-manifests")
                .join("github-api-budget.json"),
        )
    }

    pub fn admit(&self, now_ms: u64) -> Result<(), String> {
        self.update(|budget| budget.admit(now_ms))
    }

    pub fn observe(&self, status: u16, rate: &RateHeaders, now_ms: u64) -> Option<String> {
        self.update(|budget| budget.observe(status, rate, now_ms))
    }

    fn update<T>(&self, change: impl FnOnce(&mut ApiBudget) -> T) -> T {
        let lock = self.lock();
        let mut budget = self.read();
        let before = budget.clone();
        let result = change(&mut budget);
        if budget != before {
            if let Err(error) = self.write(&budget) {
                log::warn!("[updater] GitHub API budget not saved: {error}");
            }
        }
        drop(lock);
        result
    }

    fn lock(&self) -> Option<std::fs::File> {
        let parent = self.path.parent()?;
        std::fs::create_dir_all(parent).ok()?;
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

    fn read(&self) -> ApiBudget {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write(&self, budget: &ApiBudget) -> Result<(), String> {
        let bytes = serde_json::to_vec(budget).map_err(|error| error.to_string())?;
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &self.path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            error.to_string()
        })
    }
}
