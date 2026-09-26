//! Codex launch-marker binding through the real `provider-codex/plugin.js`
//! (JS-only provider): the marker the launch command writes into CodeTerm's
//! instructions is the marker detection finds, and only that launch's rollout binds.

use super::*;
use crate::provider::{LaunchSessionDetection, LaunchSessionEvidence, LaunchSessionSource};

const MARKER: &str = "ct-launch-0123456789abcdef0123456789abcdef";
const INSTRUCTIONS_SUFFIX: &str = ".ct-task.md";

fn written_instructions(command: &str) -> String {
    let end = command
        .find(INSTRUCTIONS_SUFFIX)
        .expect("the launch command writes its instructions to a file")
        + INSTRUCTIONS_SUFFIX.len();
    let start = command[..end]
        .rfind('\'')
        .expect("the instructions path is quoted")
        + 1;
    std::fs::read_to_string(&command[start..end]).expect("instructions file")
}

fn rollout(id: &str, developer_text: &str) -> String {
    let lines = [
        serde_json::json!({"type": "session_meta", "payload": {"id": id, "cwd": "/work"}}),
        serde_json::json!({"type": "response_item", "payload": {
            "type": "message",
            "role": "developer",
            "content": [{"type": "input_text", "text": developer_text}],
        }}),
    ];
    lines
        .iter()
        .map(|line| format!("{line}\n"))
        .collect::<String>()
}

#[test]
fn launch_marker_binds_only_the_rollout_that_carries_it() {
    if require_plugins().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let state_db = dir.path().join("state_5.sqlite");
    let _env = EnvGuard::new(&[("CODETERM_CODEX_STATE_DB_PATH", state_db.to_str().unwrap())]);
    let codex = load_js("codex");

    let command = codex.build_launch_command(&AgentLaunchParams {
        task: Some("assigned task".into()),
        launch_marker: Some(MARKER.into()),
        instance_id: Some("instance-parity".into()),
        ..Default::default()
    });
    let instructions = written_instructions(&command);
    assert!(instructions.contains("assigned task"));
    assert!(instructions.contains(MARKER));

    {
        let db = rusqlite::Connection::open(&state_db).unwrap();
        db.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, \
             created_at INTEGER NOT NULL, model TEXT, cwd TEXT);",
        )
        .unwrap();
        let add = |id: &str, created_at: i64, text: &str| {
            let path = dir.path().join(format!("rollout-{id}.jsonl"));
            std::fs::write(&path, rollout(id, text)).unwrap();
            db.execute(
                "INSERT INTO threads (id, rollout_path, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, path.to_str().unwrap(), created_at],
            )
            .unwrap();
        };
        add("thread-own", 1_789_000_010, &instructions);
        add("thread-unmarked", 1_789_000_020, "assigned task");
        add("thread-before-launch", 1_788_999_000, &instructions);
    }

    let evidence = |marker: Option<&str>| LaunchSessionEvidence {
        cwd: "/work".into(),
        launch_marker: marker.map(str::to_string),
        launched_at_ms: 1_789_000_000_000,
        processes: Vec::new(),
    };
    assert_eq!(
        codex.detect_launch_session(&evidence(Some(MARKER))),
        Some(LaunchSessionDetection {
            session_id: "thread-own".into(),
            source: LaunchSessionSource::LaunchMarker,
        })
    );
    assert_eq!(codex.detect_launch_session(&evidence(None)), None);
    assert_eq!(
        codex.detect_launch_session(&evidence(Some(
            "ct-launch-00000000000000000000000000000000"
        ))),
        None
    );
}
