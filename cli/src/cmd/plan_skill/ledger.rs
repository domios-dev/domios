use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::store::{skills_root, valid_verdict};

// milestone of kind "skill" with a verdict and a one-line rationale.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LedgerEntry {
    pub seq: u64,
    pub kind: String,
    pub slug: String,
    pub verdict: String,
    pub rationale: String,
}

fn ledger_path(plan_dir: &Path) -> std::path::PathBuf {
    skills_root(plan_dir).join("ledger.jsonl")
}

pub fn read_ledger(plan_dir: &Path) -> Vec<LedgerEntry> {
    let path = ledger_path(plan_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<LedgerEntry>(line).ok())
        .collect()
}

pub fn append_entry(
    plan_dir: &Path,
    slug: &str,
    verdict: &str,
    rationale: &str,
) -> Result<LedgerEntry, String> {
    if !valid_verdict(verdict) {
        return Err(format!(
            "invalid verdict '{verdict}'; expected keep | discard | superseded"
        ));
    }
    let rationale = rationale.trim().replace(['\n', '\r'], " ");
    if rationale.is_empty() {
        return Err("rationale must be a non-empty one-line note".to_string());
    }
    let next_seq = read_ledger(plan_dir)
        .last()
        .map(|entry| entry.seq + 1)
        .unwrap_or(1);
    let entry = LedgerEntry {
        seq: next_seq,
        kind: "skill".to_string(),
        slug: slug.to_string(),
        verdict: verdict.to_string(),
        rationale,
    };
    let root = skills_root(plan_dir);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let mut line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger_path(plan_dir))
        .map_err(|e| e.to_string())?;
    // Single O_APPEND write: concurrent writers cannot interleave a torn JSONL line.
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_is_monotonic_and_append_only() {
        let tmp = tempfile::tempdir().unwrap();
        let plan_dir = tmp.path();
        let e1 = append_entry(plan_dir, "deploy", "keep", "created").unwrap();
        let e2 = append_entry(plan_dir, "deploy", "superseded", "replaced by v2").unwrap();
        assert_eq!(e1.seq, 1);
        assert_eq!(e2.seq, 2);
        let all = read_ledger(plan_dir);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0], e1);
        assert_eq!(all[1].kind, "skill");
    }

    #[test]
    fn rejects_bad_verdict() {
        let tmp = tempfile::tempdir().unwrap();
        let err = append_entry(tmp.path(), "x", "nope", "why").unwrap_err();
        assert!(err.contains("invalid verdict"));
    }

    #[test]
    fn rejects_empty_rationale() {
        let tmp = tempfile::tempdir().unwrap();
        let err = append_entry(tmp.path(), "x", "keep", "   ").unwrap_err();
        assert!(err.contains("one-line"));
    }

    #[test]
    fn rationale_is_flattened_to_one_line() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = append_entry(tmp.path(), "x", "keep", "line1\nline2").unwrap();
        assert_eq!(entry.rationale, "line1 line2");
    }
}
