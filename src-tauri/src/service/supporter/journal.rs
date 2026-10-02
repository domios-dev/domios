use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_JOURNAL_BYTES: u64 = 1_048_576;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerdictKind {
    Rerank,
    TabState,
}

impl VerdictKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Rerank => "rerank",
            Self::TabState => "tab_state",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    SpawnOk,
    StrongAction,
    WeakHint,
    Rerank,
    Recovery,
}

#[derive(Serialize)]
pub(crate) struct ActionRecord<'a> {
    pub(crate) kind: VerdictKind,
    pub(crate) tab: Option<&'a str>,
    pub(crate) label: &'a str,
    pub(crate) confidence: f64,
    pub(crate) action: Action,
    pub(crate) reason: &'static str,
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

    pub(super) fn load_calls(&self) -> io::Result<CallSnapshot> {
        match fs::read(self.path.with_file_name("calls.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(CallSnapshot::default()),
            Err(error) => Err(error),
        }
    }

    pub(super) fn save_calls(&self, calls: &VecDeque<i64>, budget_reached: bool) -> io::Result<()> {
        let Some(dir) = self.path.parent() else {
            return Err(io::Error::other("journal directory missing"));
        };
        fs::create_dir_all(dir)?;
        let target = dir.join("calls.json");
        let temp = dir.join(format!(".calls-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            serde_json::to_writer(
                &mut file,
                &serde_json::json!({ "calls": calls, "budget_reached": budget_reached }),
            )?;
            file.flush()?;
            file.sync_all()?;
            fs::rename(&temp, &target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }

    pub(super) fn append(&self, value: &Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
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
        file.flush()
    }
}

#[cfg(test)]
mod tests;
