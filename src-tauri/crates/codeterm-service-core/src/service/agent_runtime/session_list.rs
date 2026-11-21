use super::engine;
use crate::agent::provider::SessionMetadata;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{RuntimeOperation, RuntimeResult};
use std::time::Duration;

fn parse_iso_unix_secs(raw: Option<&str>) -> Option<i64> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(ts) = raw.parse::<i64>() {
        return Some(if ts > 1_000_000_000_000 { ts / 1000 } else { ts });
    }
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

fn normalized_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

pub async fn list_live_sessions(
    svc: &ServiceContext,
    project_path: Option<&str>,
    provider_filter: Option<&str>,
) -> Vec<SessionMetadata> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let target_cwd = project_path.map(normalized_path);

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
        let cwd = project_path
            .map(str::to_string)
            .unwrap_or_else(|| session.context.lock().cwd.clone());
        let reply = tokio::time::timeout(
            Duration::from_secs(3),
            engine::run(
                svc,
                &session.provider,
                &session,
                &process,
                RuntimeOperation::Control {
                    thread_id: None,
                    action: "listSessions".into(),
                    value: serde_json::json!({ "cwd": cwd }),
                },
            ),
        )
        .await;
        if let Ok(Ok(RuntimeResult::SessionsListed { sessions, .. })) = reply {
            for item in sessions {
                if let Some(ref want) = target_cwd {
                    if normalized_path(&item.cwd) != *want {
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
                    project_path: Some(item.cwd),
                    title: item.title,
                    summary: None,
                    file_path: String::new(),
                    transcript_path: None,
                    created_ts: modified_ts,
                    modified_ts,
                });
            }
        }
    }
    results
}
