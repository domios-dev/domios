use super::*;

fn stored_item(id: &str, bytes: usize) -> String {
    format!(
        "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"item_completed\",\"thread_id\":\"own\",\"turn_id\":\"turn\",\"completed_at_ms\":123,\"item\":{{\"id\":\"{id}\",\"type\":\"AgentMessage\",\"status\":\"completed\",\"text\":\"{}\"}}}}}}\n",
        "x".repeat(bytes)
    )
}

#[test]
fn oversized_last_record_is_returned_alone_and_moves_strictly_earlier() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let first = stored_item("first", 1);
    std::fs::write(
        &path,
        format!("{first}{}", stored_item("huge", 2 * 1024 * 1024)),
    )
    .unwrap();
    let total = std::fs::metadata(&path).unwrap().len();
    let (records, start, end) = read_window(&path, Some(total)).unwrap();
    assert_eq!(start, first.len() as u64);
    assert!(start < total);
    assert_eq!(end, total);
    assert_eq!(records.lines().count(), 1);
    let record: serde_json::Value = serde_json::from_str(&records).unwrap();
    assert_eq!(record["payload"]["item"]["id"], "huge");
    assert_eq!(record["payload"]["item"]["historyTruncated"], true);
    assert_eq!(read_window(&path, Some(start)).unwrap(), (first, 0, start));
}

#[test]
fn twenty_mb_record_is_projected_before_the_runtime_hook() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, stored_item("huge", 20 * 1024 * 1024)).unwrap();
    let (records, start, end) = read_window(&path, None).unwrap();
    assert_eq!(start, 0);
    assert_eq!(end, std::fs::metadata(&path).unwrap().len());
    assert!(records.len() < 1024);
    let record: serde_json::Value = serde_json::from_str(&records).unwrap();
    assert_eq!(
        record["payload"]["item"],
        serde_json::json!({
            "id":"huge", "type":"AgentMessage", "status":"completed", "historyTruncated":true
        })
    );
    assert_eq!(record["payload"]["thread_id"], "own");
    assert_eq!(record["payload"]["turn_id"], "turn");
    assert_eq!(record["payload"]["completed_at_ms"], 123);
}

#[test]
fn stored_walk_with_several_oversized_records_has_no_gaps_or_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    use std::io::Write;
    let mut file = std::fs::File::create(&path).unwrap();
    for (index, bytes) in [1, 2 * 1024 * 1024, 3, 3 * 1024 * 1024, 4, 2 * 1024 * 1024]
        .into_iter()
        .enumerate()
    {
        file.write_all(stored_item(&index.to_string(), bytes).as_bytes())
            .unwrap();
    }
    file.write_all(b"{\"partial\":").unwrap();
    drop(file);
    let mut cursor = std::fs::metadata(&path).unwrap().len() - 11;
    let mut ids = Vec::new();
    while cursor > 0 {
        let (records, start, end) = read_window(&path, Some(cursor)).unwrap();
        assert_eq!(end, cursor);
        assert!(start < cursor);
        assert!(records.len() as u64 <= WINDOW_BYTES);
        let page: Vec<_> = records
            .lines()
            .map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                value["payload"]["item"]["id"].as_str().unwrap().to_owned()
            })
            .collect();
        ids.extend(page.into_iter().rev());
        cursor = start;
    }
    assert_eq!(ids, ["5", "4", "3", "2", "1", "0"]);
}

#[test]
fn oversized_record_with_unprojectable_metadata_fails_without_skipping() {
    let record = format!(
        "{{\"payload\":{{\"item\":{{\"id\":\"id\"}},\"opaque\":\"{}\"}}}}",
        "x".repeat(2048)
    );
    assert!(projection::oversized_record(&record, 1024).is_err());
}

#[test]
fn stored_window_holds_a_partial_tail_and_reads_only_complete_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, b"{\"id\":1}\n{\"id\":2}\n{\"id\":").unwrap();
    let (records, start, end) = read_window(&path, None).unwrap();
    assert_eq!(start, 0);
    assert_eq!(end, 18);
    assert_eq!(records, "{\"id\":1}\n{\"id\":2}\n");
    assert_eq!(
        read_window(&path, Some(9)).unwrap(),
        ("{\"id\":1}\n".into(), 0, 9)
    );
}

#[test]
fn stored_window_is_bounded_and_keeps_an_exact_backfill_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let record = format!("{{\"opaque\":\"{}\"}}\n", "x".repeat(1024));
    let mut file = std::fs::File::create(&path).unwrap();
    use std::io::Write;
    for _ in 0..6000 {
        file.write_all(record.as_bytes()).unwrap();
    }
    drop(file);
    let (records, start, end) = read_window(&path, None).unwrap();
    assert!(start > 0);
    assert!(records.len() as u64 <= WINDOW_BYTES);
    assert_eq!(start % record.len() as u64, 0);
    assert_eq!(end, std::fs::metadata(&path).unwrap().len());
    let (_, previous_start, previous_end) = read_window(&path, Some(start)).unwrap();
    assert_eq!(previous_end, start);
    assert!(previous_start < start);
}

#[test]
fn on_demand_stored_windows_reach_first_record_without_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.jsonl");
    let record = format!("{{\"content\":\"{}\"}}\n", "x".repeat(1024));
    use std::io::Write;
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"{\"id\":\"first\"}\n").unwrap();
    for _ in 0..3000 {
        file.write_all(record.as_bytes()).unwrap();
    }
    drop(file);
    let mut cursor = std::fs::metadata(&path).unwrap().len();
    let mut pages = 0;
    while cursor > 0 {
        let (records, start, end) = read_window(&path, Some(cursor)).unwrap();
        assert_eq!(end, cursor);
        assert!(start < cursor);
        assert!(records.len() as u64 <= WINDOW_BYTES);
        if start == 0 {
            assert!(records.starts_with("{\"id\":\"first\"}\n"));
        }
        cursor = start;
        pages += 1;
    }
    assert!(pages > 2);
}

#[test]
fn kernel_stored_page_matches_text_projection_for_445kb_and_oversized_records() {
    use codeterm_plugin::provider::AgentProvider;
    let plugins = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let provider = std::fs::read_dir(plugins)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path().join("plugin.json");
            let manifest = codeterm_plugin::js_runtime::PluginManifest::load(&path).ok()?;
            manifest
                .capabilities
                .record_sources
                .contains_key("storedHistory")
                .then(|| codeterm_plugin::js_runtime::load_one(&path).unwrap())
        })
        .next()
        .expect("bundled stored record provider");
    let file = tempfile::NamedTempFile::new().unwrap();
    for size in [445_000, 2 * 1024 * 1024] {
        let first = stored_item("first", 1);
        let last = stored_item("last", size);
        std::fs::write(file.path(), format!("{first}{last}")).unwrap();
        let (text, start, end) = read_window(file.path(), None).unwrap();
        let expected: Vec<ChatMsg> = codeterm_plugin::js_runtime::call_json(
            provider.vm(),
            "runtimeStoredHistory",
            &[serde_json::json!("own"), serde_json::json!(text)],
        )
        .unwrap();
        let parsed = super::super::jsonl_kernel::stored_window(
            &provider,
            "own",
            file.path(),
            None,
            WINDOW_BYTES,
        )
        .unwrap();
        let actual = provider.runtime_stored_records("own", &parsed).unwrap();
        assert_eq!(
            (parsed.meta.from_offset, parsed.meta.new_offset),
            (start, end)
        );
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert!(actual.iter().any(|row| row.id == "last"));
        if size > WINDOW_BYTES as usize {
            assert_eq!(actual[0].kind, "system");
            let source = provider.record_source("storedHistory").unwrap();
            let window = codeterm_plugin::jsonl_window::records::parse_records_with_limit(
                last.as_bytes(),
                start,
                source,
                Some("own"),
                WINDOW_BYTES,
            );
            assert!(window.records[0].oversized);
            assert!(window.records[0].value["payload"]["item"]
                .get("text")
                .is_none());
        }
    }
}
