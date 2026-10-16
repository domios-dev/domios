use super::*;
use serde_json::{json, Value};
use std::sync::Arc;

fn fixture(filters: Value) -> JsProvider {
    let mut manifest = json!({"id":"record-fixture","version":"1.0.0","displayName":"Record fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"storedHistory":true,"runtime":"stdio-jsonrpc","command":{"binary":"runtime-engine","args":["serve"]},
                "requestMethods":{"initialize":["handshake"],"submitTurn":["turn.submit"]},"notificationMethods":["item.changed"]}},
        "permissions":{"subprocess":{"allow":["runtime-engine"]}}});
    manifest["capabilities"]["recordSources"] = filters;
    let source = r#"
        const row = (value, recordIndex) => ({role:'user', blocks:[{kind:'text',data:{text:value.text}}], recordIndex});
        module.exports = {
            parseSessionDelta(text) {
                const messages = [];
                text.split('\n').forEach((line, index) => {
                    try { const value = JSON.parse(line); if (value.type === 'user') messages.push(row(value,index)); } catch (_) {}
                });
                return {messages};
            },
            parseSessionRecords(records, meta) {
                return {messages:records.map(record => row(record.value,record.row)), new_offset:meta.newOffset};
            },
            parseSessionMessages(text) {
                return text.split('\n').flatMap(line => {
                    try { const value = JSON.parse(line); return value.type === 'user' ? [{role:'user',text:value.text,isMeta:true}] : []; } catch (_) { return []; }
                });
            },
            parseSessionMessageRecords(records) {
                return records.map(record => ({role:'user',text:record.value.text,isMeta:true}));
            },
            runtimeStoredHistory(session, text) {
                return text.split('\n').filter(Boolean).map(JSON.parse).filter(value => value.type === 'event' && value.payload.thread_id === session)
                    .map((value,seq) => ({id:value.id,seq,type:'assistant',content:value.text}));
            },
            runtimeStoredRecords(session, records) {
                return records.map((record,seq) => ({id:record.value.id,seq,type:'assistant',content:record.value.text}));
            }
        };
    "#;
    let text = manifest.to_string();
    let manifest = super::super::PluginManifest::from_json_str(&text).unwrap();
    let vm = super::super::PluginVm::from_source_with("record-fixture", source.into(), Some(text))
        .unwrap();
    JsProvider::new(manifest, Arc::new(vm))
}

#[test]
fn kernel_object_key_order_matches_json_parse() {
    let provider = load_inline_js(
        "record-key-order",
        r#"
        function project(value) {
            return [Object.keys(value), Object.keys(value.paths), JSON.stringify(value)];
        }
        module.exports = {
            textKeys(text) { return project(JSON.parse(text)); },
            parseSessionRecords(records, meta) {
                JSON.parse = () => { throw new Error('record arguments must already be objects'); };
                return project(records[0].value);
            }
        };
    "#,
    );
    let input = concat!(
        r#"{"z":0,"paths":{"src/z.ts":1,"src/a.ts":2,"10":3,"2":4,"__proto__":5},"a":[{"z":3,"a":1}],"z":2}"#,
        "\n"
    );
    let expected: Value =
        super::super::vm::call_json(provider.vm(), "textKeys", &[json!(input)]).unwrap();
    for limit in [None, Some(1)] {
        let source = serde_json::from_value(json!({
            "format":"jsonl", "filter":{}, "maxRecordBytes":limit,
            "oversized":{"keep":[["a"],["paths"],["z"]]}
        }))
        .unwrap();
        let window =
            crate::jsonl_window::records::parse_records(input.as_bytes(), 0, &source, None);
        assert_eq!(window.records.len(), 1);
        let actual: Value = super::super::vm::call_records(
            provider.vm(),
            "parseSessionRecords",
            &[json!(window.records), json!(window.meta)],
            &window,
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual[0], json!(["z", "paths", "a"]));
        assert_eq!(
            actual[1],
            json!(["2", "10", "src/z.ts", "src/a.ts", "__proto__"])
        );
    }
}

#[test]
fn native_text_and_kernel_columns_keep_rows_and_byte_identity() {
    let text = fixture(json!({}));
    let kernel = fixture(
        json!({"sessionTranscript":{"format":"jsonl","filter":{"type":{"path":["type"],"in":["user"]}}}}),
    );
    let lines = [
        "{\"type\":\"ignored\"}\n",
        "invalid\n",
        "{\"type\":\"user\",\"text\":\"é🦀\"}\r\n",
        "{\"type\":\"ignored\"}\n",
        "{\"type\":\"user\",\"text\":\"tail\"}\n",
        "{\"type\":\"user\"",
    ];
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), lines.concat()).unwrap();
    assert_eq!(
        serde_json::to_value(text.parse_session_messages(file.path())).unwrap(),
        serde_json::to_value(kernel.parse_session_messages(file.path())).unwrap()
    );
    for from in [0, lines[0].len() as u64, 3] {
        for budget in [1, 80, 1024] {
            let old = text
                .parse_session_delta_bounded(file.path(), from, budget)
                .unwrap();
            let new = kernel
                .parse_session_delta_bounded(file.path(), from, budget)
                .unwrap();
            assert_eq!(
                serde_json::to_value(&new).unwrap(),
                serde_json::to_value(&old).unwrap()
            );
            let window = crate::jsonl_window::read_records_from(file.path(), from, budget).unwrap();
            assert_eq!(new.new_offset, window.new_offset);
            let mut expected = Vec::new();
            let mut start = from;
            for line in window.bytes.split_inclusive(|byte| *byte == b'\n') {
                if let Ok(value) = serde_json::from_slice::<Value>(line) {
                    if value["type"] == "user" {
                        expected.push((start, value["text"].clone()));
                    }
                }
                start += line.len() as u64;
            }
            assert_eq!(new.messages.len(), expected.len());
            for (row, (start, content)) in new.messages.iter().zip(expected) {
                assert_eq!(row.record_start, Some(start));
                assert_eq!(row.record_row, 0);
                assert_eq!(row.blocks[0].data["text"], content);
            }
        }
    }
}

#[test]
fn stored_records_filter_session_and_keep_native_and_text_projection() {
    let text = fixture(json!({}));
    let kernel = fixture(
        json!({"storedHistory":{"format":"jsonl","filter":{"type":{"path":["type"],"in":["event"]}, "where":[{
            "path":["payload","thread_id"],"equals":{"placeholder":"sessionId"}
        }]}}}),
    );
    let input = concat!(
        "{\"type\":\"ignored\"}\n",
        "{\"type\":\"event\",\"payload\":{\"thread_id\":\"foreign\"},\"id\":\"x\",\"text\":\"wrong\"}\n",
        "{\"type\":\"event\",\"payload\":{\"thread_id\":\"own\"},\"id\":\"a\",\"text\":\"é🦀\"}\n"
    );
    let old = text.runtime_stored_history("own", input).unwrap();
    let new = kernel.runtime_stored_history("own", input).unwrap();
    assert_eq!(
        serde_json::to_value(&new).unwrap(),
        serde_json::to_value(&old).unwrap()
    );
    assert_eq!(new.len(), 1);
    assert_eq!(new[0].id, "a");
    assert_eq!(new[0].content, "é🦀");
    assert!(kernel
        .runtime_stored_history("own", "malformed\n")
        .unwrap()
        .is_empty());
}

#[test]
fn undeclared_record_hooks_do_not_replace_text_hooks() {
    let provider = fixture(json!({}));
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "{\"type\":\"ignored\"}\n").unwrap();
    assert!(provider
        .parse_session_delta(file.path(), 0)
        .messages
        .is_empty());
    assert!(provider
        .runtime_stored_history("own", "{\"type\":\"ignored\"}\n")
        .unwrap()
        .is_empty());
}

#[test]
fn descriptor_without_a_records_sibling_keeps_text_and_forged_offset_is_refused() {
    let declared = fixture(json!({"sessionTranscript":{"format":"jsonl","filter":{}}}));
    let manifest = declared.manifest().clone();
    let text = serde_json::to_string(&manifest).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        "record-fixture",
        r#"
        module.exports = {parseSessionDelta(){ return {messages:[]}; }};
    "#
        .into(),
        Some(text.clone()),
    )
    .unwrap();
    let absent = JsProvider::new(manifest.clone(), Arc::new(vm));
    assert!(absent.record_source("sessionTranscript").is_none());
    let vm = super::super::PluginVm::from_source_with("record-fixture", r#"
        module.exports = {parseSessionRecords(records, meta){ return {messages:[],new_offset:meta.newOffset+1}; }};
    "#.into(), Some(text)).unwrap();
    let forged = JsProvider::new(manifest, Arc::new(vm));
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "{\"type\":\"user\"}\n").unwrap();
    let window = crate::jsonl_window::records::read_records(
        file.path(),
        0,
        1024,
        declared.record_source("sessionTranscript").unwrap(),
        None,
    )
    .unwrap();
    assert!(forged.parse_session_records(&window).is_err());
}

#[test]
fn record_arguments_construct_objects_without_json_parse_and_clear_after_throw() {
    let provider = load_inline_js(
        "record-objects",
        r#"
        const parse = JSON.parse;
        module.exports = {
            parseSessionRecords(records, meta) {
                if (meta.fail) throw new Error('fixture rejection');
                JSON.parse = () => { throw new Error('JSON text crossed the record boundary'); };
                return records;
            },
            restore(records, meta) { JSON.parse = parse; return [records,meta]; },
            runtimeStoredRecords(session, records, meta) {
                JSON.parse = parse;
                return [session, records, meta];
            },
            echo(value) { return value; }
        };
    "#,
    );
    let records = json!([{"offset":1,"row":2,"value":{"__proto__":{"opaque":true},"array":[null,true,3.5,"é🦀"]}}]);
    let args = [records.clone(), json!({"fail":false})];
    let result: Value =
        super::super::vm::call_json(provider.vm(), "parseSessionRecords", &args).unwrap();
    assert_eq!(result, records);
    let args = [json!("session"), records.clone(), json!({})];
    let result: Value =
        super::super::vm::call_json(provider.vm(), "runtimeStoredRecords", &args).unwrap();
    assert_eq!(result, json!(args));
    let failed = [records, json!({"fail":true})];
    assert!(
        super::super::vm::call_json::<Value>(provider.vm(), "parseSessionRecords", &failed)
            .is_err()
    );
    let after: Value =
        super::super::vm::call_json(provider.vm(), "echo", &[json!("next")]).unwrap();
    assert_eq!(after, "next");
}

#[test]
fn one_mib_object_and_json_argument_channels_measure_same_projection() {
    let provider = load_inline_js(
        "record-measurement",
        r#"
        function project(records) { return [records.length, records[0].value.text.length]; }
        module.exports = {parseSessionRecords:project, jsonRecords:project};
    "#,
    );
    let records =
        json!([{"offset":0,"row":0,"value":{"type":"opaque","text":"x".repeat(1024*1024)}}]);
    let args = [records, json!({})];
    let started = std::time::Instant::now();
    let objects: Value =
        super::super::vm::call_json(provider.vm(), "parseSessionRecords", &args).unwrap();
    let objects_us = started.elapsed().as_micros();
    let started = std::time::Instant::now();
    let text: Value = super::super::vm::call_json(provider.vm(), "jsonRecords", &args).unwrap();
    let text_us = started.elapsed().as_micros();
    assert_eq!(objects, json!([1, 1024 * 1024]));
    assert_eq!(objects, text);
    eprintln!("record_marshalling bytes=1048576 objects_us={objects_us} json_text_us={text_us}");
}
