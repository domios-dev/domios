use super::*;
use serde_json::json;

fn source(filter: Value) -> RecordSource {
    serde_json::from_value(json!({"format":"jsonl","filter":filter})).unwrap()
}

#[test]
fn filters_types_and_nested_session_without_renumbering_or_byte_drift() {
    let input = concat!(
        "{\"type\":\"ignored\"}\n",
        "invalid\n",
        "{\"type\":\"event\",\"payload\":{\"thread_id\":\"other\"}}\n",
        "{\"type\":\"event\",\"payload\":{\"thread_id\":\"own\",\"text\":\"🦀 é\"}}\r\n",
        "{\"type\":\"event\",\"payload\":{\"thread_id\":\"own\"}}"
    );
    let descriptor = source(json!({"type":{"path":["type"],"in":["event"]}, "where":[{
        "path":["payload","thread_id"], "equals":{"placeholder":"sessionId"}
    }]}));
    let parsed = parse_records(input.as_bytes(), 97, &descriptor, Some("own"));
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].row, 3);
    let start = input
        .find("{\"type\":\"event\",\"payload\":{\"thread_id\":\"own\"")
        .unwrap();
    assert_eq!(parsed.records[0].offset, 97 + start as u64);
    assert_eq!(
        parsed.meta.new_offset,
        97 + input.rfind('\n').unwrap() as u64 + 1
    );
    assert!(parsed.meta.truncated_tail);
    assert_eq!(
        parsed.meta.counts,
        RecordCounts {
            total: 4,
            matched: 1,
            invalid: 1,
            oversized: 0
        }
    );
    assert_eq!(parsed.records[0].value["payload"]["text"], "🦀 é");
    assert!(parse_records(input.as_bytes(), 97, &descriptor, None)
        .records
        .is_empty());
}

#[test]
fn equality_is_typed_and_missing_fields_never_match_null() {
    let descriptor = source(json!({"type":{"path":["type"],"in":["event"]}, "where":[{
        "path":["payload","value"], "equals":null
    }]}));
    assert!(descriptor
        .filter
        .matches(&json!({"type":"event","payload":{"value":null}}), None));
    assert!(!descriptor
        .filter
        .matches(&json!({"type":"event","payload":{}}), None));
    assert!(!descriptor
        .filter
        .matches(&json!({"type":17,"payload":{"value":null}}), None));
    assert!(!descriptor
        .filter
        .matches(&json!({"type":"event","payload":{"value":"null"}}), None));
}

#[test]
fn invalid_records_and_lossy_utf8_match_text_delta() {
    let descriptor = source(json!({"type":{"path":["type"],"in":["event"]}}));
    let input = b" \r\ninvalid\n{\"type\":\"event\",\"text\":\"\xff\"}\n";
    let parsed = parse_records(input, 0, &descriptor, None);
    assert_eq!(parsed.meta.counts.invalid, 2);
    assert_eq!(parsed.records[0].row, 2);
    assert_eq!(parsed.records[0].offset, 11);
    assert_eq!(parsed.meta.new_offset, input.len() as u64);
    assert_eq!(parsed.records[0].value["text"], "�");
}

#[test]
fn bounded_reader_keeps_partial_tail_and_oversized_budget_semantics() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let body = "{\"type\":\"event\",\"text\":\"é🦀\"}\n";
    std::fs::write(file.path(), format!("{body}{{\"type\":")).unwrap();
    let descriptor = source(json!({"type":{"path":["type"],"in":["event"]}}));
    let parsed = read_records(file.path(), 0, 1, &descriptor, None).unwrap();
    assert_eq!(parsed.meta.new_offset, body.len() as u64);
    let tail = read_records(file.path(), parsed.meta.new_offset, 1024, &descriptor, None).unwrap();
    assert!(tail.records.is_empty());
    assert!(tail.meta.truncated_tail);
    assert_eq!(tail.meta.new_offset, parsed.meta.new_offset);
}

#[test]
fn oversized_projection_retains_bounded_descriptor_metadata() {
    let descriptor: RecordSource = serde_json::from_value(json!({"format":"jsonl",
        "maxRecordBytes":100, "filter":{"type":{"path":["type"],"in":["event"]}},
        "oversized":{"keep":[["type"],["payload","item","id"],["payload","thread_id"]]}
    }))
    .unwrap();
    let input = json!({"type":"event","payload":{"thread_id":"own","item":{"id":"a","output":"x".repeat(445*1024)}}}).to_string()+"\n";
    let parsed =
        parse_records_with_limit(input.as_bytes(), 100, &descriptor, Some("own"), 1024 * 1024);
    assert_eq!(
        parsed.meta.counts,
        RecordCounts {
            total: 1,
            matched: 1,
            invalid: 0,
            oversized: 1
        }
    );
    assert_eq!(
        parsed.records[0].value,
        json!({"type":"event","payload":{"thread_id":"own","item":{"id":"a"}}})
    );
    assert!(parsed.records[0].oversized);
    assert_eq!(parsed.records[0].bytes, Some(input.len() as u64));
    assert_eq!(parsed.meta.new_offset, 100 + input.len() as u64);
}

#[test]
fn invalid_source_limits_formats_and_placeholders_are_refused() {
    assert!(
        serde_json::from_value::<RecordSource>(json!({"format":"sqlite","filter":{}})).is_err()
    );
    assert!(
        serde_json::from_value::<RecordSource>(json!({"format":"jsonl","filter":{"where":[{
            "path":["id"],"equals":{"placeholder":"foreignSession"}
        }]}}))
        .is_err()
    );
    let mut descriptor = source(json!({}));
    descriptor.max_record_bytes = Some(0);
    assert!(descriptor.validate().is_err());
    descriptor.max_record_bytes = Some(crate::jsonl_window::MAX_RECORD_BYTES + 1);
    assert!(descriptor.validate().is_err());
}

#[test]
fn unbounded_oversized_metadata_fails_before_plugin_projection() {
    let mut descriptor = source(json!({}));
    descriptor.max_record_bytes = Some(10);
    descriptor.oversized.keep = vec![vec!["payload".into()]];
    let input = json!({"payload":"x".repeat(2048)}).to_string() + "\n";
    let parsed = parse_records_with_limit(input.as_bytes(), 0, &descriptor, None, 1024);
    assert!(parsed.projection_error.is_some());
    assert!(parsed.records.is_empty());
}
