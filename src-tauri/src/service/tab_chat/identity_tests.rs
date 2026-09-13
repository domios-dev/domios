//! One uuid-less session record keeps one normalized id in every read path:
//! CLI snapshot, record-aligned window, live tailer delta and backfill page.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tempfile::TempDir;

use crate::agent::chat_types::{ChatMsg, MessageBlock, RichMessage};
use crate::agent::registry::AgentRegistry;
use crate::service::tab_chat::{
    rich_rows_to_chat_msgs, rich_to_chat_msgs, rich_to_chat_msgs_at, rich_to_chat_msgs_live,
    session_window, spawn_tailer, DeltaSink, RowOrigin, TailConfig,
};

const CWD: &str = "/identity/proj";
const SID: &str = "identity-session";
const DELIVERY_DEADLINE: Duration = Duration::from_secs(30);
/// Enough filler that one backfill page cannot start at byte 0.
const FILLER_RECORDS: usize = 2100;

const PLAIN: &str =
    r#"{"type":"user","message":{"role":"user","content":"plain question"}}"#;
const SPLIT: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first part"},{"type":"text","text":"second part"}]}}"#;
const UUID_ROW: &str = r#"{"type":"assistant","uuid":"u-identity","message":{"role":"assistant","content":[{"type":"text","text":"uuid answer"},{"type":"text","text":"uuid tail"}]}}"#;
const TARGETS: [&str; 5] = [
    "plain question",
    "first part",
    "second part",
    "uuid answer",
    "uuid tail",
];

fn plugins_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
}

fn js_registry(plugin_dir: &str, id: &str, sessions_root: &Path) -> Arc<AgentRegistry> {
    let plugin_js = std::fs::read_to_string(plugins_root().join(plugin_dir).join("plugin.js"))
        .expect("read bundled provider plugin.js");
    let root = sessions_root.to_string_lossy().replace('\\', "/");
    let manifest_json = format!(
        r#"{{
            "id": "{id}",
            "version": "0.1.0",
            "displayName": "{id}",
            "spawn": {{
                "readinessStrategy": {{"kind": "fixed_delay", "durationMs": 0}},
                "taskDelivery": "pty_input",
                "verifySubmit": {{"kind": "none"}}
            }},
            "sessions": {{
                "rootDir": "{root}",
                "cwdEncoding": "dash_encoded",
                "fileExtension": "jsonl"
            }}
        }}"#
    );
    let manifest: crate::plugin::js_runtime::PluginManifest =
        serde_json::from_str(&manifest_json).expect("parse synthetic manifest");
    let vm = crate::plugin::js_runtime::PluginVm::from_source_with(
        id,
        plugin_js,
        Some(manifest_json),
    )
    .expect("init provider VM");
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::js_runtime::JsProvider::new(
        manifest,
        Arc::new(vm),
    )));
    Arc::new(registry)
}

struct BatchSink(Mutex<mpsc::Sender<Vec<RichMessage>>>);

impl DeltaSink for BatchSink {
    fn emit(&self, _tab_id: &str, messages: Vec<RichMessage>, _start: u64, _offset: u64) {
        let _ = self.0.lock().send(messages);
    }
}

fn filler_record(index: usize) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "type": "user",
            "uuid": format!("fill-{index:06}"),
            "message": {"role": "user", "content": "f".repeat(4093)}
        })
    )
}

fn append(path: &Path, records: &[&str]) {
    use std::io::Write;
    let body: String = records.iter().map(|r| format!("{r}\n")).collect();
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(body.as_bytes()).unwrap();
    file.flush().unwrap();
}

fn target_ids(messages: &[ChatMsg]) -> Vec<(String, String)> {
    messages
        .iter()
        .filter(|m| TARGETS.contains(&m.content.as_str()))
        .map(|m| (m.content.clone(), m.id.clone()))
        .collect()
}

fn ids(messages: &[ChatMsg]) -> Vec<String> {
    messages.iter().map(|m| m.id.clone()).collect()
}

#[test]
fn tab_chat_identity_uuidless_record_has_one_id_across_snapshot_window_live_and_backfill() {
    let root = TempDir::new().unwrap();
    let registry = js_registry("provider-claude", "claude", root.path());
    let path = registry
        .with_provider("claude", |p| p.session_jsonl_path(CWD, SID))
        .flatten()
        .expect("claude session path");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let filler: String = (0..FILLER_RECORDS).map(filler_record).collect();
    std::fs::write(&path, &filler).unwrap();
    let record_len = filler_record(0).len() as u64;
    let plain_start = filler.len() as u64;
    let split_start = plain_start + PLAIN.len() as u64 + 1;

    let (tx, rx) = mpsc::channel();
    let tailer = spawn_tailer(
        TailConfig {
            tab_id: "identity".into(),
            provider_id: "claude".into(),
            cwd: CWD.into(),
            session_id: SID.into(),
            start_offset: plain_start - record_len,
            poll_interval: Duration::from_millis(10),
        },
        registry.clone(),
        Arc::new(BatchSink(Mutex::new(tx))),
    );
    append(&path, &[PLAIN, SPLIT, UUID_ROW]);
    let mut live: Vec<ChatMsg> = Vec::new();
    let mut stream_seq = 7000;
    while !live.iter().any(|m| m.content == "uuid tail") {
        let batch = rx
            .recv_timeout(DELIVERY_DEADLINE)
            .expect("live tailer delivers the appended records");
        for row in &batch {
            live.extend(rich_to_chat_msgs_live(row, stream_seq));
            stream_seq += 1;
        }
    }
    tailer.shutdown();
    let total = std::fs::metadata(&path).unwrap().len();

    let snapshot = session_window::snapshot(&registry, "claude", &path);
    assert_eq!(snapshot.start_offset, 0, "CLI snapshot reads from byte 0");

    let window_start = codeterm_plugin::jsonl_window::tail_window_start(
        &path,
        total,
        total - (record_len * (FILLER_RECORDS as u64 - 3) + 1),
    );
    assert_eq!(
        window_start,
        record_len * (FILLER_RECORDS as u64 - 2),
        "window is record-aligned"
    );
    let (window_rows, _) =
        session_window::parse_window(&registry, "claude", &path, window_start, total);
    let window = rich_rows_to_chat_msgs(&window_rows, window_start);

    let page = crate::commands::backfill_page(&registry, "claude", &path, total)
        .expect("backfill page");
    assert!(
        page.start_offset > 0 && page.start_offset < window_start,
        "backfill page starts at its own aligned record, got {}",
        page.start_offset
    );

    let expected: Vec<(String, String)> = [
        ("plain question", format!("rec-{plain_start}-0-0")),
        ("first part", format!("rec-{split_start}-0-0")),
        ("second part", format!("rec-{split_start}-0-1")),
        ("uuid answer", "u-identity-0".to_string()),
        ("uuid tail", "u-identity-1".to_string()),
    ]
    .into_iter()
    .map(|(content, id)| (content.to_string(), id))
    .collect();
    for (origin, messages) in [
        ("snapshot", &snapshot.messages),
        ("window", &window),
        ("live", &live),
        ("backfill", &page.messages),
    ] {
        assert_eq!(target_ids(messages), expected, "{origin} ids");
    }

    let snapshot_seq: Vec<i64> = snapshot
        .messages
        .iter()
        .filter(|m| TARGETS.contains(&m.content.as_str()))
        .map(|m| m.seq)
        .collect();
    let first = FILLER_RECORDS as i64;
    assert_eq!(
        snapshot_seq,
        vec![first, first + 1, first + 1, first + 2, first + 2],
        "seq stays the batch ordinal"
    );
}

#[test]
fn tab_chat_identity_codex_ordinary_rows_share_one_id_across_reads() {
    let root = TempDir::new().unwrap();
    let registry = js_registry("provider-codex", "codex", root.path());
    let lines = [
        r#"{"type":"session_meta","payload":{"id":"11111111-1111-1111-1111-111111111111","cwd":"/work/proj"}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"codex question"}]},"timestamp":"2026-05-01T10:00:00Z"}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"codex answer"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"ls -la\"}"}}"#,
    ];
    let path = root.path().join("rollout.jsonl");
    std::fs::write(&path, "").unwrap();
    append(&path, &lines);
    let total = std::fs::metadata(&path).unwrap().len();
    let question_start = lines[0].len() as u64 + 1;
    let answer_start = question_start + lines[1].len() as u64 + 1;

    let snapshot = session_window::snapshot(&registry, "codex", &path);
    let window_start = codeterm_plugin::jsonl_window::tail_window_start(
        &path,
        total,
        total - (question_start + 1),
    );
    assert_eq!(window_start, answer_start, "window is record-aligned");
    let (window_rows, _) =
        session_window::parse_window(&registry, "codex", &path, window_start, total);
    let window = rich_rows_to_chat_msgs(&window_rows, window_start);
    let live_delta = registry
        .with_provider("codex", |p| p.parse_session_delta(&path, question_start))
        .expect("codex provider");
    let live: Vec<ChatMsg> = live_delta
        .messages
        .iter()
        .enumerate()
        .flat_map(|(i, row)| rich_to_chat_msgs_live(row, 900 + i))
        .collect();
    let page = crate::commands::backfill_page(&registry, "codex", &path, total)
        .expect("backfill page");

    let answer_id = |messages: &[ChatMsg]| {
        messages
            .iter()
            .find(|m| m.content == "codex answer")
            .map(|m| m.id.clone())
    };
    let expected = Some(format!("rec-{answer_start}-0-0"));
    for (origin, messages) in [
        ("snapshot", &snapshot.messages),
        ("window", &window),
        ("live", &live),
        ("backfill", &page.messages),
    ] {
        assert_eq!(answer_id(messages), expected, "{origin} codex answer id");
    }
}

fn row(
    role: &str,
    uuid: Option<&str>,
    record_start: Option<u64>,
    blocks: Vec<MessageBlock>,
) -> RichMessage {
    RichMessage {
        role: role.into(),
        uuid: uuid.map(String::from),
        record_start,
        blocks,
        ..Default::default()
    }
}

fn text(value: &str) -> MessageBlock {
    MessageBlock {
        kind: "text".into(),
        data: serde_json::json!({ "text": value }),
    }
}

#[test]
fn tab_chat_identity_uuid_rows_keep_provider_ids_byte_for_byte() {
    let anchored = row(
        "assistant",
        Some("3f2a9c1e-uuid"),
        Some(8404),
        vec![text("one"), text("two")],
    );
    let expected = vec!["3f2a9c1e-uuid-0".to_string(), "3f2a9c1e-uuid-1".to_string()];
    assert_eq!(ids(&rich_to_chat_msgs(&anchored, 0)), expected);
    assert_eq!(ids(&rich_to_chat_msgs_at(&anchored, RowOrigin::at(123, 9))), expected);
    assert_eq!(ids(&rich_to_chat_msgs_live(&anchored, 55)), expected);
}

#[test]
fn tab_chat_identity_record_anchor_names_every_derived_id() {
    let mut user = row(
        "user",
        None,
        Some(8404),
        vec![
            MessageBlock {
                kind: "tool_result".into(),
                data: serde_json::json!({ "content": "out" }),
            },
            MessageBlock {
                kind: "image".into(),
                data: serde_json::json!({ "source": { "media_type": "image/png", "data": "AA" } }),
            },
            text("look"),
        ],
    );
    user.record_row = 1;
    for messages in [
        rich_to_chat_msgs_at(&user, RowOrigin::at(0, 0)),
        rich_to_chat_msgs_at(&user, RowOrigin::at(4096, 17)),
        rich_to_chat_msgs_live(&user, 3),
    ] {
        assert_eq!(ids(&messages), vec!["result-rec-8404-1", "rec-8404-1-0"]);
        let attachment = messages[1].attachments.as_ref().expect("image attachment");
        assert_eq!(attachment[0].id, "rec-8404-1-img-0");
    }
}

#[test]
fn tab_chat_identity_unanchored_rows_keep_their_batch_ids() {
    let unanchored = row("assistant", None, None, vec![text("legacy")]);
    assert_eq!(ids(&rich_to_chat_msgs_at(&unanchored, RowOrigin::at(7, 3))), vec!["7-3-0"]);
    assert_eq!(ids(&rich_to_chat_msgs_live(&unanchored, 42)), vec!["42-0"]);
    assert_eq!(rich_to_chat_msgs_live(&unanchored, 42)[0].seq, 42);
}
