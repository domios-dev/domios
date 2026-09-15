//! Sub-agent transcripts Claude writes beside a session: `<session>/subagents/agent-*.jsonl`.

use std::path::{Path, PathBuf};

pub(super) fn usage_sources(session_file: &Path, session_id: &str) -> Vec<PathBuf> {
    let named_for_session = session_file
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == format!("{session_id}.jsonl"));
    if !is_plain_id(session_id) || !named_for_session {
        return Vec::new();
    }
    let Some(parent) = session_file.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent.join(session_id).join("subagents")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_agent_log_name)
        })
        .collect();
    paths.sort();
    paths
}

fn is_plain_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_agent_log_name(name: &str) -> bool {
    name.strip_prefix("agent-")
        .and_then(|rest| rest.strip_suffix(".jsonl"))
        .is_some_and(is_plain_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}\n").unwrap();
    }

    #[test]
    fn lists_only_this_sessions_agent_logs() {
        let tmp = tempfile::tempdir().unwrap();
        let session = tmp.path().join("own-session.jsonl");
        touch(&session);
        let own = tmp.path().join("own-session").join("subagents");
        touch(&own.join("agent-b.jsonl"));
        touch(&own.join("agent-a.jsonl"));
        touch(&own.join("agent-a.meta.json"));
        touch(&own.join("notes.jsonl"));
        touch(&tmp.path().join("foreign-session").join("subagents").join("agent-x.jsonl"));

        assert_eq!(
            usage_sources(&session, "own-session"),
            vec![own.join("agent-a.jsonl"), own.join("agent-b.jsonl")]
        );
    }

    #[test]
    fn absent_directory_or_foreign_session_file_yields_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let session = tmp.path().join("own-session.jsonl");
        touch(&session);
        assert!(usage_sources(&session, "own-session").is_empty());

        touch(&tmp.path().join("other-session").join("subagents").join("agent-x.jsonl"));
        assert!(usage_sources(&session, "other-session").is_empty());
        assert!(usage_sources(&session, "../other-session").is_empty());
    }
}
