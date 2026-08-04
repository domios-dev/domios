use serde::Serialize;

use super::journal::{JournalLoad, TailStatus};
use super::record::{JournalRecord, LifecycleCheckpoint, LifecycleEntry};

/// Read-only projection of one tab's lifecycle history. Pure data: a CLI, a UI,
/// or a test can render it without recovery depending on any of them.
#[derive(Debug, Clone, Serialize)]
pub struct TabAudit {
    pub tab_id: String,
    pub transitions: Vec<LifecycleEntry>,
    /// Checkpoints the journal still carries for this tab, oldest first.
    pub checkpoints: Vec<LifecycleCheckpoint>,
    /// The one checkpoint a restore may be authorized against, if any.
    pub committed: Option<LifecycleCheckpoint>,
    /// Set when the journal could not be replayed to its end.
    pub tail_refusal: Option<String>,
}

impl TabAudit {
    pub fn read(load: &JournalLoad, tab_id: &str) -> Self {
        let mut transitions = Vec::new();
        let mut checkpoints = Vec::new();
        for record in &load.records {
            if record.tab_id() != tab_id {
                continue;
            }
            match record {
                JournalRecord::Transition { entry } => transitions.push(entry.clone()),
                JournalRecord::Checkpoint { checkpoint } => checkpoints.push(checkpoint.clone()),
            }
        }
        Self {
            tab_id: tab_id.to_string(),
            transitions,
            checkpoints,
            committed: super::journal::checkpoints(load)
                .into_iter()
                .find(|checkpoint| checkpoint.key.tab_id == tab_id),
            tail_refusal: match &load.tail {
                TailStatus::Intact => None,
                TailStatus::Refused { line, reason } => Some(format!("line {line}: {reason}")),
            },
        }
    }
}
