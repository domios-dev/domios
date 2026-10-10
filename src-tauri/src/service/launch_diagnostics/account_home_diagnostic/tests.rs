use super::*;

fn hook_document(event: &str, hook: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"hooks": {event: [{"hooks": [hook]}]}})).unwrap()
}

#[test]
fn account_home_diagnostic_checks_effective_home_with_launch_precedence() {
    let inherited = HashMap::from([("CODEX_HOME".into(), "other-home".into())]);
    assert_eq!(
        effective_home(&HashMap::new(), &inherited, "CODEX_HOME"),
        Some(("other-home", "inherited"))
    );
    for value in ["", "launch-home"] {
        let launch = HashMap::from([("CODEX_HOME".into(), value.into())]);
        assert_eq!(
            effective_home(&launch, &inherited, "CODEX_HOME"),
            (!value.is_empty()).then_some((value, "launch"))
        );
    }
    assert_eq!(
        effective_home(&HashMap::new(), &inherited, "CLAUDE_CONFIG_DIR"),
        None
    );
}

#[test]
fn account_home_diagnostic_ignores_empty_inherited_values() {
    let inherited = HashMap::from([("CLAUDE_CONFIG_DIR".into(), "  ".into())]);
    assert_eq!(
        effective_home(&HashMap::new(), &inherited, "CLAUDE_CONFIG_DIR"),
        None
    );
}

#[test]
fn account_home_diagnostic_accepts_both_marked_command_formats() {
    for hook in &HOME_HOOKS {
        for separator in [": ", ":"] {
            let bytes = hook_document(
                "SessionStart",
                serde_json::json!({
                    "type": "command",
                    "command": format!("callback X-CodeTerm-Hook{separator}{}", hook.marker),
                }),
            );
            assert_eq!(
                classify_hook_document(&bytes, hook.marker),
                HookObservation::Marked
            );
        }
    }
}

#[test]
fn account_home_diagnostic_rejects_foreign_and_wrong_event_hooks() {
    let marker = HOME_HOOKS[0].marker;
    for (event, hook) in [
        (
            "Stop",
            serde_json::json!({"type": "command", "command": format!("X-CodeTerm-Hook: {marker}")}),
        ),
        (
            "SessionStart",
            serde_json::json!({"type": "prompt", "command": format!("X-CodeTerm-Hook: {marker}")}),
        ),
        (
            "SessionStart",
            serde_json::json!({"type": "command", "command": "foreign /hook/session_start", "_source": marker}),
        ),
        (
            "SessionStart",
            serde_json::json!({"type": "command", "command": "X-CodeTerm-Hook: other-marker"}),
        ),
        (
            "SessionStart",
            serde_json::json!({"type": "command", "command": format!("X-CodeTerm-Hook: {marker}0")}),
        ),
    ] {
        assert_eq!(
            classify_hook_document(&hook_document(event, hook), marker),
            HookObservation::Unmarked
        );
    }
}

#[test]
fn account_home_diagnostic_distinguishes_invalid_from_unmarked() {
    for bytes in [b"not-json".as_slice(), b"null", b"[]"] {
        assert_eq!(
            classify_hook_document(bytes, HOME_HOOKS[0].marker),
            HookObservation::Invalid
        );
    }
    assert_eq!(
        classify_hook_document(b"{}", HOME_HOOKS[0].marker),
        HookObservation::Unmarked
    );
}

#[test]
fn account_home_diagnostic_does_not_create_or_modify_home_files() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("settings.json");
    assert_eq!(
        observe_hook_file(&path, HOME_HOOKS[0].marker),
        HookObservation::Missing
    );
    assert!(!path.exists());
    let bytes = b"{\"foreign\":true}";
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(
        observe_hook_file(&path, HOME_HOOKS[0].marker),
        HookObservation::Unmarked
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn account_home_diagnostic_bounds_hook_document_reads() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("hooks.json");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_HOOK_FILE_BYTES + 1).unwrap();
    assert_eq!(
        observe_hook_file(&path, HOME_HOOKS[1].marker),
        HookObservation::Oversized
    );
}
