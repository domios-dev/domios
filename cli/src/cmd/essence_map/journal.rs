//! The append-only decision log beside the essence map, and the two owner-
//! facing operations built on it: one release summary, and the revert it
//! offers. Map generation writes records here; it never notifies per change.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{is_sha256, sha256, write_text};

pub(crate) const DECISION_ID_PREFIX: &str = "decision-";
const HISTORY_DIR_NAME: &str = "map-history";

/// Labels of the two options a release summary offers. A label is the whole
/// payload of a choice, so both sides read the same constant.
pub(crate) const KEEP_CHOICE: &str = "Keep the generated map";
pub(crate) const REVERT_CHOICE_PREFIX: &str = "Revert the map to ";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionRecord {
    pub kind: DecisionKind,
    pub id: String,
    pub map_sha256: String,
    pub summary: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum DecisionKind {
    #[serde(rename = "decision")]
    Decision,
}

/// What one `record` call did. An unchanged map appends nothing, so a boundary
/// that reproduces the same map stays silent instead of logging a no-op.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub(crate) enum RecordOutcome {
    Appended {
        id: String,
        map_sha256: String,
        decisions: usize,
    },
    Unchanged {
        id: String,
        map_sha256: String,
    },
}

impl RecordOutcome {
    pub(crate) fn decision_id(&self) -> &str {
        match self {
            Self::Appended { id, .. } | Self::Unchanged { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DecisionLine {
    pub id: String,
    pub map_sha256: String,
    pub summary: String,
    pub reason: String,
}

/// One owner-facing summary covering every decision of a release window. The
/// choices are empty when the window holds nothing to keep or undo.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ReleaseSummary {
    pub baseline: Option<String>,
    pub baseline_map_sha256: Option<String>,
    pub current_map_sha256: Option<String>,
    pub changes: Vec<DecisionLine>,
    pub choices: Vec<String>,
}

impl ReleaseSummary {
    /// The single revert option, or `None` when the window has no baseline to
    /// return to.
    pub(crate) fn revert_choice(&self) -> Option<&str> {
        self.choices
            .iter()
            .find(|choice| choice.starts_with(REVERT_CHOICE_PREFIX))
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct RevertOutcome {
    pub restored: String,
    pub map_sha256: String,
    pub decision: String,
}

pub(crate) fn decision_number(id: &str) -> Option<u64> {
    id.strip_prefix(DECISION_ID_PREFIX)
        .and_then(|value| value.parse().ok())
}

fn format_id(number: u64) -> String {
    format!("{DECISION_ID_PREFIX}{number:04}")
}

/// Read and fully validate the log. Reading is validation: a caller never sees
/// a malformed or out-of-order record.
pub(crate) fn read(path: &Path) -> Result<Vec<DecisionRecord>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read decisions {}: {error}", path.display()))?;
    let mut previous_number = 0u64;
    let mut records = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let at = format!("decisions {} line {line_number}", path.display());
        if line.trim().is_empty() {
            return Err(format!("{at}: blank JSONL record"));
        }
        let decision: DecisionRecord = serde_json::from_str(line)
            .map_err(|error| format!("{at}: malformed record: {error}"))?;
        if decision.id.trim().is_empty()
            || decision.summary.trim().is_empty()
            || decision.reason.trim().is_empty()
        {
            return Err(format!("{at}: id, summary and reason are required"));
        }
        if !is_sha256(&decision.map_sha256) {
            return Err(format!(
                "{at}: map_sha256 must be 64 lowercase hex characters"
            ));
        }
        let Some(number) = decision_number(&decision.id) else {
            return Err(format!(
                "{at}: id `{}` must use decision-N numbering",
                decision.id
            ));
        };
        if number <= previous_number {
            return Err(format!(
                "{at}: decision ids must be strictly increasing (append-only)"
            ));
        }
        previous_number = number;
        records.push(decision);
    }
    Ok(records)
}

pub(crate) fn validate_decisions(path: &Path) -> Result<(), String> {
    read(path).map(|_| ())
}

fn map_digest(map_path: &Path) -> Result<String, String> {
    let bytes = fs::read(map_path)
        .map_err(|error| format!("cannot read map {}: {error}", map_path.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|error| format!("map {} is not UTF-8: {error}", map_path.display()))?;
    Ok(sha256(text.replace("\r\n", "\n").as_bytes()))
}

fn history_dir(map_path: &Path, data_root: &Path) -> PathBuf {
    let project_root = map_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let project_key = sha256(project_root.to_string_lossy().as_bytes());
    data_root.join(HISTORY_DIR_NAME).join(project_key)
}

fn history_file(map_path: &Path, digest: &str, data_root: &Path) -> PathBuf {
    history_dir(map_path, data_root).join(format!("{digest}.jsonl"))
}

/// Keep the bytes a regeneration is about to replace, so the revert a release
/// summary offers can restore them. Content-addressed, so regenerating the same
/// map writes one file.
pub(crate) fn retain_superseded_map(
    map_path: &Path,
    next_contents: &str,
    data_root: &Path,
) -> Result<(), String> {
    let Ok(existing) = fs::read(map_path) else {
        return Ok(());
    };
    let digest = sha256(
        String::from_utf8(existing.clone())
            .map_err(|error| {
                format!(
                    "superseded map {} is not UTF-8: {error}",
                    map_path.display()
                )
            })?
            .replace("\r\n", "\n")
            .as_bytes(),
    );
    if digest == sha256(next_contents.as_bytes()) {
        return Ok(());
    }
    let target = history_file(map_path, &digest, data_root);
    if target.exists() {
        return Ok(());
    }
    let text = String::from_utf8(existing).map_err(|error| {
        format!(
            "superseded map {} is not UTF-8: {error}",
            map_path.display()
        )
    })?;
    write_text(&target, &text)
}

/// Append one decision for the map as it stands. An unchanged map appends
/// nothing: the log records changes, not runs.
pub(crate) fn record(
    map_path: &Path,
    decisions_path: &Path,
    summary: &str,
    reason: &str,
) -> Result<RecordOutcome, String> {
    let summary = summary.trim();
    let reason = reason.trim();
    if summary.is_empty() || reason.is_empty() {
        return Err("a decision requires both --summary and --reason".to_string());
    }
    let digest = map_digest(map_path)?;
    let records = read(decisions_path)?;
    if let Some(last) = records.last() {
        if last.map_sha256 == digest {
            return Ok(RecordOutcome::Unchanged {
                id: last.id.clone(),
                map_sha256: digest,
            });
        }
    }
    let next = records
        .last()
        .and_then(|last| decision_number(&last.id))
        .unwrap_or(0)
        + 1;
    let decision = DecisionRecord {
        kind: DecisionKind::Decision,
        id: format_id(next),
        map_sha256: digest.clone(),
        summary: summary.to_string(),
        reason: reason.to_string(),
    };
    append(decisions_path, &decision)?;
    Ok(RecordOutcome::Appended {
        id: decision.id,
        map_sha256: digest,
        decisions: records.len() + 1,
    })
}

fn append(decisions_path: &Path, decision: &DecisionRecord) -> Result<(), String> {
    let line = serde_json::to_string(decision)
        .map_err(|error| format!("cannot encode decision: {error}"))?;
    let mut text = fs::read_to_string(decisions_path).map_err(|error| {
        format!(
            "cannot read decisions {}: {error}",
            decisions_path.display()
        )
    })?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    text.push('\n');
    write_text(decisions_path, &text)
}

/// The one summary a release emits. Every decision after the baseline is
/// reported together, and the single revert option names that baseline.
pub(crate) fn release_summary(
    map_path: &Path,
    decisions_path: &Path,
    since: Option<&str>,
) -> Result<ReleaseSummary, String> {
    let records = read(decisions_path)?;
    let baseline_index = match since {
        Some(id) => Some(
            records
                .iter()
                .position(|record| record.id == id)
                .ok_or_else(|| format!("decision `{id}` is not in the log"))?,
        ),
        None if records.is_empty() => None,
        None => Some(0),
    };
    let baseline = baseline_index.map(|index| &records[index]);
    let changes: Vec<DecisionLine> = baseline_index
        .map(|index| &records[index + 1..])
        .unwrap_or(&records[..])
        .iter()
        .map(|record| DecisionLine {
            id: record.id.clone(),
            map_sha256: record.map_sha256.clone(),
            summary: record.summary.clone(),
            reason: record.reason.clone(),
        })
        .collect();
    let choices = match baseline {
        Some(baseline) if !changes.is_empty() => vec![
            KEEP_CHOICE.to_string(),
            format!("{REVERT_CHOICE_PREFIX}{}", baseline.id),
        ],
        _ => Vec::new(),
    };
    Ok(ReleaseSummary {
        baseline: baseline.map(|record| record.id.clone()),
        baseline_map_sha256: baseline.map(|record| record.map_sha256.clone()),
        current_map_sha256: map_digest(map_path).ok(),
        changes,
        choices,
    })
}

/// Restore the map a decision recorded. The log stays append-only: the revert
/// is itself a new decision.
pub(crate) fn revert(
    map_path: &Path,
    decisions_path: &Path,
    to: &str,
    data_root: &Path,
) -> Result<RevertOutcome, String> {
    let records = read(decisions_path)?;
    let target = records
        .iter()
        .find(|record| record.id == to)
        .ok_or_else(|| format!("decision `{to}` is not in the log"))?
        .clone();
    if map_digest(map_path).ok().as_deref() != Some(target.map_sha256.as_str()) {
        let snapshot = history_file(map_path, &target.map_sha256, data_root);
        let contents = fs::read_to_string(&snapshot).map_err(|error| {
            format!(
                "no retained map for {to} at {}: {error}",
                snapshot.display()
            )
        })?;
        retain_superseded_map(map_path, &contents, data_root)?;
        write_text(map_path, &contents)?;
    }
    let outcome = record(
        map_path,
        decisions_path,
        &format!("Revert the map to {to}"),
        "The owner took the revert offered with the release summary; the map returns to the state that decision recorded.",
    )?;
    Ok(RevertOutcome {
        restored: to.to_string(),
        map_sha256: target.map_sha256,
        decision: outcome.decision_id().to_string(),
    })
}
