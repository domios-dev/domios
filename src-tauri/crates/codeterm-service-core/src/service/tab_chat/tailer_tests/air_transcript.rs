use super::*;
use crate::service::tab_chat::{rich_rows_to_chat_msgs, session_window};
use serde_json::Value;

fn fixture(padded: bool) -> Vec<Value> {
    let text = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/chat/air-transcript.jsonl"
    ));
    text.lines()
        .map(|line| {
            let mut row: Value = serde_json::from_str(line).unwrap();
            if padded && row["type"] == "api-request-blob" {
                row["fixturePadding"] = Value::String("x".repeat(667_748));
            }
            if padded
                && matches!(
                    row["uuid"].as_str(),
                    Some(
                        "22a69a2e-8769-4049-afd5-dbffbce3dad6"
                            | "233c4442-ecae-4ead-a7b2-36ef4914ddbf"
                    )
                )
            {
                row["message"]["content"][0]["content"] = Value::String("x".repeat(1_335_564));
            }
            row
        })
        .collect()
}

fn registry() -> (Arc<AgentRegistry>, String) {
    let descriptor: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/chat/air-transcript.json"
    )))
    .unwrap();
    let root = workspace_plugins_root().join(descriptor["pluginDirectory"].as_str().unwrap());
    let manifest_json = std::fs::read_to_string(root.join("plugin.json")).unwrap();
    let manifest: crate::plugin::js_runtime::PluginManifest =
        serde_json::from_str(&manifest_json).unwrap();
    let provider_id = manifest.id.clone();
    let source = std::fs::read_to_string(root.join("plugin.js")).unwrap();
    let vm = crate::plugin::js_runtime::PluginVm::from_source_with(
        &provider_id,
        source,
        Some(manifest_json),
    )
    .unwrap();
    let provider = crate::plugin::js_runtime::JsProvider::new(manifest, Arc::new(vm));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(provider));
    (Arc::new(registry), provider_id)
}

#[test]
fn air_transcript_snapshot_and_history_preserve_every_assistant() {
    for padded in [false, true] {
        let rows = fixture(padded);
        let expected: Vec<_> = rows
            .iter()
            .filter(|row| row["type"] == "assistant")
            .map(|row| format!("{}-0", row["uuid"].as_str().unwrap()))
            .collect();
        let text = rows
            .iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>();
        let root = TempDir::new().unwrap();
        let path = root.path().join("air.jsonl");
        std::fs::write(&path, &text).unwrap();
        let (registry, provider_id) = registry();
        let (live, end) =
            session_window::try_parse_window(&registry, &provider_id, &path, 0, text.len() as u64)
                .unwrap();
        assert_eq!(end, text.len() as u64);
        let live = rich_rows_to_chat_msgs(&live, 0);
        let snapshot = session_window::try_snapshot(&registry, &provider_id, &path).unwrap();
        let mut cursor = snapshot.start_offset;
        let mut history = snapshot.messages;
        while cursor > 0 {
            let page =
                session_window::parse_page_before(&registry, &provider_id, &path, cursor).unwrap();
            assert!(page.start_offset < cursor);
            assert_eq!(page.end_offset, cursor);
            cursor = page.start_offset;
            let mut earlier = rich_rows_to_chat_msgs(&page.messages, cursor);
            earlier.extend(history);
            history = earlier;
        }
        let assistant_ids = |messages: &[crate::agent::chat_types::ChatMsg]| {
            messages
                .iter()
                .filter(|row| matches!(row.kind.as_str(), "assistant" | "thinking" | "tool_call"))
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        };
        eprintln!(
            "air padded={padded}: expected={}, live={}, snapshot+history={}",
            expected.len(),
            assistant_ids(&live).len(),
            assistant_ids(&history).len()
        );
        assert_eq!(assistant_ids(&live), expected, "live padded={padded}");
        assert_eq!(
            assistant_ids(&history),
            expected,
            "snapshot padded={padded}"
        );
        let notice = "0b94af20-b836-4da2-8ab4-6b5a77924d5c-0";
        assert_eq!(history.iter().filter(|row| row.id == notice).count(), 1);
        assert_eq!(
            history.iter().find(|row| row.id == notice).unwrap().kind,
            "user"
        );
    }
}
