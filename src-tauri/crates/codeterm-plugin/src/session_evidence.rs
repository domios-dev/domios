//! Provider-owned session evidence: transcript paths reported by agent hooks.
//!
//! SessionStart ingestion records the hook-reported transcript path here, and
//! transcript readers prefer it over id-based path reconstruction (a guess
//! about vendor file naming). Absence of a record keeps reconstruction, so
//! ceasing to persist reverts every reader automatically. The rule is
//! provider-neutral: records are keyed by the provider-reported session id.

use std::path::{Path, PathBuf};

/// Test/instance override for the on-disk store location.
pub const EVIDENCE_DIR_ENV: &str = "CODETERM_TRANSCRIPT_EVIDENCE_DIR";

fn evidence_dir() -> PathBuf {
    match std::env::var(EVIDENCE_DIR_ENV) {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => codeterm_paths::data_dir().join("session-evidence"),
    }
}

/// Record keys are provider session ids; anything path-like must not become a
/// store filename.
fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn record_path(session_id: &str) -> Option<PathBuf> {
    valid_session_id(session_id).then(|| evidence_dir().join(format!("{session_id}.json")))
}

/// Normalize a hook-reported host-native path for storage: Windows verbatim
/// prefix stripped, separators forward-slashed.
pub fn normalize_reported_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(codeterm_paths::strip_verbatim_prefix(trimmed).replace('\\', "/"))
}

/// Persist the provider-reported transcript path for `session_id`.
/// Returns false when the id or path is unusable or the write fails.
pub fn record(session_id: &str, reported_path: &str) -> bool {
    let Some(normalized) = normalize_reported_path(reported_path) else {
        return false;
    };
    let Some(path) = record_path(session_id) else {
        return false;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let body = serde_json::json!({ "transcript_path": normalized });
    codeterm_util::atomic_write::write_atomic(&path, body.to_string()).is_ok()
}

/// The stored transcript path for `session_id`, if evidence was ever recorded.
/// `None` means "no evidence" — callers must fall back to reconstruction.
pub fn stored_transcript_path(session_id: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(record_path(session_id)?).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stored = value.get("transcript_path")?.as_str()?.trim();
    (!stored.is_empty()).then(|| PathBuf::from(stored))
}

/// Existence check for an evidenced transcript. Storage is forward-slashed and
/// verbatim-stripped; a stored path past MAX_PATH may still need the Windows
/// extended-length form to stat, so retry with the `\\?\` prefix.
pub fn transcript_exists(path: &Path) -> bool {
    if path.exists() {
        return true;
    }
    if cfg!(windows) {
        let native = path.to_string_lossy().replace('/', "\\");
        if !native.starts_with(r"\\") && native.contains(':') {
            return Path::new(&format!(r"\\?\{native}")).exists();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_util::test_env::EnvGuard;

    #[test]
    fn transcript_path_record_round_trips_normalized() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::lock();
        env.set(EVIDENCE_DIR_ENV, tmp.path().join("store"));

        let sid = "evidence-round-trip-sid";
        assert!(record(sid, r"\\?\C:\Users\u\.claude\projects\p\drifted.jsonl"));
        assert_eq!(
            stored_transcript_path(sid),
            Some(PathBuf::from("C:/Users/u/.claude/projects/p/drifted.jsonl")),
            "verbatim prefix stripped and separators normalized before storage",
        );
    }

    #[test]
    fn transcript_path_store_rejects_pathlike_session_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::lock();
        env.set(EVIDENCE_DIR_ENV, tmp.path().join("store"));

        assert!(!record("../escape", "/tmp/x.jsonl"));
        assert!(!record("a/b", "/tmp/x.jsonl"));
        assert!(!record("", "/tmp/x.jsonl"));
        assert!(!record("sid-ok", "   "));
        assert_eq!(stored_transcript_path("../escape"), None);
    }

    #[test]
    fn transcript_path_absent_evidence_reads_none() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::lock();
        env.set(EVIDENCE_DIR_ENV, tmp.path().join("empty-store"));
        assert_eq!(stored_transcript_path("never-recorded-sid"), None);
    }

    #[test]
    fn transcript_path_exists_check_handles_forward_slash_storage() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("transcript.jsonl");
        std::fs::write(&real, "{}\n").unwrap();

        let forward = real.to_string_lossy().replace('\\', "/");
        assert!(transcript_exists(Path::new(&forward)));
        assert!(!transcript_exists(&tmp.path().join("missing.jsonl")));
    }
}
