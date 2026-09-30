use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::general_dir;

#[derive(Debug, Serialize, Deserialize)]
struct GeneralAgentSessionState {
    provider: String,
    session_id: String,
}

struct SessionCandidate {
    session_id: String,
    file_path: String,
    sort_ts: i64,
}

pub(super) fn load_session_id(
    registry: &crate::agent::registry::AgentRegistry,
    provider_name: &str,
    cwd: &str,
) -> Option<String> {
    let text = std::fs::read_to_string(session_state_path()).ok()?;
    let state: GeneralAgentSessionState = serde_json::from_str(&text).ok()?;
    if state.provider != provider_name || state.session_id.is_empty() {
        return None;
    }
    let valid = registry
        .with_provider(provider_name, |p| p.session_exists(cwd, &state.session_id))
        .unwrap_or(false);
    valid.then_some(state.session_id)
}

pub(super) fn persist_session_id(provider_name: &str, session_id: &str) {
    let state = GeneralAgentSessionState {
        provider: provider_name.to_string(),
        session_id: session_id.to_string(),
    };
    let path = session_state_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[general-agent] failed to create session state dir {}: {}",
                parent.display(),
                e
            );
            return;
        }
    }
    let tmp = path.with_extension("json.tmp");
    let Ok(body) = serde_json::to_string_pretty(&state) else {
        return;
    };
    if let Err(e) = std::fs::write(&tmp, body) {
        log::warn!(
            "[general-agent] failed to write session state {}: {}",
            tmp.display(),
            e
        );
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        log::warn!(
            "[general-agent] failed to commit session state {}: {}",
            path.display(),
            e
        );
    }
}

pub(super) fn clear_session_id() {
    let path = session_state_path();
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "[general-agent] failed to remove session state {}: {}",
                path.display(),
                e
            );
        }
    }
}

pub(super) fn require_fresh_session() -> Result<(), String> {
    let path = fresh_session_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, b"fresh").map_err(|error| error.to_string())
}

pub(super) fn take_fresh_session_requirement() -> bool {
    let path = fresh_session_path();
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            log::warn!("[general-agent] failed to consume fresh-session intent: {error}");
            false
        }
    }
}

pub(super) fn detect_owned_session_id(
    registry: &crate::agent::registry::AgentRegistry,
    provider_name: &str,
    cwd: &str,
    exclude: &[String],
    max_age: Option<Duration>,
) -> Option<String> {
    let mut candidates = registry
        .with_provider(provider_name, |p| {
            p.enumerate_sessions()
                .into_iter()
                .filter(|s| s.provider == provider_name)
                .filter(|s| !exclude.contains(&s.session_id))
                .filter(|s| session_matches_cwd(s.project_path.as_deref(), cwd))
                .filter_map(|s| {
                    let sort_ts = s.modified_ts.or(s.created_ts)?;
                    if !within_age(sort_ts, max_age) {
                        return None;
                    }
                    Some(SessionCandidate {
                        session_id: s.session_id,
                        file_path: s.file_path,
                        sort_ts,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    pick_general_agent_session(&mut candidates)
}

pub(super) fn session_id_is_owned(
    registry: &crate::agent::registry::AgentRegistry,
    provider_name: &str,
    cwd: &str,
    session_id: &str,
) -> bool {
    registry
        .with_provider(provider_name, |p| p.session_file_path(cwd, session_id))
        .flatten()
        .map(|path| is_general_agent_session_file(&path))
        .unwrap_or(false)
}

fn session_state_path() -> std::path::PathBuf {
    general_dir()
        .join("state")
        .join("general-agent-session.json")
}

fn fresh_session_path() -> std::path::PathBuf {
    general_dir().join("state").join("force-fresh-session")
}

fn session_matches_cwd(session_cwd: Option<&str>, cwd: &str) -> bool {
    session_cwd
        .map(|p| crate::paths::canonical_cwd(p) == crate::paths::canonical_cwd(cwd))
        .unwrap_or(false)
}

fn within_age(unix_ts: i64, max_age: Option<Duration>) -> bool {
    let Some(max_age) = max_age else {
        return true;
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return true;
    };
    now.as_secs().saturating_sub(unix_ts.max(0) as u64) <= max_age.as_secs()
}

fn pick_general_agent_session(candidates: &mut [SessionCandidate]) -> Option<String> {
    pick_general_agent_session_with(candidates, is_general_agent_session_file)
}

fn pick_general_agent_session_with(
    candidates: &mut [SessionCandidate],
    mut is_general_agent: impl FnMut(&Path) -> bool,
) -> Option<String> {
    candidates.sort_by(|a, b| b.sort_ts.cmp(&a.sort_ts));
    let mut scanned_paths = HashSet::new();
    candidates.iter().find_map(|candidate| {
        let path = Path::new(&candidate.file_path);
        let identity = path.canonicalize().unwrap_or_else(|_| PathBuf::from(path));
        if !scanned_paths.insert(identity) {
            return None;
        }
        is_general_agent(path).then(|| candidate.session_id.clone())
    })
}

/// Cap on how much of a session transcript is scanned for the markers below.
const MARKER_SCAN_LIMIT: usize = 512 * 1024;

fn is_general_agent_session_file(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    session_file_has_ga_marker(file)
}

fn session_file_has_ga_marker(mut file: impl Read) -> bool {
    let mut bytes = Vec::with_capacity(MARKER_SCAN_LIMIT);
    if file.by_ref().take(16).read_to_end(&mut bytes).is_err() {
        return false;
    }
    if bytes.starts_with(b"SQLite format 3\0") {
        return false;
    }
    if file
        .take((MARKER_SCAN_LIMIT - bytes.len()) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    let valid_len = std::str::from_utf8(&bytes)
        .map(|text| text.len())
        .unwrap_or_else(|error| error.valid_up_to());
    let text = String::from_utf8(bytes[..valid_len].to_vec()).unwrap_or_default();
    let markers = [
        "# CodeTerm General Agent",
        "You are the General Agent",
        "always-on system brain",
        "<!-- bootstrap: true -->",
    ];
    markers.iter().any(|marker| text.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn picks_ga_marked_session_over_newer_worker_session() {
        let dir = tempfile::tempdir().unwrap();
        let ga = dir.path().join("ga.jsonl");
        let worker = dir.path().join("worker.jsonl");
        write_jsonl(
            &ga,
            r##"{"type":"system","message":{"content":"# CodeTerm General Agent (v1)\nYou are the General Agent - the always-on system brain."}}"##,
        );
        write_jsonl(
            &worker,
            r#"{"type":"user","message":{"content":"codeterm agent report haiku facts done\nGPU facts"}}"#,
        );

        let mut candidates = vec![
            SessionCandidate {
                session_id: "ga-session".into(),
                file_path: ga.to_string_lossy().to_string(),
                sort_ts: 10,
            },
            SessionCandidate {
                session_id: "worker-session".into(),
                file_path: worker.to_string_lossy().to_string(),
                sort_ts: 20,
            },
        ];

        assert_eq!(
            pick_general_agent_session(&mut candidates).as_deref(),
            Some("ga-session")
        );
    }

    #[test]
    fn rejects_worker_only_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("worker.jsonl");
        write_jsonl(
            &worker,
            r#"{"type":"user","message":{"content":"codeterm agent report haiku facts done\nGPU facts"}}"#,
        );

        let mut candidates = vec![SessionCandidate {
            session_id: "worker-session".into(),
            file_path: worker.to_string_lossy().to_string(),
            sort_ts: 20,
        }];

        assert_eq!(pick_general_agent_session(&mut candidates), None);
    }

    #[test]
    fn scans_oversized_session_with_multibyte_text_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let ga = dir.path().join("ga.jsonl");

        // Pad so a Cyrillic codepoint straddles MARKER_SCAN_LIMIT, leaving a
        // UTF-8 continuation byte at the limit — what used to panic startup.
        let mut line =
            String::from(r##"{"type":"system","message":{"content":"# CodeTerm General Agent"}}"##);
        line.push_str(&"a".repeat(MARKER_SCAN_LIMIT - line.len() - 1));
        line.push('я');
        line.push_str(&"b".repeat(1024));
        assert!(line.len() > MARKER_SCAN_LIMIT);
        assert!(!line.is_char_boundary(MARKER_SCAN_LIMIT));
        write_jsonl(&ga, &line);

        let mut candidates = vec![SessionCandidate {
            session_id: "ga-session".into(),
            file_path: ga.to_string_lossy().to_string(),
            sort_ts: 10,
        }];

        assert_eq!(
            pick_general_agent_session(&mut candidates).as_deref(),
            Some("ga-session")
        );
    }

    #[test]
    fn general_agent_marker_scan_reads_at_most_limit() {
        let mut bytes = vec![b'x'; 3 * 1024 * 1024];
        bytes.extend_from_slice(b"# CodeTerm General Agent");
        let mut reader = std::io::Cursor::new(bytes);
        assert!(!session_file_has_ga_marker(&mut reader));
        assert_eq!(reader.position(), MARKER_SCAN_LIMIT as u64);
    }

    #[test]
    fn general_agent_scan_skips_sqlite_and_repeated_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.db");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"SQLite format 3\0# CodeTerm General Agent")
            .unwrap();
        let mut candidates: Vec<_> = (0..5)
            .map(|index| SessionCandidate {
                session_id: format!("session-{index}"),
                file_path: path.to_string_lossy().into_owned(),
                sort_ts: 5 - index,
            })
            .collect();
        let mut scans = 0;
        let selected = pick_general_agent_session_with(&mut candidates, |candidate_path| {
            scans += 1;
            is_general_agent_session_file(candidate_path)
        });
        assert_eq!(selected, None);
        assert_eq!(scans, 1);
    }

    fn write_jsonl(path: &Path, line: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        writeln!(file, "{line}").unwrap();
    }
}
