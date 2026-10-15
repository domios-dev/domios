use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_JOURNAL_BYTES: u64 = 1_048_576;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Rerank,
    TabState,
    #[serde(rename = "blueprint")]
    Sentinel,
    Heartbeat,
}

impl VerdictKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rerank => "rerank",
            Self::TabState => "tab_state",
            Self::Sentinel => "blueprint",
            Self::Heartbeat => "heartbeat",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    SpawnOk,
    #[serde(rename = "strong_action")]
    Strong,
    WeakHint,
    #[cfg(test)]
    Rerank,
    Recovery,
}

#[derive(Serialize)]
pub struct ActionRecord<'a> {
    pub kind: VerdictKind,
    pub tab: Option<&'a str>,
    pub label: &'a str,
    pub confidence: f64,
    pub action: Action,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked: Option<Vec<super::narrative::QuestionAnswer>>,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct CallSnapshot {
    pub(super) calls: VecDeque<i64>,
    pub(super) budget_reached: bool,
}

pub(super) struct Journal {
    path: PathBuf,
}

impl Journal {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self {
            path: dir.join("journal.jsonl"),
        }
    }

    pub fn feed(&self, limit: usize, team: Option<&str>) -> io::Result<Vec<Value>> {
        let limit = limit.min(super::feed::MAX_ENTRIES);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut entries = VecDeque::with_capacity(limit);
        for path in [
            self.path.with_extension("previous.jsonl"),
            self.path.clone(),
        ] {
            let file = match fs::File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let mut text = String::new();
            file.take(MAX_JOURNAL_BYTES).read_to_string(&mut text)?;
            for line in text.lines() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if team.is_some_and(|id| value["team_id"].as_str() != Some(id)) {
                    continue;
                }
                let Some(entry) = super::feed::entry(&value) else {
                    continue;
                };
                if entries.len() == limit {
                    entries.pop_front();
                }
                entries.push_back(entry);
            }
        }
        Ok(entries.into_iter().rev().collect())
    }

    pub(super) fn load_calls(&self) -> io::Result<CallSnapshot> {
        super::storage::load(&self.path.with_file_name("calls.json"))
    }

    pub(super) fn save_calls(&self, calls: &VecDeque<i64>, budget_reached: bool) -> io::Result<()> {
        super::storage::save(
            &self.path.with_file_name("calls.json"),
            &serde_json::json!({ "calls": calls, "budget_reached": budget_reached }),
        )
    }

    pub(super) fn claim_notification(&self, key: &str) -> io::Result<bool> {
        let path = self.path.with_file_name("notification-keys.json");
        let mut keys: std::collections::BTreeSet<String> = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Default::default(),
            Err(error) => return Err(error),
        };
        if keys.contains(key) {
            return Ok(false);
        }
        if keys.len() >= 4096 {
            return Err(io::Error::other("notification journal index is full"));
        }
        keys.insert(key.into());
        self.append(&serde_json::json!({ "time": chrono::Utc::now().timestamp(), "kind": "notification_claim", "key": key }))?;
        codeterm_util::atomic_write::write_atomic(&path, serde_json::to_vec(&keys)?)?;
        Ok(true)
    }

    pub fn append(&self, value: &Value) -> io::Result<()> {
        let mut value = value.clone();
        if let Some(asked) = value.get("asked") {
            let asked = if value["kind"] == "tab_state" {
                crate::service::supporter_tab_state::redact_decision_diagnostic(asked)
            } else {
                asked.clone()
            };
            value["asked"] = serde_json::to_value(super::narrative::project(&asked))?;
        }
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal entry too large",
            ));
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let size = match fs::metadata(&self.path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        if size + bytes.len() as u64 > MAX_JOURNAL_BYTES {
            let previous = self.path.with_extension("previous.jsonl");
            match fs::remove_file(&previous) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::rename(&self.path, previous)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()
    }
}

#[cfg(test)]
mod tests;
