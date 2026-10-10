use super::*;

#[test]
fn claude_context_compaction_uses_only_post_tokens_from_a_boundary() {
    for tokens in [0, 25, 10_000_000] {
        assert_eq!(
            parse(&json!({"type":"system","subtype":"compact_boundary",
                "compactMetadata":{"preTokens":170000,"postTokens":tokens}})),
            Some(TurnContext {
                used_tokens: tokens,
                window_tokens: 0,
                cost_usd: None,
                source: "claude-jsonl-compaction".into(),
            })
        );
    }
    for tokens in [
        json!(null),
        json!(-1),
        json!(0.5),
        json!("25"),
        json!(10_000_001),
    ] {
        assert!(parse(&json!({"type":"system","subtype":"compact_boundary",
            "compactMetadata":{"postTokens":tokens}}))
        .is_none());
    }
    assert_eq!(
        parse(&json!({"type":"system","subtype":"compact_boundary",
        "compactMetadata":{"postTokens":25.0}}))
        .unwrap()
        .used_tokens,
        25
    );
    for record in [
        json!({"type":"system","subtype":"compact_boundary"}),
        json!({"type":"system","subtype":"compact_boundary","compactMetadata":{"preTokens":170000}}),
        json!({"type":"system","subtype":"other","compactMetadata":{"postTokens":25}}),
        json!({"type":"user","subtype":"compact_boundary","compactMetadata":{"postTokens":25}}),
    ] {
        assert!(parse(&record).is_none());
    }
}

#[test]
fn claude_context_usage_counts_current_input_and_redacts_payload() {
    let record = json!({"session_id":"secret", "cwd":"private", "model":{"id":"opaque"},
        "cost":{"total_cost_usd":1.25}, "context_window":{"context_window_size":200000,
        "current_usage":{"input_tokens":100,"cache_creation_input_tokens":20,"cache_read_input_tokens":30,"output_tokens":999}}});
    let context = parse(&record).unwrap();
    assert_eq!(context.used_tokens, 150);
    assert_eq!(context.window_tokens, 200000);
    assert_eq!(context.cost_usd, Some(1.25));
    let wire = serde_json::to_value(context).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 4);
    assert!(wire.get("cwd").is_none());
    assert!(wire.get("session_id").is_none());
}

#[test]
fn claude_context_usage_rejects_invalid_counts_and_unknown_window_stays_unknown() {
    for input in [json!(-1), json!(0.5), json!(10_000_001), json!("1")] {
        assert!(
            parse(&json!({"type":"assistant","message":{"usage":{"input_tokens":input}}}))
                .is_none()
        );
    }
    assert!(parse(&json!({"context_window":{"current_usage":null}})).is_none());
    let context =
        parse(&json!({"type":"assistant","message":{"usage":{"input_tokens":4}}})).unwrap();
    assert_eq!(context.window_tokens, 0);
    assert_eq!(context.source, "claude-jsonl");
    assert_eq!(context.cost_usd, None);
}

#[test]
fn claude_statusline_settings_preserve_command_padding_and_do_not_mutate_user_settings() {
    let user = json!({"statusLine":{"type":"command","command":"opaque-command --flag '$x'","padding":2},"hooks":{"opaque":[]}});
    let original = user.clone();
    let composed: Value = serde_json::from_str(&settings(&user).unwrap()).unwrap();
    assert_eq!(user, original);
    assert_eq!(composed["statusLine"]["padding"], 2);
    let command = composed["statusLine"]["command"].as_str().unwrap();
    assert!(command.ends_with("opaque-command --flag '$x'\n)"));
    assert_eq!(composed.as_object().unwrap().len(), 1);
    assert!(settings(&json!({"statusLine":false})).is_none());
    assert_eq!(
        compose_args(&["--model".into(), "opaque".into()]),
        Some(vec![
            "--model".into(),
            "opaque".into(),
            "--settings".into(),
            "{\"showThinkingSummaries\":true}".into()
        ])
    );
    assert_eq!(
        compose_args(&[FLAG.into(), "--settings".into(), "opaque".into()]),
        None
    );
}

#[test]
fn claude_thinking_settings_merge_inline_and_file_without_writes() {
    let user = json!({"hooks":{"opaque":[]},"statusLine":{"type":"command","command":"cat"},"showThinkingSummaries":false});
    let original = user.to_string();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, &original).unwrap();
    for raw in [original.clone(), path.to_string_lossy().into_owned()] {
        for args in [
            vec!["--settings".into(), raw.clone()],
            vec![format!("--settings={raw}")],
        ] {
            let composed = compose_args(&args).unwrap();
            assert_eq!(composed.len(), 2);
            let overlay: Value = serde_json::from_str(&composed[1]).unwrap();
            assert_eq!(overlay["showThinkingSummaries"], true);
            assert_eq!(overlay["hooks"], user["hooks"]);
            assert_eq!(overlay["statusLine"], user["statusLine"]);
        }
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    assert!(compose_args(&["--settings".into()]).is_none());
}

#[test]
fn claude_settings_command_uses_a_file_path_without_inline_json() {
    let path = "/instance data/runtime/task-files/opaque.ct-settings.json";
    let args = settings_file_args(compose_args(&[]).unwrap(), path.into());
    let command = crate::provider::append_launch_args("claude".into(), &args);
    assert!(command.contains(path));
    assert!(!command.contains("showThinkingSummaries"));
    assert!(!command.contains('{'));
    assert_eq!(args, vec!["--settings", path]);
}
