use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::agent::provider::ModelSwitchTurnCompletion;

pub(super) struct RecordBoundary {
    path: PathBuf,
    offset: u64,
    declaration: ModelSwitchTurnCompletion,
    active_turn: Option<String>,
    more: bool,
    started: bool,
}

impl RecordBoundary {
    pub(super) fn capture(
        path: PathBuf,
        declaration: ModelSwitchTurnCompletion,
        session: &str,
    ) -> Option<Self> {
        let offset = std::fs::metadata(&path).ok()?.len();
        let mut boundary = Self {
            path,
            offset,
            declaration,
            active_turn: None,
            more: false,
            started: false,
        };
        let start = offset.saturating_sub(512 * 1024);
        let mut file = std::fs::File::open(&boundary.path).ok()?;
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.take(offset - start).read_to_end(&mut bytes).ok()?;
        boundary.observe(&bytes, session);
        Some(boundary)
    }

    pub(super) fn watch(&self, wake: Sender<()>) -> Result<notify::RecommendedWatcher, String> {
        let path = std::fs::canonicalize(&self.path).map_err(|error| error.to_string())?;
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|event| {
                    event.paths.iter().any(|changed| {
                        std::fs::canonicalize(changed).is_ok_and(|changed| changed == path)
                    })
                }) {
                    let _ = wake.send(());
                }
            })
            .map_err(|error| error.to_string())?;
        notify::Watcher::watch(
            &mut watcher,
            self.path
                .parent()
                .ok_or_else(|| "the completion transcript has no parent".to_owned())?,
            notify::RecursiveMode::NonRecursive,
        )
        .map_err(|error| error.to_string())?;
        Ok(watcher)
    }

    pub(super) fn has_more(&self) -> bool {
        self.more
    }

    pub(super) fn started(&self) -> bool {
        self.started
    }

    pub(super) fn completed(&mut self, session: &str) -> Result<bool, String> {
        self.started = false;
        let mut file = std::fs::File::open(&self.path).map_err(|error| error.to_string())?;
        if file.metadata().map_err(|error| error.to_string())?.len() < self.offset {
            return Err("the completion transcript was truncated".into());
        }
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        self.more = bytes.len() == 8 * 1024 * 1024;
        let Some(end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
            if self.more {
                return Err("the completion transcript record exceeds its bounded read".into());
            }
            return Ok(false);
        };
        self.offset += end as u64 + 1;
        Ok(self.observe(&bytes[..=end], session))
    }

    fn observe(&mut self, bytes: &[u8], session: &str) -> bool {
        let mut completed = false;
        for line in bytes.split(|byte| *byte == b'\n') {
            let Ok(record) = serde_json::from_slice::<serde_json::Value>(line) else {
                continue;
            };
            if record
                .pointer(&self.declaration.session_pointer)
                .and_then(serde_json::Value::as_str)
                != Some(session)
            {
                continue;
            }
            let event = record
                .pointer(&self.declaration.event_pointer)
                .and_then(serde_json::Value::as_str);
            if event == Some(&self.declaration.start_event_value) {
                self.started = true;
                self.active_turn = None;
                completed = false;
            }
            if let Some(turn) = record
                .pointer(&self.declaration.active_turn_pointer)
                .and_then(serde_json::Value::as_str)
                .filter(|turn| !turn.is_empty())
            {
                if self.active_turn.as_deref() != Some(turn) {
                    self.started |= self.active_turn.is_some();
                    completed = false;
                }
                self.active_turn = Some(turn.into());
            }
            if matches_record(&self.declaration, &record, session)
                && self.active_turn.is_some()
                && record
                    .pointer(&self.declaration.turn_pointer)
                    .and_then(serde_json::Value::as_str)
                    == self.active_turn.as_deref()
            {
                completed = true;
            }
        }
        completed
    }
}

fn matches_record(
    declaration: &ModelSwitchTurnCompletion,
    record: &serde_json::Value,
    session: &str,
) -> bool {
    !declaration.event_pointer.is_empty()
        && !declaration.session_pointer.is_empty()
        && !declaration.event_value.is_empty()
        && record
            .pointer(&declaration.event_pointer)
            .and_then(serde_json::Value::as_str)
            == Some(&declaration.event_value)
        && record
            .pointer(&declaration.session_pointer)
            .and_then(serde_json::Value::as_str)
            == Some(session)
}

#[cfg(test)]
mod tests;
