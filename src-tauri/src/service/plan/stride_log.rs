use crate::plan::Milestone;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub enum StrideLogError {
    Io(io::Error),
    Serialize(serde_json::Error),
}

impl fmt::Display for StrideLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrideLogError::Io(source) => write!(f, "stride log io error: {source}"),
            StrideLogError::Serialize(source) => {
                write!(f, "stride log serialization error: {source}")
            }
        }
    }
}

impl std::error::Error for StrideLogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StrideLogError::Io(source) => Some(source),
            StrideLogError::Serialize(source) => Some(source),
        }
    }
}

impl From<io::Error> for StrideLogError {
    fn from(source: io::Error) -> Self {
        StrideLogError::Io(source)
    }
}

pub fn append_milestone(path: &Path, milestone: &Milestone) -> Result<(), StrideLogError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let json = serde_json::to_string(milestone).map_err(StrideLogError::Serialize)?;
    append_line(&mut file, &json)?;
    Ok(())
}

// One O_APPEND write per line so concurrent prod/dev/worker appends cannot tear a JSONL record.
fn append_line(file: &mut File, json: &str) -> io::Result<()> {
    let mut buf = String::with_capacity(json.len() + 1);
    buf.push_str(json);
    buf.push('\n');
    file.write_all(buf.as_bytes())
}

pub fn rewrite_milestones(
    path: &Path,
    milestones: Vec<Milestone>,
) -> Result<Vec<Milestone>, StrideLogError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let folded = fold_milestones(milestones);
    let tmp = unique_tmp_path(path);
    let result = (|| -> Result<(), StrideLogError> {
        let mut file = File::create(&tmp)?;
        for milestone in &folded {
            let json = serde_json::to_string(milestone).map_err(StrideLogError::Serialize)?;
            append_line(&mut file, &json)?;
        }
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    Ok(folded)
}

// Per-writer unique tmp: a fixed name lets concurrent daemons clobber each other's tmp mid-write.
fn unique_tmp_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("strides.jsonl");
    path.with_file_name(format!(".{name}.{pid}.{seq}.tmp"))
}

pub fn read_milestones(path: &Path) -> Result<Vec<Milestone>, StrideLogError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(StrideLogError::Io(error)),
    };
    let mut milestones = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                log::warn!(
                    "[plan] skip unreadable stride line {} in {}: {error}",
                    index + 1,
                    path.display()
                );
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(milestone) => milestones.push(milestone),
            Err(source) => {
                log::warn!(
                    "[plan] skip malformed stride line {} in {}: {source}",
                    index + 1,
                    path.display()
                );
            }
        }
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
            attachments: vec![],
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
    fn rewrite_milestones_dedupes_and_orders_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/strides.jsonl");
        let later = milestone("m-1", 40, "later");
        let earlier = milestone("m-1", 20, "earlier");
        let other = milestone("m-2", 30, "other");

        let written =
            rewrite_milestones(&path, vec![later, other.clone(), earlier.clone()]).unwrap();

        assert_eq!(written, vec![earlier, other]);
        assert_eq!(read_milestones(&path).unwrap(), written);
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
    fn malformed_line_is_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("malformed.jsonl");
        let valid = milestone("m-1", 1, "valid");
        append_milestone(&path, &valid).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{bad json}\n")
            .unwrap();

        assert_eq!(read_milestones(&path).unwrap(), vec![valid]);
    }

    // Mirrors the live incident: a torn whitespace-only line + a truncated line
    // (the strides.jsonl.corrupt.bak shape) must not drop the surviving records.
    #[test]
    fn corrupt_bak_shape_recovers_intact_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("strides.jsonl");
        let first = milestone("m-1", 10, "before tear");
        let second = milestone("m-2", 20, "after tear");
        let first_json = serde_json::to_string(&first).unwrap();
        let second_json = serde_json::to_string(&second).unwrap();
        let truncated = &first_json[..first_json.len() / 2];
        let raw = format!("{first_json}\n   \n{truncated}\n{second_json}\n");
        std::fs::write(&path, raw).unwrap();

        assert_eq!(read_milestones(&path).unwrap(), vec![first, second]);
    }

    #[test]
    fn concurrent_appends_never_tear_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("strides.jsonl");
        std::fs::File::create(&path).unwrap();

        let threads: Vec<_> = (0..8)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..50 {
                        let id = format!("m-{t}-{i}");
                        append_milestone(&path, &milestone(&id, (t * 100 + i) as u64, "x")).unwrap();
                    }
                })
            })
            .collect();
        for handle in threads {
            handle.join().unwrap();
        }

        let raw = std::fs::read_to_string(&path).unwrap();
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            serde_json::from_str::<Milestone>(line)
                .unwrap_or_else(|e| panic!("torn line {line:?}: {e}"));
        }
        assert_eq!(read_milestones(&path).unwrap().len(), 8 * 50);
    }

    #[test]
    fn concurrent_rewrites_use_disjoint_tmp_paths() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("strides.jsonl");
        let a = unique_tmp_path(&base);
        let b = unique_tmp_path(&base);
        assert_ne!(a, b);
        assert_ne!(a, base);
        assert!(a.file_name().unwrap().to_str().unwrap().ends_with(".tmp"));
    }
}
