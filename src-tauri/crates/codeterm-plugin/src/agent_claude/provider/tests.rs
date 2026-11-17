use super::*;

#[test]
fn test_claude_provider_id() {
    let p = ClaudeProvider::new();
    assert_eq!(p.id(), "claude");
    assert_eq!(p.display_name(), "Claude Code");
    assert_eq!(p.binary_name(), "claude");
}

#[test]
fn claude_auto_approve_flags_has_dangerously_skip() {
    let p = ClaudeProvider::new();
    assert_eq!(
        p.auto_approve_flags(),
        vec!["--dangerously-skip-permissions"]
    );
}

#[test]
fn claude_compact_command() {
    let p = ClaudeProvider::new();
    assert_eq!(p.compact_command(), Some("/compact"));
}

#[test]
fn claude_fresh_context_command() {
    let p = ClaudeProvider::new();
    assert_eq!(p.fresh_context_command(), Some("/new"));
}

#[test]
fn claude_switches_model_by_restart_resume() {
    let p = ClaudeProvider::new();
    assert_eq!(
        p.model_switch(),
        crate::provider::ModelSwitchMode::LiveCommand
    );
    assert_eq!(p.model_launch_args("sonnet"), vec!["--model", "sonnet"]);
}

#[test]
fn claude_prompt_response_is_highlight_relative() {
    let p = ClaudeProvider::new();
    let screen = "Choose:\n  1. Opus\n❯ 2. Sonnet\n  3. Haiku\nEnter to select";
    assert_eq!(p.build_prompt_response(1, screen), vec![b"\r".to_vec()]);
    assert_eq!(
        p.build_prompt_response(0, screen),
        vec![b"k".to_vec(), b"\r".to_vec()]
    );
    assert_eq!(
        p.build_prompt_response(2, screen),
        vec![b"j".to_vec(), b"\r".to_vec()]
    );
}

#[test]
fn current_folder_trust_prompt_parses_and_responds_from_the_refusal_focus() {
    let screen = std::fs::read_to_string(
        std::path::PathBuf::from(crate::test_support::fixture_manifest_dir())
            .join("../../../plugins/provider-claude/tests/fixtures/folder-trust")
            .join("quick-safety-no-first.txt"),
    )
    .expect("captured folder-trust fixture");
    let provider = ClaudeProvider::new();
    let prompt = provider.parse_prompt(&screen).expect("two-option choice");
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.label.split_whitespace().next().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["No,", "Yes,"]
    );
    assert!(prompt.options[0].label.to_lowercase().contains("exit"));
    assert!(prompt.options[1].label.to_lowercase().contains("trust"));
    assert_eq!(highlighted_option_index(&screen), Some(0));
    assert_eq!(
        provider.build_prompt_response(1, &screen),
        vec![b"\x1b[B".to_vec(), b"\r".to_vec()]
    );
}

#[test]
fn claude_describe_injects_usage_tracking_section() {
    let p = ClaudeProvider::new();
    let desc = p.describe();
    let has = desc.settings_schema.iter().any(|f| matches!(
        f,
        crate::provider::SettingField::Section { title, .. } if title.starts_with("Usage Tracking")
    ));
    assert!(
        has,
        "Claude has usage_parser — describe() must auto-inject the Usage Tracking toggle"
    );
}

#[test]
fn claude_exit_command() {
    let p = ClaudeProvider::new();
    assert_eq!(p.exit_command(), Some("/exit"));
}

#[test]
fn claude_supports_lazy_skills() {
    let p = ClaudeProvider::new();
    assert!(p.supports_lazy_skills());
}

#[test]
fn claude_install_skills_default_noop_is_ok() {
    let p = ClaudeProvider::new();
    // Default implementation is a no-op that returns Ok(()) — ClaudeProvider doesn't override it yet (real skill delivery comes with Phase 2).
    assert!(p.install_skills(&[]).is_ok());
    assert!(p.uninstall_skills(&[]).is_ok());
}

#[test]
fn test_claude_does_not_inject_session_id() {
    // Claude 2.1.x drops the transcript when --session-id is passed to a fresh interactive launch; we detect the id instead.
    let p = ClaudeProvider::new();
    assert_eq!(
        p.session_id_injection(),
        crate::provider::SessionIdInjection::None
    );
}

#[test]
fn test_readiness_strategy() {
    let p = ClaudeProvider::new();
    match p.readiness_strategy() {
        ReadinessStrategy::PollShellPrompt { timeout } => {
            assert_eq!(timeout, Duration::from_secs(30));
        }
        _ => panic!("Expected PollShellPrompt"),
    }
}

#[test]
fn claude_uses_tui_input_drained_for_submit_verification() {
    let p = ClaudeProvider::new();
    assert!(matches!(
        p.verify_submit_strategy(),
        VerifySubmitStrategy::TuiInputDrained { .. }
    ));
}

#[test]
fn claude_describe_assembles_full_descriptor() {
    let p = ClaudeProvider::new();
    let desc = p.describe();
    assert_eq!(desc.id, "claude");
    assert_eq!(desc.display_name, "Claude Code");
    assert_eq!(desc.icon_hint.as_deref(), Some("✳"));
    assert_eq!(desc.default_model.as_deref(), Some("sonnet"));
    assert!(desc.supports_lazy_skills);
    assert!(desc.has_hooks);
    assert!(!desc.settings_schema.is_empty());
}

#[test]
fn claude_settings_schema_round_trips_as_json() {
    // Snapshot/golden — catches accidental schema breakage.
    let p = ClaudeProvider::new();
    let desc = p.describe();
    let json = serde_json::to_string(&desc).expect("serialize");
    let decoded: crate::provider::ProviderDescriptor =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(desc, decoded);
}

#[test]
fn claude_describe_includes_notifications_schema() {
    let p = ClaudeProvider::new();
    let desc = p.describe();
    assert!(!desc.notifications_schema.is_empty());
}
