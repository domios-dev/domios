use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::general_dir;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeneralAgentSessionState {
    pub(super) provider: String,
    pub(super) session_id: String,
    #[serde(default)]
    pub(super) by_provider: std::collections::BTreeMap<String, String>,
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
    load_session_record(registry, provider_name, cwd, &[]).map(|state| state.session_id)
}

pub(super) fn load_session_record(
    registry: &crate::agent::registry::AgentRegistry,
    provider_name: &str,
    cwd: &str,
    excluded: &[String],
) -> Option<GeneralAgentSessionState> {
    load_session_record_from(
        registry,
        provider_name,
        cwd,
        excluded,
        &session_state_path(),
    )
}

pub(super) fn load_session_record_from(
    registry: &crate::agent::registry::AgentRegistry,
    provider_name: &str,
    cwd: &str,
    excluded: &[String],
    path: &Path,
) -> Option<GeneralAgentSessionState> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut state: GeneralAgentSessionState = serde_json::from_str(&text).ok()?;
    let session_id = if state.provider == provider_name {
        state.session_id.clone()
    } else {
        state.by_provider.get(provider_name)?.clone()
    };
    if session_id.is_empty() || excluded.contains(&session_id) {
        return None;
    }
    let valid = registry
        .with_provider(provider_name, |p| {
            can_resume_saved_session(p.agent_runtime().is_some(), || {
                p.session_exists(cwd, &session_id)
            })
        })
        .unwrap_or(false);
    if !valid {
        return None;
    }
    state.provider = provider_name.to_string();
    state.session_id = session_id;
    Some(state)
}

fn can_resume_saved_session(runtime: bool, session_exists: impl FnOnce() -> Option<bool>) -> bool {
    runtime || session_exists() == Some(true)
}

pub(super) fn persist_session_id(provider_name: &str, session_id: &str) {
    persist_session_id_to(&session_state_path(), provider_name, session_id);
}

pub(super) fn persist_session_id_to(path: &Path, provider_name: &str, session_id: &str) {
    let mut by_provider = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<GeneralAgentSessionState>(&text).ok())
        .map(|mut existing| {
            if !existing.provider.is_empty() && !existing.session_id.is_empty() {
                existing
                    .by_provider
                    .entry(existing.provider)
                    .or_insert(existing.session_id);
            }
            existing.by_provider
        })
        .unwrap_or_default();
    by_provider.insert(provider_name.to_string(), session_id.to_string());
    let state = GeneralAgentSessionState {
        provider: provider_name.to_string(),
        session_id: session_id.to_string(),
        by_provider,
    };
    write_session_state(path, &state);
}

fn write_session_state(path: &Path, state: &GeneralAgentSessionState) {
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
    let Ok(body) = serde_json::to_string_pretty(state) else {
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
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        log::warn!(
            "[general-agent] failed to commit session state {}: {}",
            path.display(),
            e
        );
    }
}

pub(super) fn has_recorded_session() -> bool {
    has_recorded_session_in(&general_dir())
}

pub(super) fn has_recorded_session_in(dir: &Path) -> bool {
    let path = dir.join("state").join("general-agent-session.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<GeneralAgentSessionState>(&text).ok())
        .is_some_and(|state| {
            !state.session_id.is_empty() || state.by_provider.values().any(|id| !id.is_empty())
        })
}

fn bootstrap_dispatched_path_in(dir: &Path) -> PathBuf {
    dir.join("state").join("bootstrap-dispatched")
}

pub(super) fn bootstrap_dispatched_in(dir: &Path) -> bool {
    bootstrap_dispatched_path_in(dir).is_file()
}

pub(super) fn mark_bootstrap_dispatched() {
    mark_bootstrap_dispatched_in(&general_dir());
}

pub(super) fn mark_bootstrap_dispatched_in(dir: &Path) {
    let path = bootstrap_dispatched_path_in(dir);
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[general-agent] failed to create bootstrap state dir {}: {error}",
                parent.display()
            );
            return;
        }
    }
    if let Err(error) = std::fs::write(&path, b"dispatched") {
        log::warn!(
            "[general-agent] failed to persist bootstrap marker {}: {error}",
            path.display()
        );
    }
}

pub(super) fn clear_bootstrap_dispatched() {
    clear_bootstrap_dispatched_in(&general_dir());
}

pub(super) fn clear_bootstrap_dispatched_in(dir: &Path) {
    let path = bootstrap_dispatched_path_in(dir);
    if let Err(error) = std::fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "[general-agent] failed to remove bootstrap marker {}: {error}",
                path.display()
            );
        }
    }
}

pub(super) fn clear_all_session_ids() {
    clear_all_session_ids_at(&session_state_path());
}

fn clear_all_session_ids_at(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "[general-agent] failed to remove session state {}: {}",
                path.display(),
                e
            );
        }
    }
}

pub(super) fn clear_session_id_for_provider(provider_name: &str) {
    clear_session_id_for_provider_in(&session_state_path(), provider_name);
}

pub(super) fn clear_session_id_for_provider_in(path: &Path, provider_name: &str) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(mut state) = serde_json::from_str::<GeneralAgentSessionState>(&text) else {
        return;
    };
    if !state.provider.is_empty() && !state.session_id.is_empty() {
        state
            .by_provider
            .entry(state.provider.clone())
            .or_insert_with(|| state.session_id.clone());
    }
    state.by_provider.remove(provider_name);
    if state.provider == provider_name {
        state.provider.clear();
        state.session_id.clear();
    }
    if state.session_id.is_empty() && state.by_provider.is_empty() {
        clear_all_session_ids_at(path);
    } else {
        write_session_state(path, &state);
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

pub(super) fn prepare_with_fresh_session_requirement<T>(
    marker: &Path,
    prepare: impl FnOnce(bool) -> Result<T, String>,
) -> Result<T, String> {
    let fresh = marker
        .try_exists()
        .map_err(|error| format!("Cannot read General's fresh-session request: {error}"))?;
    let prepared = prepare(fresh)?;
    if fresh {
        std::fs::remove_file(marker)
            .map_err(|error| format!("Cannot clear General's fresh-session request: {error}"))?;
    }
    Ok(prepared)
}

pub(super) fn detect_owned_session_id(
    registry: &crate::agent::registry::AgentRegistry,
    provider_name: &str,
    cwd: &str,
    exclude: &[String],
    max_age: Option<Duration>,
) -> Option<String> {
    discover_main_session(independent_sessions_path().exists(), || {
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
    })
}

pub(super) fn mark_independent_sessions() -> Result<(), String> {
    std::fs::write(independent_sessions_path(), b"").map_err(|error| error.to_string())
}

fn independent_sessions_path() -> PathBuf {
    general_dir().join("state").join("independent-sessions")
}

fn discover_main_session(
    has_independent_sessions: bool,
    discover: impl FnOnce() -> Option<String>,
) -> Option<String> {
    // Siblings share Main's prompt and home, so those cannot establish Main ownership.
    if has_independent_sessions {
        None
    } else {
        discover()
    }
}

pub(super) fn session_id_is_owned(
    registry: &crate::agent::registry::AgentRegistry,
    provider_name: &str,
    cwd: &str,
    session_id: &str,
    (bound_provider, bound_session): (Option<&str>, Option<&str>),
    durable: Option<&GeneralAgentSessionState>,
) -> bool {
    let bound = bound_provider == Some(provider_name) && bound_session == Some(session_id);
    let saved = durable
        .is_some_and(|state| state.provider == provider_name && state.session_id == session_id);
    if !bound && !saved {
        return false;
    }
    registry
        .with_provider(provider_name, |p| {
            p.session_exists(cwd, session_id) == Some(true)
        })
        .unwrap_or(false)
}

pub(super) fn session_state_path() -> std::path::PathBuf {
    general_dir()
        .join("state")
        .join("general-agent-session.json")
}

pub(super) fn fresh_session_path() -> std::path::PathBuf {
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

pub(super) fn first_seen_at(dir: &Path, now: u64) -> Result<u64, String> {
    static WRITE_LOCK: parking_lot::Mutex<()> = parking_lot::const_mutex(());
    let _guard = WRITE_LOCK.lock();
    let state = dir.join("state");
    std::fs::create_dir_all(&state).map_err(|error| error.to_string())?;
    let path = state.join("first-seen");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            write!(file, "{now}").map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            Ok(now)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::read_to_string(path)
                .map_err(|error| error.to_string())?
                .parse::<u64>()
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests;
