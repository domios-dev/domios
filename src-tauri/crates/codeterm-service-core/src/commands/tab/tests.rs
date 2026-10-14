use super::backfill_window_slice;
use crate::service::tab_chat::session_window::{
    self, SNAPSHOT_ANCHOR_STRIDE_BYTES, SNAPSHOT_CAP_BYTES, SNAPSHOT_WINDOW_BYTES,
};
use std::sync::Arc;

#[test]
fn chat_start_replays_registered_sse_history_without_a_plugin_store() {
    use crate::multiplexer::state::MultiplexerState;
    use std::io::{Read, Write};
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some("own".into());
    tab.session_binding_nonce = Some("nonce".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(state);
    let service = Arc::new(fixture.service.clone());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert!(String::from_utf8(request)
                .unwrap()
                .starts_with("GET /session/own/message?"));
            let body = serde_json::json!([{"info":{"sessionID":"own","id":"message","role":"assistant"},
                "parts":[{"sessionID":"own","messageID":"message","id":"part","type":"text","text":"retained"}]}]).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    crate::service::structured_prompt::register_endpoint(
        &tab_id,
        format!("http://{address}"),
        "/repo".into(),
    );
    let first = super::chat_start_core(&service, tab_id.clone(), "first".into()).unwrap();
    let replay = super::chat_start_core(&service, tab_id.clone(), "remount".into()).unwrap();
    let snapshot = crate::service::tab_chat::tab_chat_snapshot(&service, &tab_id, 0).unwrap();
    crate::service::structured_endpoint::remove(&tab_id);
    server.join().unwrap();
    assert_eq!(first.messages, replay.messages);
    let rows = replay.messages.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "part");
    assert_eq!(rows[0].content, "retained");
    assert_eq!(snapshot, (rows, 1));
    assert_eq!(replay.source_session_id.as_deref(), Some("own"));
    assert_eq!(replay.session_binding_nonce.as_deref(), Some("nonce"));
    assert_eq!(replay.offset, 0);
}

#[test]
fn chat_start_removed_plugin_reports_state_without_rebinding_restored_tab() {
    use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "removed-fixture".into(),
        session_id: "preserved-session".into(),
    };
    tab.restored_from_snapshot = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(state);
    let service = Arc::new(fixture.service.clone());
    let result = super::chat_start_core(&service, tab_id.clone(), "fixture-subscription".into());
    let error: serde_json::Value =
        serde_json::from_str(&result.err().expect("missing plugin must be refused")).unwrap();
    assert_eq!(error["code"], "chat_backend_plugin_removed");
    let mux = service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.id, tab_id);
    assert_eq!(
        tab.backing,
        TabBacking::ChatBackend {
            plugin_id: "removed-fixture".into(),
            session_id: "preserved-session".into(),
        }
    );
    assert!(tab.restored_from_snapshot);
    assert!(tab.pty_id.is_none());
}

fn workspace_plugins_root() -> std::path::PathBuf {
    crate::test_support::fixture_manifest_dir()
        .join("../../..")
        .join("plugins")
}

fn claude_registry() -> Arc<crate::agent::registry::AgentRegistry> {
    let plugin_js = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-claude")
            .join("plugin.js"),
    )
    .expect("read claude plugin.js");
    let manifest_json = r#"{
        "id": "claude",
        "version": "0.1.0",
        "displayName": "Claude Code",
        "spawn": {
            "readinessStrategy": {"kind": "fixed_delay", "durationMs": 0},
            "taskDelivery": "pty_input",
            "verifySubmit": {"kind": "none"}
        },
        "sessions": {
            "rootDir": "~/.claude/projects",
            "cwdEncoding": "dash_encoded",
            "fileExtension": "jsonl"
        }
    }"#;
    let manifest: crate::plugin::js_runtime::PluginManifest =
        serde_json::from_str(manifest_json).expect("parse claude manifest");
    let vm = crate::plugin::js_runtime::PluginVm::from_source_with(
        "claude",
        plugin_js,
        Some(manifest_json.to_string()),
    )
    .expect("init claude VM");
    let provider = crate::plugin::js_runtime::JsProvider::new(manifest, Arc::new(vm));
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(provider));
    Arc::new(registry)
}

#[test]
fn large_jsonl_tail_window_returns_normalized_history() {
    if !workspace_plugins_root().join("provider-claude").exists() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dante.jsonl");
    let compact = serde_json::json!({
        "type": "user",
        "uuid": "compact-user",
        "isVisibleInTranscriptOnly": true,
        "isCompactSummary": true,
        "message": {
            "role": "user",
            "content": "This session is being continued from a previous conversation.\n\nSummary:\n- Dante context"
        }
    });
    let task = serde_json::json!({
        "type": "user",
        "uuid": "task-user",
        "origin": {"kind": "task-notification"},
        "message": {
            "role": "user",
            "content": "<task-notification>\n<status>completed</status>\n</task-notification>"
        }
    });
    let normal = serde_json::json!({
        "type": "user",
        "uuid": "normal-user",
        "message": {"role": "user", "content": "real user message"}
    });
    std::fs::write(&path, format!("{}\n{}\n{}\n", compact, task, normal)).unwrap();

    let snap = session_window::snapshot(&claude_registry(), "claude", &path);
    assert!(!snap.truncated());
    let msgs = snap.messages;

    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].kind, "system");
    assert_eq!(msgs[0].system_subtype.as_deref(), Some("compact_summary"));
    assert_eq!(
        msgs[0].timeline_marker,
        Some(crate::agent::chat_types::ChatTimelineMarker::ContextCompaction)
    );
    assert!(msgs[0].collapsed);
    assert_eq!(msgs[1].kind, "status");
    assert_eq!(msgs[1].content, "Task completed");
    assert_eq!(msgs[1].system_subtype.as_deref(), Some("task_notification"));
    assert_eq!(msgs[2].kind, "user");
    assert_eq!(msgs[2].content, "real user message");
}

#[test]
fn backfill_paging_is_lossless_and_line_aligned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let lines: Vec<String> = (0..600)
        .map(|i| format!("{{\"type\":\"user\",\"uuid\":\"u{i}\",\"message\":{{\"role\":\"user\",\"content\":\"line {i} {}\"}}}}", "x".repeat(40)))
        .collect();
    let body = format!("{}\n", lines.join("\n"));
    std::fs::write(&path, &body).unwrap();
    let total = std::fs::metadata(&path).unwrap().len();

    // Page backward in small windows; every page must start on a line start
    // and end exactly where the previous page began.
    let mut cursor = total;
    let mut pages_reversed: Vec<String> = Vec::new();
    while cursor > 0 {
        let (text, aligned_start) = backfill_window_slice(&path, cursor, 4096).unwrap();
        assert!(aligned_start < cursor, "every page makes progress");
        if aligned_start > 0 {
            assert_eq!(
                body.as_bytes()[aligned_start as usize - 1],
                b'\n',
                "aligned start sits on a line start",
            );
        }
        pages_reversed.push(text);
        cursor = aligned_start;
    }
    let reassembled: String = pages_reversed
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(
        reassembled, body,
        "concatenated pages reproduce the file losslessly"
    );
    assert!(
        pages_reversed.len() > 10,
        "fixture actually exercised many pages"
    );
}

#[test]
fn backfill_window_clamps_at_eof() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tiny.jsonl");
    std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n").unwrap();
    let total = std::fs::metadata(&path).unwrap().len();

    // Requesting far past EOF clamps to the file.
    let (text, start) = backfill_window_slice(&path, total + 10_000, 1 << 20).unwrap();
    assert_eq!(start, 0);
    assert_eq!(text.as_bytes(), std::fs::read(&path).unwrap().as_slice());

    // A zero-progress request (before_offset 0) yields an empty window.
    let (text, start) = backfill_window_slice(&path, 0, 4096).unwrap();
    assert!(text.is_empty());
    assert_eq!(start, 0);
}

/// Backfill pages parse through the tab's own provider parser via the
/// bounded session_window range — the same normalized shape as chat_start.
#[test]
fn backfill_range_parses_with_provider_parser() {
    if !workspace_plugins_root().join("provider-claude").exists() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bf.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"type\":\"user\",\"uuid\":\"bf-1\",\"message\":{\"role\":\"user\",\"content\":\"older question\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"bf-2\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"older answer\"}]}}\n",
        ),
    )
    .unwrap();
    let total = std::fs::metadata(&path).unwrap().len();

    let (rich, _end) = session_window::parse_window(&claude_registry(), "claude", &path, 0, total);
    let msgs: Vec<crate::agent::chat_types::ChatMsg> = rich
        .iter()
        .enumerate()
        .flat_map(|(i, r)| crate::service::tab_chat::rich_to_chat_msgs(r, i))
        .collect();
    assert_eq!(msgs.len(), 2, "provider parser normalizes the page");
    assert_eq!(msgs[0].kind, "user");
    assert_eq!(msgs[0].content, "older question");
    assert_eq!(msgs[1].kind, "assistant");
    assert_eq!(msgs[1].content, "older answer");
}

/// The production regression, against the real claude plugin: a session file
/// past the snapshot cap must be read as a bounded tail, not whole.
#[test]
fn oversized_claude_session_snapshots_a_bounded_tail() {
    if !workspace_plugins_root().join("provider-claude").exists() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge.jsonl");
    let filler = serde_json::json!({
        "type": "user",
        "uuid": "filler",
        "message": {"role": "user", "content": "f".repeat(4096)}
    })
    .to_string();
    let tail_marker = serde_json::json!({
        "type": "user",
        "uuid": "tail-user",
        "message": {"role": "user", "content": "the last message"}
    })
    .to_string();

    {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        let mut written = 0u64;
        while written <= SNAPSHOT_CAP_BYTES + SNAPSHOT_WINDOW_BYTES {
            writeln!(f, "{filler}").unwrap();
            written += filler.len() as u64 + 1;
        }
        writeln!(f, "{tail_marker}").unwrap();
        f.flush().unwrap();
    }
    let total = std::fs::metadata(&path).unwrap().len();
    assert!(total > SNAPSHOT_CAP_BYTES);

    let snap = session_window::snapshot(&claude_registry(), "claude", &path);
    assert!(
        snap.truncated(),
        "oversized session must use the tail window"
    );
    assert!(
        snap.bytes_read <= SNAPSHOT_WINDOW_BYTES + SNAPSHOT_ANCHOR_STRIDE_BYTES,
        "read {} bytes",
        snap.bytes_read
    );
    assert_eq!(snap.total, total, "tailer must resume at file length");
    assert_eq!(
        snap.start_offset + snap.bytes_read,
        total,
        "window must end exactly at EOF"
    );
    let last = snap.messages.last().expect("tail window has messages");
    assert_eq!(last.kind, "user");
    assert_eq!(last.content, "the last message");
    // Every record in the window parsed — no partial leading line survived.
    assert!(snap.messages.iter().all(|m| m.kind == "user"));
}
