use super::*;
use serde_json::json;

#[test]
fn parity_claude_context_usage() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    for record in [
        json!({"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":10,"cache_creation_input_tokens":2,"cache_read_input_tokens":3,"output_tokens":999}},"cost":{"total_cost_usd":1.5},"cwd":"secret"}),
        json!({"type":"assistant","message":{"usage":{"input_tokens":20}}}),
        json!({"type":"assistant","message":{"usage":{"input_tokens":-1}}}),
        json!({"context_window":{"current_usage":null}}),
        json!({"context_window":{"context_window_size":-1,"current_usage":{"input_tokens":10}}}),
        json!({"context_window":{"context_window_size":null,"current_usage":{"input_tokens":10}}}),
        json!({}),
    ] {
        assert_eq!(
            native.parse_context_usage(&record),
            js.parse_context_usage(&record)
        );
    }
}

#[test]
fn parity_claude_jsonl_context_metadata_matches_native_normalization() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let record = json!({"type":"assistant","uuid":"opaque","message":{
        "content":[{"type":"text","text":"opaque"}],"usage":{"input_tokens":7,"cache_read_input_tokens":3,"output_tokens":99}}});
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("own.jsonl");
    std::fs::write(&path, format!("{record}\n")).unwrap();
    let delta = js.parse_session_delta(&path, 0);
    assert_eq!(delta.messages.len(), 1);
    assert_eq!(
        delta.messages[0].context_usage,
        native.parse_context_usage(&record)
    );
}

#[test]
fn parity_claude_per_launch_statusline_settings() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    for args in [
        vec![],
        vec!["--codeterm-statusline".into()],
        vec![
            "--codeterm-statusline".into(),
            "--settings".into(),
            "{}".into(),
        ],
    ] {
        let params = crate::provider::AgentLaunchParams {
            args: Some(args.clone()),
            ..Default::default()
        };
        assert_eq!(
            native.build_launch_command(&params),
            js.build_launch_command(&params)
        );
        let resume = crate::provider::AgentResumeParams {
            session_id: "opaque".into(),
            args,
            ..Default::default()
        };
        assert_eq!(
            native.build_resume_command_with_context(&resume),
            js.build_resume_command_with_context(&resume)
        );
    }
}
