use super::{identifier, integer, text};
use crate::error::StorageError;
use codeterm_blueprint_types::blueprint_journal::{
    JournalMarker, JournalOutcome, JournalReason, JournalState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlueprintJournalEntry {
    pub entry_id: i64,
    pub run_id: String,
    pub step_index: i64,
    pub step_name: String,
    pub state: JournalState,
    pub outcome: Option<JournalOutcome>,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub result_summary: Option<String>,
    pub reason: Option<JournalReason>,
    pub body_ref: Option<String>,
}

impl BlueprintJournalEntry {
    pub fn marker(&self) -> JournalMarker {
        JournalMarker::from_columns(self.step_index, &self.step_name)
            .expect("journal reads validate marker columns")
    }

    pub fn derive_run(entries: &[&Self]) -> (Option<JournalState>, Option<u16>) {
        let mut newest = std::collections::BTreeMap::new();
        for entry in entries {
            let marker = entry.marker();
            let current = newest.entry(marker).or_insert(*entry);
            if current.entry_id < entry.entry_id {
                *current = *entry;
            }
        }
        if let Some(close) = newest.get(&JournalMarker::Run) {
            if close.state != JournalState::Running {
                return (Some(close.state), None);
            }
        }
        let current = newest
            .iter()
            .find_map(|(marker, entry)| match (marker, entry.state) {
                (JournalMarker::Step(index), JournalState::Running) => Some(*index),
                _ => None,
            });
        (None, current)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BlueprintJournalWrite {
    Claim {
        run_id: String,
        marker: JournalMarker,
        step_name: String,
        generation: u64,
        started_at_ms: u64,
    },
    Finish {
        run_id: String,
        marker: JournalMarker,
        state: JournalState,
        outcome: JournalOutcome,
        ended_at_ms: u64,
        result_summary: String,
        body_ref: Option<String>,
        reason: Option<JournalReason>,
    },
    Decision {
        run_id: String,
        marker: JournalMarker,
        step_name: String,
        outcome: JournalOutcome,
        at_ms: u64,
        result_summary: Option<String>,
        reason: Option<JournalReason>,
    },
    CloseRun {
        run_id: String,
        state: JournalState,
        outcome: JournalOutcome,
        at_ms: u64,
        reason: Option<JournalReason>,
    },
}

impl BlueprintJournalWrite {
    pub(in crate::repositories::sentinel) fn validate(&self) -> Result<(), StorageError> {
        let (run_id, marker, step_name, outcome, summary, reason) = match self {
            Self::Claim {
                run_id,
                marker,
                step_name,
                generation,
                ..
            } => {
                integer("journal generation", *generation)?;
                (
                    run_id.as_str(),
                    *marker,
                    Some(step_name.as_str()),
                    None,
                    None,
                    None,
                )
            }
            Self::Finish {
                run_id,
                marker,
                state,
                outcome,
                result_summary,
                reason,
                ..
            } => {
                if !matches!(state, JournalState::Done | JournalState::Failed) {
                    return Err(StorageError::invalid(
                        "journal state",
                        "must be done or failed",
                    ));
                }
                (
                    run_id.as_str(),
                    *marker,
                    None,
                    Some(outcome.as_str()),
                    Some(result_summary.as_str()),
                    reason.as_deref(),
                )
            }
            Self::Decision {
                run_id,
                marker,
                step_name,
                outcome,
                result_summary,
                reason,
                ..
            } => (
                run_id.as_str(),
                *marker,
                Some(step_name.as_str()),
                Some(outcome.as_str()),
                result_summary.as_deref(),
                reason.as_deref(),
            ),
            Self::CloseRun {
                run_id,
                state,
                outcome,
                reason,
                ..
            } => {
                if matches!(state, JournalState::Running) {
                    return Err(StorageError::invalid("journal state", "invalid run state"));
                }
                (
                    run_id.as_str(),
                    JournalMarker::Run,
                    Some("$run"),
                    Some(outcome.as_str()),
                    None,
                    reason.as_deref(),
                )
            }
        };
        if let Self::Finish {
            body_ref: Some(reference),
            ..
        } = self
        {
            text("journal body reference", reference)?;
        }
        identifier("journal run id", run_id)?;
        if matches!(marker, JournalMarker::Step(index) if index >= 64) {
            return Err(StorageError::invalid("journal step index", "out of bounds"));
        }
        if let Some(step_name) = step_name {
            identifier("journal step name", step_name)?;
            if marker.name(step_name) != step_name {
                return Err(StorageError::invalid(
                    "journal marker",
                    "name and marker disagree",
                ));
            }
        }
        if let Some(outcome) = outcome {
            identifier("journal outcome", outcome)?;
        }
        for (field, value) in [
            ("journal result summary", summary),
            ("journal reason", reason),
        ] {
            if let Some(value) = value {
                if value.chars().count() > 200 || value.chars().any(char::is_control) {
                    return Err(StorageError::invalid(
                        field,
                        "must be at most 200 printable characters",
                    ));
                }
            }
        }
        Ok(())
    }
}
