use std::collections::HashMap;
use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_MS: u64 = 60_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtractionRetry {
    pub attempt: u32,
    pub parked: bool,
    pub retry_after: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    fingerprint: String,
    worker_id: String,
    retry: ExtractionRetry,
    finished: bool,
    succeeded: bool,
}

pub struct ExtractionRetries {
    path: PathBuf,
    entries: Mutex<Result<HashMap<String, Entry>, String>>,
}

pub fn fingerprint(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.len().to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

impl ExtractionRetries {
    pub fn load(path: PathBuf) -> Self {
        let entries = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(e) => Err(e.to_string()),
        };
        Self {
            path,
            entries: Mutex::new(entries),
        }
    }

    fn persist(&self, entries: &HashMap<String, Entry>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec(entries).map_err(|e| e.to_string())?;
        codeterm_util::atomic_write::write_atomic(&self.path, bytes).map_err(|e| e.to_string())
    }

    pub fn reset(&self, intent: &str) -> Result<(), String> {
        let mut state = self.entries.lock();
        let entries = state.as_mut().map_err(|e| e.clone())?;
        if entries.get(intent).is_some_and(|entry| !entry.finished) {
            return Err("extraction worker is still active".into());
        }
        entries.remove(intent);
        self.persist(entries)
    }

    pub fn abandon_unfinished(&self, now: u64) -> Result<(), String> {
        let unfinished: Vec<(String, String)> = {
            let state = self.entries.lock();
            state
                .as_ref()
                .map_err(|e| e.clone())?
                .iter()
                .filter(|(_, entry)| !entry.finished)
                .map(|(intent, entry)| (intent.clone(), entry.worker_id.clone()))
                .collect()
        };
        for (intent, worker_id) in unfinished {
            self.finish(
                &intent,
                &worker_id,
                false,
                Some("daemon restarted before worker registration completed".into()),
                now,
            )?;
        }
        Ok(())
    }

    pub fn failed_intents(&self) -> Result<Vec<String>, String> {
        let state = self.entries.lock();
        Ok(state
            .as_ref()
            .map_err(|e| e.clone())?
            .iter()
            .filter(|(_, entry)| entry.finished && !entry.succeeded)
            .map(|(intent, _)| intent.clone())
            .collect())
    }

    pub fn claim(
        &self,
        intent: &str,
        fingerprint: &str,
        worker_id: &str,
        now: u64,
    ) -> Result<Option<ExtractionRetry>, String> {
        let mut state = self.entries.lock();
        let entries = state.as_mut().map_err(|e| e.clone())?;
        if entries.get(intent).is_some_and(|entry| !entry.finished) {
            return Ok(None);
        }
        let previous = entries
            .get(intent)
            .filter(|entry| entry.fingerprint == fingerprint);
        if previous.is_some_and(|entry| {
            !entry.finished
                || entry.succeeded
                || entry.retry.parked
                || entry.retry.attempt >= MAX_ATTEMPTS
                || entry.retry.retry_after.is_some_and(|at| now < at)
        }) {
            return Ok(None);
        }
        let retry = ExtractionRetry {
            attempt: previous.map_or(1, |entry| entry.retry.attempt + 1),
            parked: false,
            retry_after: None,
            reason: None,
        };
        entries.insert(
            intent.into(),
            Entry {
                fingerprint: fingerprint.into(),
                worker_id: worker_id.into(),
                retry: retry.clone(),
                finished: false,
                succeeded: false,
            },
        );
        self.persist(entries)?;
        Ok(Some(retry))
    }

    pub fn finish(
        &self,
        intent: &str,
        worker_id: &str,
        succeeded: bool,
        reason: Option<String>,
        now: u64,
    ) -> Result<Option<ExtractionRetry>, String> {
        let mut state = self.entries.lock();
        let entries = state.as_mut().map_err(|e| e.clone())?;
        let Some(entry) = entries
            .get_mut(intent)
            .filter(|entry| entry.worker_id == worker_id && !entry.finished)
        else {
            return Ok(None);
        };
        entry.finished = true;
        entry.succeeded = succeeded;
        entry.retry.reason = reason;
        entry.retry.parked = !succeeded && entry.retry.attempt >= MAX_ATTEMPTS;
        entry.retry.retry_after = (!succeeded && !entry.retry.parked)
            .then(|| now.saturating_add(BACKOFF_MS * (1 << (entry.retry.attempt - 1))));
        let retry = entry.retry.clone();
        self.persist(entries)?;
        Ok(Some(retry))
    }
}

#[cfg(test)]
mod tests;
