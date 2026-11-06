use super::super::{chat_backfill_core, chat_start_core};
use codeterm_service_core::agent::chat_types::ChatMsg;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use codeterm_service_core::service::tab_chat::results::ChatOffsetKind;
use codeterm_service_core::service::tab_chat::tab_chat_snapshot;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

fn capture_rows() -> Vec<Value> {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/chat/air-transcript.jsonl"
    ))
    .lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect()
}

fn captured_provider(path: &Path) -> (JsProvider, String) {
    let descriptor: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/chat/air-transcript.json"
    )))
    .unwrap();
    let root =
        super::workspace_plugins_root().join(descriptor["pluginDirectory"].as_str().unwrap());
    let manifest_json = std::fs::read_to_string(root.join("plugin.json")).unwrap();
    let manifest: PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let provider_id = manifest.id.clone();
    let mut source = std::fs::read_to_string(root.join("plugin.js")).unwrap();
    source.push_str(&format!(
        "\nmodule.exports.default.sessionJsonlPath = function() {{ return {}; }};",
        json!(path.to_string_lossy())
    ));
    let vm = PluginVm::from_source_with(&provider_id, source, Some(manifest_json)).unwrap();
    (JsProvider::new(manifest, Arc::new(vm)), provider_id)
}

fn padded_image(mut record: Value, bytes: usize) -> Value {
    let data = &mut record["message"]["content"][0]["content"][0]["source"]["data"];
    assert!(data.is_string());
    *data = Value::String(String::new());
    let overhead = record.to_string().len() + 1;
    record["message"]["content"][0]["content"][0]["source"]["data"] =
        Value::String("x".repeat(bytes - overhead));
    assert_eq!(record.to_string().len() + 1, bytes);
    record
}

fn wire_rows(rows: &[ChatMsg]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            let mut wire = serde_json::to_value(row).unwrap();
            if row
                .tool_result
                .as_ref()
                .is_some_and(|body| body.len() > 2048)
            {
                wire["toolResult"] = json!("[image result body omitted from wire capture]");
            }
            wire
        })
        .collect()
}

#[test]
fn air_chat_start_oversized_boundary_and_frontend_pages() {
    let captured = capture_rows();
    let image = captured
        .iter()
        .find(|row| row["uuid"] == "22a69a2e-8769-4049-afd5-dbffbce3dad6")
        .unwrap();
    let reply = captured
        .iter()
        .find(|row| row["uuid"] == "f93c4ed2-d8e7-4638-9091-43c1ec91812b")
        .unwrap();
    let owner = captured
        .iter()
        .find(|row| row["uuid"] == "d157e783-4579-4174-9c97-80a60ef44845")
        .unwrap();
    let blob = captured
        .iter()
        .find(|row| {
            row["type"] == "api-request-blob"
                && row["message"]["content"][0]["content"][0]["source"]["data"].is_string()
        })
        .unwrap();
    let session = owner["sessionId"].as_str().unwrap();
    let reply_id = format!("{}-0", reply["uuid"].as_str().unwrap());
    let mut cases = Vec::new();
    for shape in ["replies-after-image", "reply-behind-images", "silent-tail"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        let filler = json!({"type":"api-request-blob","message":{"role":"user","content":"x".repeat(65536)}});
        for _ in 0..512 {
            writeln!(file, "{filler}").unwrap();
        }
        let body_start = file.metadata().unwrap().len();
        writeln!(file, "{owner}").unwrap();
        if shape != "replies-after-image" {
            writeln!(file, "{reply}").unwrap();
        }
        let first_image_start = file.metadata().unwrap().len();
        writeln!(file, "{}", padded_image(image.clone(), 1_335_564)).unwrap();
        writeln!(file, "{}", padded_image(blob.clone(), 667_748)).unwrap();
        let last_image_start = file.metadata().unwrap().len();
        if shape != "silent-tail" {
            let mut second = padded_image(image.clone(), 1_335_564);
            second["uuid"] = json!("second-image");
            writeln!(file, "{second}").unwrap();
        }
        let after_images = file.metadata().unwrap().len();
        if shape == "replies-after-image" {
            writeln!(file, "{reply}").unwrap();
        }
        if shape != "silent-tail" {
            let mut last = owner.clone();
            last["uuid"] = json!("latest-owner");
            writeln!(file, "{last}").unwrap();
        }
        file.flush().unwrap();
        let total = file.metadata().unwrap().len();
        drop(file);
        let (provider, provider_id) = captured_provider(&path);
        let (mut state, tab_id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(provider_id);
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some("air-start-launch".into());
        tab.cwd = Some(directory.path().to_string_lossy().into_owned());
        let fixture = ServiceFixture::new(state);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));
        let service = Arc::new(fixture.service.clone());
        let start = chat_start_core(&service, tab_id.clone(), "cold".into(), None).unwrap();
        assert_eq!(start.offset, total);
        assert!(start.start_offset >= body_start);
        assert!(start.offset - start.start_offset <= 1024 * 1024);
        let rows = start.messages.as_ref().unwrap();
        if shape == "replies-after-image" {
            assert!(last_image_start < total - 1024 * 1024 && total - 1024 * 1024 < after_images);
            assert!(rows.iter().any(|row| row.id == reply_id));
            assert_eq!(start.start_offset, after_images);
        } else {
            assert!(!rows.iter().any(|row| row.id == reply_id));
        }
        let remount = chat_start_core(&service, tab_id.clone(), "warm".into(), None).unwrap();
        assert_eq!(remount.messages, start.messages);
        let (stateless, count) = tab_chat_snapshot(&service, &tab_id, 0).unwrap();
        assert_eq!(count, rows.len());
        assert_eq!(
            stateless.iter().map(|row| &row.id).collect::<Vec<_>>(),
            rows.iter().map(|row| &row.id).collect::<Vec<_>>()
        );
        if shape == "silent-tail" {
            assert_eq!(count, 0);
        }
        let mut pages = Vec::new();
        let mut cursor = start.start_offset;
        for _ in 0..8 {
            if cursor == 0 {
                break;
            }
            let page = chat_backfill_core(
                &service,
                tab_id.clone(),
                cursor,
                Some(ChatOffsetKind::Bytes),
            )
            .unwrap();
            assert!(page.start_offset < cursor);
            assert_eq!(page.end_offset, cursor);
            cursor = page.start_offset;
            pages.push(json!({"start_offset":cursor,"end_offset":page.end_offset,"offset_kind":"bytes",
                "messages":wire_rows(&page.messages),"source_session_id":session,"session_binding_nonce":"air-start-launch"}));
        }
        assert!(
            rows.iter().any(|row| row.id == reply_id)
                || pages.iter().any(|page| page["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["id"] == reply_id))
        );
        cases.push(json!({"name":shape,"reply_id":reply_id,"body_start":body_start,"first_image_start":first_image_start,
            "start":{"kind":"snapshot","text":"","pending":false,"start_offset":start.start_offset,
                "offset":start.offset,"offset_kind":"bytes","messages":wire_rows(rows),
                "source_session_id":session,"session_binding_nonce":"air-start-launch"},"pages":pages}));
        service.chat_registry.shutdown_tab(&tab_id);
    }
    if let Some(path) = std::env::var_os("DOMIOS_AIR_CHAT_CAPTURE") {
        std::fs::write(path, serde_json::to_string_pretty(&cases).unwrap() + "\n").unwrap();
    }
}
