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
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":25}}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":25.0}}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":0}}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":-1}}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":0.5}}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":"25"}}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"postTokens":10000001}}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"preTokens":170000}}),
        json!({"type":"system","subtype":"other","compactMetadata":{"postTokens":25}}),
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
fn parity_claude_compact_boundary_context_reaches_delta_metadata() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let record = json!({"type":"system","subtype":"compact_boundary","uuid":"compact-own",
        "compactMetadata":{"preTokens":170000,"postTokens":25000}});
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("own.jsonl");
    std::fs::write(&path, format!("{record}\n")).unwrap();
    let delta = js.parse_session_delta(&path, 0);
    assert_eq!(delta.messages.len(), 1);
    let context = delta.messages[0].context_usage.as_ref().unwrap();
    assert_eq!(context.used_tokens, 25000);
    assert_eq!(Some(context.clone()), native.parse_context_usage(&record));
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
        vec!["--settings={\"showThinkingSummaries\":false,\"hooks\":{\"opaque\":[]}}".into()],
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
            settings_command(|| native.build_launch_command(&params)),
            settings_command(|| js.build_launch_command(&params))
        );
        let launch = native.build_launch_command(&params);
        assert!(launch.contains(".ct-settings.json"));
        assert!(!launch.contains("showThinkingSummaries"));
        assert_eq!(launch.matches("--settings").count(), 1);
        let resume = crate::provider::AgentResumeParams {
            session_id: "opaque".into(),
            args,
            ..Default::default()
        };
        assert_eq!(
            settings_command(|| native.build_resume_command_with_context(&resume).unwrap()),
            settings_command(|| js.build_resume_command_with_context(&resume).unwrap())
        );
    }
    assert_eq!(
        settings_command(|| native.build_resume_command("own", false).unwrap()),
        settings_command(|| js.build_resume_command("own", false).unwrap()),
    );
}

pub(super) fn settings_command(
    build: impl FnOnce() -> String,
) -> (String, serde_json::Value, Option<Vec<u8>>) {
    let temp = tempfile::tempdir().unwrap();
    let (command, paths) =
        codeterm_util::task_files::capture_in(temp.path(), "settings-parity", build);
    let path = paths
        .iter()
        .find(|path| path.to_string_lossy().ends_with(".ct-settings.json"))
        .expect("launch must write settings to its captured artifact store");
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(settings["showThinkingSummaries"], true);
    assert_eq!(command.matches("--settings").count(), 1);
    assert!(!command.contains("showThinkingSummaries"));
    assert!(command.contains(path.to_str().unwrap()));
    let prompt_paths: Vec<_> = paths
        .iter()
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".ct-task-")
        })
        .collect();
    assert!(prompt_paths.len() <= 1);
    let mut normalized = command.replace(path.to_str().unwrap(), "SETTINGS_PATH");
    let prompt = prompt_paths.first().map(|path| {
        assert_eq!(command.matches("--append-system-prompt-file").count(), 1);
        assert!(command.contains(path.to_str().unwrap()));
        normalized = normalized.replace(path.to_str().unwrap(), "PROMPT_PATH");
        std::fs::read(path).unwrap()
    });
    (normalized, settings, prompt)
}

#[test]
fn parity_claude_launch_prompt_artifacts() {
    let (native, js) = pair();
    for prompt in [None, Some(""), Some("opaque-task\n$literal `literal` λ")] {
        let launch = crate::provider::AgentLaunchParams {
            task: prompt.map(str::to_owned),
            ..Default::default()
        };
        let native_launch = settings_command(|| native.build_launch_command(&launch));
        let js_launch = settings_command(|| js.build_launch_command(&launch));
        assert!(native_launch.2.is_some());
        assert_eq!(native_launch, js_launch);
        let resume = crate::provider::AgentResumeParams {
            session_id: "fixture".into(),
            system_prompt: prompt.map(str::to_owned),
            ..Default::default()
        };
        let native_resume =
            settings_command(|| native.build_resume_command_with_context(&resume).unwrap());
        let js_resume = settings_command(|| js.build_resume_command_with_context(&resume).unwrap());
        assert!(native_resume.2.is_some());
        assert_eq!(native_resume, js_resume);
    }
}
