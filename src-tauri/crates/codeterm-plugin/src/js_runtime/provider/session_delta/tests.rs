use super::*;
use std::io::Write;

fn records_provider(body: &str) -> JsProvider {
    let manifest = serde_json::from_value(serde_json::json!({
        "id": "record-delta-fixture", "version": "0.1.0", "displayName": "Fixture",
        "capabilities": {"recordSources": {"sessionTranscript": {
            "format": "jsonl", "filter": {}
        }}}
    }))
    .unwrap();
    let script = format!("module.exports = {{ parseSessionRecords(records, meta) {{ {body} }} }};");
    let vm = crate::js_runtime::PluginVm::from_source("record-delta-fixture", script).unwrap();
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}

const RECORD_DELTA: &str = r#"
    if (records.length === 0) throw new Error('empty projection must not run');
    return {new_offset: meta.newOffset, messages: records.map(record => ({
        role: 'assistant', blocks: [{kind: 'text', data: {text: record.value.text}}],
        recordIndex: record.row
    }))};
"#;

#[test]
fn partial_record_waits_for_completion_then_streams_following_rows() {
    let provider = records_provider(RECORD_DELTA);
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(br#"{"type":"assistant","text":""#).unwrap();
    file.flush().unwrap();
    let held = provider.js_parse_session_delta(file.path(), 0, 8 * 1024 * 1024);
    assert_eq!(held.new_offset, 0);
    assert!(held.messages.is_empty());
    file.write_all(b"opaque completed row\"}\n").unwrap();
    file.flush().unwrap();
    let completed = provider.js_parse_session_delta(file.path(), 0, 8 * 1024 * 1024);
    assert_eq!(
        completed.new_offset,
        file.as_file().metadata().unwrap().len()
    );
    assert_eq!(completed.messages.len(), 1);
    assert_eq!(
        completed.messages[0].blocks[0].data["text"],
        "opaque completed row"
    );
    file.write_all(b"{\"type\":\"assistant\",\"text\":\"next row\"}\n")
        .unwrap();
    file.flush().unwrap();
    let next = provider.js_parse_session_delta(file.path(), completed.new_offset, 8 * 1024 * 1024);
    assert_eq!(next.new_offset, file.as_file().metadata().unwrap().len());
    assert_eq!(next.messages.len(), 1);
    assert_eq!(next.messages[0].blocks[0].data["text"], "next row");
}

#[test]
fn failed_record_projection_surfaces_error_and_advances_at_host_boundary() {
    for (failure, expected) in [
        (
            "throw new Error('opaque projection failure');",
            "opaque projection failure",
        ),
        (
            "return {new_offset: meta.newOffset + 1, messages: []};",
            "differs from host boundary",
        ),
        (
            "return {new_offset: meta.newOffset, messages: [null]};",
            "row is invalid",
        ),
    ] {
        let script = format!("if (records[0].value.text === 'bad') {{ {failure} }} {RECORD_DELTA}");
        let provider = records_provider(&script);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"{\"type\":\"assistant\",\"text\":\"bad\"}\n")
            .unwrap();
        file.flush().unwrap();
        let end = file.as_file().metadata().unwrap().len();
        let failed = provider.js_parse_session_delta(file.path(), 0, 8 * 1024 * 1024);
        assert_eq!(failed.new_offset, end);
        assert_eq!(failed.messages.len(), 1);
        assert_eq!(failed.messages[0].role, "system");
        assert_eq!(failed.messages[0].record_start, Some(0));
        assert_eq!(
            failed.messages[0].blocks[0].data["subtype"],
            "transcript_error"
        );
        assert!(failed.messages[0].blocks[0].data["text"]
            .as_str()
            .unwrap()
            .contains(expected));
        file.write_all(b"{\"type\":\"assistant\",\"text\":\"after failure\"}\n")
            .unwrap();
        file.flush().unwrap();
        let next = provider.js_parse_session_delta(file.path(), end, 8 * 1024 * 1024);
        assert_eq!(next.new_offset, file.as_file().metadata().unwrap().len());
        assert_eq!(next.messages.len(), 1);
        assert_eq!(next.messages[0].blocks[0].data["text"], "after failure");
    }
}

#[test]
fn unreadable_source_reports_once_without_inventing_a_consumed_offset() {
    let provider = records_provider(RECORD_DELTA);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("missing.jsonl");
    let failed = provider.js_parse_session_delta(&path, 17, 1024);
    assert_eq!(failed.new_offset, 17);
    assert_eq!(failed.messages.len(), 1);
    assert_eq!(
        failed.messages[0].blocks[0].data["subtype"],
        "transcript_error"
    );
    let repeated = provider.js_parse_session_delta(&path, 17, 1024);
    assert_eq!(repeated.new_offset, 17);
    assert!(repeated.messages.is_empty());
}

#[test]
fn oversized_record_projection_preserves_rows_after_the_record() {
    let mut provider = records_provider(RECORD_DELTA);
    let source = provider
        .manifest
        .capabilities
        .record_sources
        .get_mut("sessionTranscript")
        .unwrap();
    source.max_record_bytes = Some(1024);
    source.oversized.keep = vec![vec!["type".into()], vec!["text".into()]];
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({
            "type": "assistant", "text": "oversized row", "padding": "x".repeat(1_300_000)
        })
    )
    .unwrap();
    file.write_all(b"{\"type\":\"assistant\",\"text\":\"after oversized row\"}\n")
        .unwrap();
    file.flush().unwrap();
    let delta = provider.js_parse_session_delta(file.path(), 0, 8 * 1024 * 1024);
    assert_eq!(delta.new_offset, file.as_file().metadata().unwrap().len());
    assert_eq!(delta.messages.len(), 2);
    assert_eq!(delta.messages[0].blocks[0].data["text"], "oversized row");
    assert_eq!(
        delta.messages[1].blocks[0].data["text"],
        "after oversized row"
    );
}

#[test]
fn chunk_decode_moves_the_original_allocation_into_json() {
    let mut bytes = Vec::with_capacity(1024);
    bytes.extend_from_slice("opaque λ\n".as_bytes());
    let pointer = bytes.as_ptr();
    let capacity = bytes.capacity();
    let serde_json::Value::String(chunk) = chunk_value(bytes) else {
        panic!("expected string argument");
    };
    assert_eq!(chunk.as_ptr(), pointer);
    assert_eq!(chunk.capacity(), capacity);
    assert_eq!(chunk, "opaque λ\n");
}

#[test]
fn lossy_chunk_decode_preserves_original_record_positions() {
    let bytes = b"\xff\nnext\n".to_vec();
    let starts = crate::jsonl_window::record_starts(&bytes);
    assert_eq!(
        chunk_value(bytes),
        serde_json::Value::String("�\nnext\n".into())
    );
    let mut rows = vec![Default::default(), Default::default()];
    crate::jsonl_window::anchor_rows_from_starts(&mut rows, &[Some(0), Some(1)], &starts, 100);
    assert_eq!(rows[0].record_start, Some(100));
    assert_eq!(rows[1].record_start, Some(102));
}
