use crate::plan::Milestone;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

#[derive(Debug)]
pub enum StrideLogError {
    Io(io::Error),
    Serialize(serde_json::Error),
    MalformedLine {
        line: usize,
        source: serde_json::Error,
    },
}

impl fmt::Display for StrideLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrideLogError::Io(source) => write!(f, "stride log io error: {source}"),
            StrideLogError::Serialize(source) => {
                write!(f, "stride log serialization error: {source}")
            }
            StrideLogError::MalformedLine { line, source } => {
                write!(f, "malformed stride log line {line}: {source}")
            }
        }
    }
}

impl std::error::Error for StrideLogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StrideLogError::Io(source) => Some(source),
            StrideLogError::Serialize(source) => Some(source),
            StrideLogError::MalformedLine { source, .. } => Some(source),
        }
    }
}

impl From<io::Error> for StrideLogError {
    fn from(source: io::Error) -> Self {
        StrideLogError::Io(source)
    }
}

pub fn append_milestone(path: &Path, milestone: &Milestone) -> Result<(), StrideLogError> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let json = serde_json::to_string(milestone).map_err(StrideLogError::Serialize)?;
    writeln!(file, "{json}")?;
    Ok(())
}

pub fn read_milestones(path: &Path) -> Result<Vec<Milestone>, StrideLogError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(StrideLogError::Io(error)),
    };
    let mut milestones = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        let milestone =
            serde_json::from_str(&line).map_err(|source| StrideLogError::MalformedLine {
                line: index + 1,
                source,
            })?;
        milestones.push(milestone);
    }
    Ok(milestones)
}

pub fn fold_milestones(milestones: Vec<Milestone>) -> Vec<Milestone> {
    let mut by_id = BTreeMap::<String, Milestone>::new();
    for milestone in milestones {
        match by_id.get(&milestone.id) {
            Some(existing) if !prefer_fold_candidate(existing, &milestone) => {}
            _ => {
                by_id.insert(milestone.id.clone(), milestone);
            }
        }
    }
    let mut folded = by_id.into_values().collect::<Vec<_>>();
    folded.sort_by(|left, right| (left.ts, &left.id).cmp(&(right.ts, &right.id)));
    folded
}

fn prefer_fold_candidate(existing: &Milestone, candidate: &Milestone) -> bool {
    let existing_key = (
        existing.ts,
        serde_json::to_string(existing).unwrap_or_default(),
    );
    let candidate_key = (
        candidate.ts,
        serde_json::to_string(candidate).unwrap_or_default(),
    );
    candidate_key < existing_key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{AgentRef, ElementCanvas};

    fn milestone(id: &str, ts: u64, text: &str) -> Milestone {
        Milestone {
            id: id.into(),
            from: AgentRef::from_pane("pane-1"),
            kind: "stride".into(),
            text: text.into(),
            ts,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: ElementCanvas::default(),
        }
    }

    #[test]
    fn append_and_read_milestones_as_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task-strides.jsonl");

        let first = milestone("m-1", 20, "first");
        let second = milestone("m-2", 10, "second");
        append_milestone(&path, &first).unwrap();
        append_milestone(&path, &second).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), 2);
        assert_eq!(read_milestones(&path).unwrap(), vec![first, second]);
    }

    #[test]
    fn reading_missing_log_returns_empty_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.jsonl");

        assert_eq!(read_milestones(&path).unwrap(), Vec::<Milestone>::new());
    }

    #[test]
    fn fold_dedupes_duplicate_ids() {
        let earlier = milestone("m-1", 20, "earlier");
        let later = milestone("m-1", 40, "later");
        let other = milestone("m-2", 30, "other");

        assert_eq!(
            fold_milestones(vec![later, other.clone(), earlier.clone()]),
            vec![earlier, other]
        );
    }

    #[test]
    fn fold_orders_deterministically_by_timestamp_then_id() {
        let middle_b = milestone("m-b", 10, "middle b");
        let first = milestone("m-a", 5, "first");
        let middle_a = milestone("m-a2", 10, "middle a");
        let last = milestone("m-c", 30, "last");

        let folded = fold_milestones(vec![
            middle_b.clone(),
            last.clone(),
            first.clone(),
            middle_a.clone(),
        ]);

        assert_eq!(folded, vec![first, middle_a, middle_b, last]);
    }

    #[test]
    fn fold_is_stable_for_different_union_orders() {
        let duplicate_a = milestone("m-1", 20, "same id from branch a");
        let duplicate_b = milestone("m-1", 10, "same id from branch b");
        let other = milestone("m-2", 15, "other");

        let left_first = fold_milestones(vec![
            duplicate_a.clone(),
            other.clone(),
            duplicate_b.clone(),
        ]);
        let right_first = fold_milestones(vec![duplicate_b, other, duplicate_a]);

        assert_eq!(left_first, right_first);
        assert_eq!(left_first.len(), 2);
        assert_eq!(left_first[0].id, "m-1");
    }

    #[test]
    fn malformed_line_reports_line_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("malformed.jsonl");
        append_milestone(&path, &milestone("m-1", 1, "valid")).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{bad json}\n")
            .unwrap();

        let error = read_milestones(&path).unwrap_err();

        match error {
            StrideLogError::MalformedLine { line, .. } => assert_eq!(line, 2),
            other => panic!("expected malformed line error, got {other:?}"),
        }
    }
}
