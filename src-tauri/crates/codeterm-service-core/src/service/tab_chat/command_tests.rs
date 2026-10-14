use super::*;
use crate::agent::provider::AgentProvider;
use crate::plugin::agent_claude::ClaudeProvider;

fn captured() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/model-switch/live-command-records.json"
    )))
    .unwrap()
}

fn rich(record: &serde_json::Value) -> RichMessage {
    serde_json::from_value(serde_json::json!({"role":"user", "blocks":[{
        "kind":"text", "data":{"text":record["message"]["content"]}
    }]}))
    .unwrap()
}

#[test]
fn captured_switch_wrappers_never_become_chat_user_rows() {
    let provider = ClaudeProvider::new();
    let source = provider.model_switch_commands().unwrap().transcript;
    let records = captured();
    assert_eq!(records.len(), 4);
    for record in records {
        assert!(source.is_command_record(&record));
        let mut message = rich(&record);
        enrich_interactive(&mut message, &provider);
        assert!(rich_to_chat_msgs(&message, 0).is_empty());
        assert!(rich_to_chat_msgs_at(&message, RowOrigin::at(0, 0)).is_empty());
        assert!(rich_to_chat_msgs_live(&message, 0).is_empty());
    }
}

#[test]
fn switch_wrappers_preserve_real_text_attachments_and_tool_results() {
    let provider = ClaudeProvider::new();
    for content in [
        "inspect the build".to_owned(),
        format!(
            "{}\ninspect the build",
            captured()[0]["message"]["content"].as_str().unwrap()
        ),
    ] {
        let record = serde_json::json!({"message":{"content":content}});
        let mut message = rich(&record);
        enrich_interactive(&mut message, &provider);
        assert!(rich_to_chat_msgs(&message, 0)
            .iter()
            .any(|row| row.kind == "user"));
    }
    for block in [
        serde_json::json!({"kind":"image", "data":{"source":{"media_type":"image/png","data":"image"}}}),
        serde_json::json!({"kind":"tool_result", "data":{"tool_use_id":"tool","content":"output"}}),
    ] {
        let mut message = rich(&captured()[0]);
        message.blocks.push(serde_json::from_value(block).unwrap());
        let count = message.blocks.len();
        enrich_interactive(&mut message, &provider);
        assert_eq!(message.blocks.len(), count);
    }
}

#[test]
fn unsupported_prompt_answers_remove_question_widgets() {
    let provider = ClaudeProvider::new();
    let mut message: RichMessage = serde_json::from_value(serde_json::json!({
        "role":"assistant",
        "blocks":[{"kind":"tool_use","data":{
            "name":"AskUserQuestion","id":"question-tool",
            "input":{"questions":[{"question":"opaque?","header":"opaque",
                "options":[{"label":"a"},{"label":"b"}]}]}
        }}]
    }))
    .unwrap();
    let questions = provider.parse_interactive_events(&message);
    assert!(!questions.is_empty());
    message.interactive = questions;
    enrich_interactive(&mut message, &provider);
    assert!(message.interactive.is_empty());
    assert_eq!(message.blocks.len(), 1);
}
