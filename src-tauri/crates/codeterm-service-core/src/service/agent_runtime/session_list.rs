use super::engine;
use crate::agent::provider::SessionMetadata;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeListedSession, RuntimeOperation, RuntimeResult};
use std::time::Duration;

fn parse_iso_unix_secs(raw: Option<&str>) -> Option<i64> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(ts) = raw.parse::<i64>() {
        return Some(if ts > 1_000_000_000_000 {
            ts / 1000
        } else {
            ts
        });
    }
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Normalizes Windows URI leading-slash drive paths (`/C:/...` -> `C:/...`),
/// backslashes, trailing slashes, and ASCII drive-letter casing.
pub fn normalize_workspace_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    if normalized.len() >= 3 {
        let bytes = normalized.as_bytes();
        if bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
            normalized.remove(0);
        }
    }
    if is_windows_drive_path(&normalized) {
        let drive = normalized.as_bytes()[0].to_ascii_uppercase() as char;
        normalized.replace_range(0..1, &drive.to_string());
    }
    while normalized.len() > 1
        && normalized.ends_with('/')
        && !(normalized.len() == 3 && is_windows_drive_path(&normalized))
    {
        normalized.pop();
    }
    normalized
}

pub fn workspace_paths_match(left: &str, right: &str) -> bool {
    let left = normalize_workspace_path(left);
    let right = normalize_workspace_path(right);
    if is_windows_drive_path(&left) && is_windows_drive_path(&right) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

async fn query_session_list(
    svc: &ServiceContext,
    session: &std::sync::Arc<engine::Session>,
    process: &std::sync::Arc<super::process::Process>,
    cwd: &str,
) -> Option<Vec<RuntimeListedSession>> {
    let reply = tokio::time::timeout(
        Duration::from_secs(3),
        engine::run(
            svc,
            &session.provider,
            session,
            process,
            RuntimeOperation::Control {
                thread_id: None,
                action: "listSessions".into(),
                value: serde_json::json!({ "cwd": cwd }),
            },
        ),
    )
    .await;
    match reply {
        Ok(Ok(RuntimeResult::SessionsListed { sessions, .. })) => Some(sessions),
        _ => None,
    }
}

pub async fn list_live_sessions(
    svc: &ServiceContext,
    project_path: Option<&str>,
    provider_filter: Option<&str>,
) -> Vec<SessionMetadata> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let target_cwd = project_path.map(normalize_workspace_path);

    for (_tab_id, session) in engine::all_sessions() {
        if let Some(filter) = provider_filter {
            if session.provider != filter {
                continue;
            }
        }
        let supports_list = session
            .state
            .lock()
            .session_capabilities
            .as_ref()
            .is_some_and(|caps| caps.list);
        if !supports_list {
            continue;
        }
        let Some(process) = session.process.lock().clone() else {
            continue;
        };
        let Ok(_permit) = session.gate.try_acquire() else {
            continue;
        };
        let raw_cwd = target_cwd
            .clone()
            .unwrap_or_else(|| normalize_workspace_path(&session.context.lock().cwd));
        let mut sessions = query_session_list(svc, &session, &process, &raw_cwd)
            .await
            .unwrap_or_default();
        if sessions.is_empty() && is_windows_drive_path(&raw_cwd) {
            let uri_cwd = format!("/{raw_cwd}");
            if let Some(fallback) = query_session_list(svc, &session, &process, &uri_cwd).await {
                sessions = fallback;
            }
        }
        for item in sessions {
            let item_cwd = normalize_workspace_path(&item.cwd);
            if let Some(ref want) = target_cwd {
                if !workspace_paths_match(&item_cwd, want) {
                    continue;
                }
            }
            if !seen.insert((session.provider.clone(), item.session_id.clone())) {
                continue;
            }
            let modified_ts = parse_iso_unix_secs(item.updated_at.as_deref());
            results.push(SessionMetadata {
                session_id: item.session_id,
                provider: session.provider.clone(),
                project_path: Some(item_cwd),
                title: item.title,
                summary: None,
                file_path: String::new(),
                transcript_path: None,
                created_ts: modified_ts,
                modified_ts,
            });
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::{normalize_workspace_path, workspace_paths_match};

    #[test]
    fn normalizes_windows_leading_slash_and_backslash_drive_paths() {
        assert_eq!(
            normalize_workspace_path("/C:/Developer/codeterm/codeterm"),
            "C:/Developer/codeterm/codeterm"
        );
        assert_eq!(
            normalize_workspace_path(r"C:\Developer\codeterm\codeterm\"),
            "C:/Developer/codeterm/codeterm"
        );
        assert_eq!(
            normalize_workspace_path("/c:/Developer/codeterm/codeterm/"),
            "C:/Developer/codeterm/codeterm"
        );
        assert_eq!(normalize_workspace_path("/work/project/"), "/work/project");
        assert_eq!(normalize_workspace_path("C:/"), "C:/");
    }

    #[test]
    fn matches_equivalent_windows_drive_path_forms() {
        let forms = [
            "C:/Developer/codeterm/codeterm",
            r"C:\Developer\codeterm\codeterm",
            "/C:/Developer/codeterm/codeterm",
            "/c:/Developer/codeterm/codeterm/",
        ];
        for left in forms {
            for right in forms {
                assert!(
                    workspace_paths_match(left, right),
                    "expected {left:?} to match {right:?}"
                );
            }
        }
        assert!(!workspace_paths_match(
            "C:/Developer/codeterm/codeterm",
            "C:/Developer/codeterm/other"
        ));
        assert!(!workspace_paths_match("/work/project", "/work/other"));
    }
}
