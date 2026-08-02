use super::*;

#[test]
fn missing_report_binding_is_a_loud_orchestration_incident() {
    let message = missing_report_binding_incident("tab-worker");
    assert!(message.contains("ORCHESTRATION AUTHORITY REFUSED"));
    assert!(message.contains("no explicit plan/task assignment"));
    assert!(message.contains("Impact:"));
    assert!(message.contains("Required:"));
    assert!(message.contains("no plan or task was guessed"));
    assert!(message.contains("tab-worker"));
}

#[test]
fn agent_models_formats_descriptor_models_from_api_snapshot() {
    let provider = serde_json::json!({
        "id": "codex",
        "default_model": "live-codex",
        "available_models": [
            {
                "id": "live-codex",
                "display_name": "Live Codex",
                "description": "catalog snapshot"
            }
        ]
    });

    let out = format_agent_models(&provider, "codex");

    assert!(out.contains("live-codex"));
    assert!(out.contains("Live Codex (default)"));
    assert!(out.contains("catalog snapshot"));
    assert!(!out.contains("static-codex"));
}

#[test]
fn env_default_uses_explicit_value_before_worker_env() {
    std::env::set_var("CODETERM_TEST_REPORT_DEFAULT", "from-env");
    assert_eq!(
        env_default(Some("explicit".into()), "CODETERM_TEST_REPORT_DEFAULT"),
        Some("explicit".into())
    );
    assert_eq!(
        env_default(None, "CODETERM_TEST_REPORT_DEFAULT"),
        Some("from-env".into())
    );
    std::env::remove_var("CODETERM_TEST_REPORT_DEFAULT");
}

#[test]
fn print_report_notices_is_noop_in_json_mode() {
    // json=true should not print anything (we rely on the JSON output channel)
    let val = serde_json::json!({ "ok": true, "notices": ["stale plan"] });
    print_report_notices(true, &val);
}

#[test]
fn print_report_notices_handles_missing_notices_key() {
    let val = serde_json::json!({ "ok": true, "report_id": "r1" });
    print_report_notices(false, &val); // must not panic
}

#[test]
fn print_report_notices_handles_empty_notices_array() {
    let val = serde_json::json!({ "ok": true, "notices": [] });
    print_report_notices(false, &val); // must not panic
}

#[test]
fn report_refusal_text_starts_and_ends_with_not_sent() {
    let refusal = ReportRefusal {
        reason: "readiness report refused: this git worktree has uncommitted lane changes."
            .to_string(),
        dirty_files: vec!["src/main.rs".to_string()],
    };

    let out = format_report_refusal_text(&refusal, true, Some("rep-1"), None);
    let lines = out.lines().collect::<Vec<_>>();

    assert_eq!(lines.first().copied(), Some("REPORT NOT SENT"));
    assert_eq!(lines.last().copied(), Some("REPORT NOT SENT"));
    assert!(out.contains("git status --short"));
    assert!(out.contains("codeterm agent report --status partial"));
    assert!(out.contains("Diagnostic report sent to parent: rep-1"));
}

#[test]
fn task_file_round_trips_shell_metacharacters_byte_exact() {
    let body = "run `whoami` then $(rm -rf /) and ${HOME}\nline two\twith tab\n";
    let path = std::env::temp_dir().join(format!("ct-task-file-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&path, body).unwrap();

    let resolved = resolve_spawn_task(
        None,
        Some(path.to_string_lossy().into_owned()),
        None,
        &mut &b""[..],
    )
    .unwrap();

    std::fs::remove_file(&path).ok();
    assert_eq!(resolved, Some(body.to_string()));
}

#[test]
fn tab_description_validation_accepts_200_and_rejects_201() {
    let ok = "x".repeat(MAX_TAB_DESCRIPTION_CHARS);
    let too_long = "x".repeat(MAX_TAB_DESCRIPTION_CHARS + 1);

    assert_eq!(normalize_tab_description(&ok).unwrap(), Some(ok));
    assert!(normalize_tab_description(&too_long).is_err());
    assert_eq!(normalize_tab_description("  ").unwrap(), None);
    assert_eq!(
        normalize_tab_description("  running tests  ").unwrap(),
        Some("running tests".into())
    );
}

#[test]
fn task_file_dash_reads_stdin_byte_exact() {
    let body = "stdin `backticks` and $(date) survive\n";
    let mut stdin = body.as_bytes();
    let resolved = resolve_spawn_task(None, Some("-".to_string()), None, &mut stdin).unwrap();
    assert_eq!(resolved, Some(body.to_string()));
}

#[test]
fn charter_file_dash_reads_stdin_byte_exact() {
    let body = "observe the group\nnudge only when stalled\n";
    let mut stdin = body.as_bytes();
    let resolved = resolve_spawn_charter(None, Some("-".to_string()), &mut stdin).unwrap();
    assert_eq!(resolved, Some(body.to_string()));
}

#[test]
fn parse_watcher_policy_duration_to_seconds() {
    assert_eq!(parse_duration_secs("5", "--interval").unwrap(), 300);
    assert_eq!(parse_duration_secs("90s", "--interval").unwrap(), 90);
    assert_eq!(parse_duration_secs("10m", "--nudge-cooldown").unwrap(), 600);
    assert_eq!(parse_duration_secs("1h30m", "--interval").unwrap(), 5_400);
}

#[test]
fn task_and_task_file_together_is_an_error() {
    let err = resolve_spawn_task(
        Some("inline".to_string()),
        Some("-".to_string()),
        None,
        &mut &b""[..],
    )
    .unwrap_err();
    assert!(err.contains("not both"), "unexpected error: {}", err);
}

#[test]
fn missing_task_file_reports_unreadable() {
    let path = std::env::temp_dir().join(format!("ct-nope-{}.txt", uuid::Uuid::new_v4()));
    let err = resolve_spawn_task(
        None,
        Some(path.to_string_lossy().into_owned()),
        None,
        &mut &b""[..],
    )
    .unwrap_err();
    assert!(err.contains("failed to read --task-file"));
}

#[test]
fn neither_task_nor_task_file_is_none() {
    assert_eq!(resolve_spawn_task(None, None, None, &mut &b""[..]).unwrap(), None);
}

#[test]
fn relative_task_file_resolves_from_spawn_cwd() {
    let base = std::env::temp_dir().join(format!("ct-task-cwd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("brief.txt"), "cwd-relative body\n").unwrap();

    let resolved = resolve_spawn_task(
        None,
        Some("brief.txt".into()),
        Some(base.to_str().unwrap()),
        &mut &b""[..],
    )
    .unwrap();

    std::fs::remove_dir_all(&base).ok();
    assert_eq!(resolved.as_deref(), Some("cwd-relative body\n"));
}

#[test]
fn missing_relative_task_file_reports_requested_path_base_and_resolution() {
    let base = std::env::temp_dir().join(format!("ct-task-cwd-{}", uuid::Uuid::new_v4()));
    let err = resolve_spawn_task(
        None,
        Some("missing.txt".into()),
        Some(base.to_str().unwrap()),
        &mut &b""[..],
    )
    .unwrap_err();
    assert!(err.contains("missing.txt"));
    assert!(err.contains(&base.display().to_string()));
    assert!(err.contains(&base.join("missing.txt").display().to_string()));
}

#[test]
fn failed_agent_transition_response_is_not_reported_as_success() {
    let error = parse_agent_mutation_response(
        reqwest::StatusCode::CONFLICT,
        r#"{"error":"transition rolled back"}"#.into(),
    )
    .unwrap_err();
    assert_eq!(error, "transition rolled back");
}
