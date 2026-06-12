use std::path::Path;
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
    (valid && session_id_is_owned(registry, provider_name, cwd, &state.session_id))
        .then_some(state.session_id)
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
    candidates.sort_by(|a, b| b.sort_ts.cmp(&a.sort_ts));
    candidates
        .iter()
        .find(|c| is_general_agent_session_file(Path::new(&c.file_path)))
        .map(|c| c.session_id.clone())
}

fn is_general_agent_session_file(path: &Path) -> bool {
    let Ok(mut text) = std::fs::read_to_string(path) else {
        return false;
    };
    if text.len() > 512 * 1024 {
        text.truncate(512 * 1024);
    }
    let markers = [
        "# CodeTerm General Agent",
        "You are the General Agent",
        "always-on system brain",
        "init [Orient yourself:",
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

    fn write_jsonl(path: &Path, line: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        writeln!(file, "{line}").unwrap();
    }
}
