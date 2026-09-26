use super::report_refusal::{
    format_report_refusal_text, refusal_diagnostic_request, refusal_next_commands,
    report_refusal_json,
};
use super::*;
use crate::cmd::report_guard::ReportRefusal;

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
fn agent_models_formats_per_model_efforts_and_provider_fallback() {
    let provider = serde_json::json!({
        "id": "codex",
        "default_model": "gpt-6-sol",
        "default_reasoning_effort": "medium",
        "reasoning_efforts": [
            { "id": "low", "display_name": "Low" },
            { "id": "medium", "display_name": "Medium" },
            { "id": "high", "display_name": "High" },
            { "id": "xhigh", "display_name": "Extra high" }
        ],
        "available_models": [
            {
                "id": "gpt-6-sol",
                "display_name": "GPT-6 Sol",
                "reasoning_efforts": [
                    { "id": "low" }, { "id": "medium" }, { "id": "max" }, { "id": "ultra" }
                ]
            },
            {
                "id": "gpt-5.5",
                "display_name": "GPT-5.5",
                "reasoning_efforts": [
                    { "id": "low" }, { "id": "medium" }, { "id": "high" }, { "id": "xhigh" }
                ]
            },
            {
                "id": "gpt-5.6-sol",
                "display_name": "GPT-5.6 Sol",
                "default_reasoning_effort": "low",
                "reasoning_efforts": [
                    { "id": "low" }, { "id": "medium" }, { "id": "high" }, { "id": "xhigh" }
                ]
            },
            { "id": "fallback-model", "display_name": "Fallback Model" }
        ]
    });

    let out = format_agent_models(&provider, "codex");

    assert!(out.contains(
        "GPT-6 Sol (default)\n             Reasoning effort: low, medium (default), max, ultra"
    ));
    assert!(
        out.contains("GPT-5.5\n             Reasoning effort: low, medium (default), high, xhigh")
    );
    assert!(out.contains(
        "GPT-5.6 Sol\n             Reasoning effort: low (default), medium, high, xhigh"
    ));
    assert!(out.contains("Fallback Model\n"));
    assert!(out.contains("Provider fallback reasoning effort:"));
    assert!(out.contains(&format!("{:<10} {}", "xhigh", "Extra high")));
}

#[test]
fn agent_models_cli_preserves_codex_live_default_from_models_cache() {
    use codeterm_plugin::js_runtime::{
        call_json, JsProvider, PluginManifest, PluginVm, SerdeModelInfo,
    };
    use codeterm_plugin::model_catalog;
    use codeterm_plugin::provider::ModelInfo;

    let mut manifest: PluginManifest = serde_json::from_str(include_str!(
        "../../../plugins/provider-codex/plugin.json"
    ))
    .expect("Codex manifest fixture");
    let catalog_provider_id = "test-codex-cache-cli-default";
    manifest.id = catalog_provider_id.into();
    let cache: serde_json::Value = serde_json::from_str(include_str!(
        "../../../plugins/provider-codex/tests/fixtures/models_cache.json"
    ))
    .expect("Codex model cache fixture");
    let cache_json = serde_json::to_string(&cache).expect("cache JSON");
    let host_setup = format!(
        r#"host.homeDir = function () {{ return "/fixture-home"; }};
host.platform = function () {{ return "linux"; }};
host.fs.fileExists = function (path) {{ return path.endsWith("/.codex/models_cache.json"); }};
host.fs.readJson = function (path) {{ return path.endsWith("/.codex/models_cache.json") ? JSON.parse({cache_json}) : null; }};"#
    );
    let bundle = format!(
        "{host_setup}\n{}",
        include_str!("../../../plugins/provider-codex/plugin.js")
    );
    let vm = PluginVm::from_source_with(
        &manifest.id,
        bundle,
        Some(serde_json::to_string(&manifest).expect("manifest JSON")),
    )
    .expect("Codex provider VM");
    let discovered: Option<Vec<SerdeModelInfo>> =
        call_json(&vm, "discoverModels", &[]).expect("Codex live model discovery");
    let discovered = discovered.expect("cache fixture produces live models");
    let models: Vec<ModelInfo> = discovered
        .iter()
        .map(ModelInfo::from)
        .collect();
    assert_eq!(
        models
            .iter()
            .find(|model| model.id == "gpt-5.6-sol")
            .and_then(|model| model.default_reasoning_effort.as_deref()),
        Some("low")
    );

    let provider = JsProvider::new(manifest.clone(), std::sync::Arc::new(vm));
    let catalog_cache_path =
        model_catalog::models_dir().join(format!("{catalog_provider_id}.cache.json"));
    let _ = std::fs::remove_file(&catalog_cache_path);
    model_catalog::put_live(catalog_provider_id, models, manifest.default_model.clone());
    let available_models =
        codeterm_plugin::provider::AgentProvider::available_models(&provider);
    let descriptor = serde_json::json!({
        "default_reasoning_effort": manifest.default_reasoning_effort,
        "available_models": available_models,
    });

    let rendered = format_agent_models(&descriptor, &manifest.id);
    assert!(rendered.contains(
        "GPT-5.6 Sol\n             Reasoning effort: low (default), medium, high, xhigh"
    ));
    let output_json = format_agent_models_json(&descriptor);
    let json_models: serde_json::Value =
        serde_json::from_str(&output_json).expect("CLI model JSON output");
    let json_models = json_models
        .as_array()
        .expect("CLI model JSON array");
    let model = json_models
        .iter()
        .find(|model| model["id"] == "gpt-5.6-sol")
        .expect("live Codex model");
    assert_eq!(model["default_reasoning_effort"], "low");
    let _ = std::fs::remove_file(catalog_cache_path);
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
fn env_default_treats_explicit_empty_identity_as_absent() {
    std::env::remove_var("CODETERM_TEST_EMPTY_REPORT_DEFAULT");
    assert_eq!(
        env_default(Some("   ".into()), "CODETERM_TEST_EMPTY_REPORT_DEFAULT"),
        None
    );
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
fn report_refusal_text_preserves_supplied_diagnostic_data() {
    let refusal = ReportRefusal {
        reason: "readiness report refused: this git worktree has uncommitted lane changes."
            .to_string(),
        dirty_files: vec!["src/main.rs".to_string()],
    };

    let out = format_report_refusal_text(&refusal, true, Some("parent-1"), None);
    assert!(out.contains(&refusal.reason));
    assert!(out.contains(&refusal.dirty_files[0]));
}

#[test]
fn refused_readiness_report_sends_a_diagnostic_not_a_report() {
    let refusal = ReportRefusal {
        reason: "readiness report refused: canonical checkout is dirty".into(),
        dirty_files: vec!["src/main.rs".into()],
    };
    let original = "STATUS: in_review\nCHANGES: finished";
    let (method, body) = refusal_diagnostic_request("worker-tab", &refusal, original);

    assert_eq!(method, "agent.diagnostic");
    assert_eq!(body["tab_id"], "worker-tab");
    assert_eq!(body["kind"], "report_refused");
    let message = body["message"].as_str().unwrap();
    assert!(message.contains(&refusal.message()));
    assert!(message.contains(original));
    for report_field in ["status", "plan_id", "task_id", "report_outcome"] {
        assert!(body.get(report_field).is_none());
    }
}

#[test]
fn refused_report_json_names_the_diagnostic_without_report_outcome() {
    let refusal = ReportRefusal {
        reason: "readiness report refused".into(),
        dirty_files: vec!["src/main.rs".into()],
    };
    let value = report_refusal_json(&refusal, true, Some("parent-tab".into()), None);

    assert!(value.get("verdict").is_some());
    assert_eq!(value["refused_report_diagnostic_sent"], true);
    assert_eq!(value["diagnostic_target_tab_id"], "parent-tab");
    assert!(value.get("diagnostic_error").is_some());
    assert!(value.get("diagnostic_report_sent").is_none());
    assert!(value.get("diagnostic_report_id").is_none());
    assert!(value.get("report_outcome").is_none());
}

#[test]
fn refusal_hint_stages_the_report_body_in_the_pad() {
    let refusal = ReportRefusal {
        reason: "readiness report refused".to_string(),
        dirty_files: Vec::new(),
    };

    let message = refusal.message();

    assert!(message.contains("codeterm pad path"));
    assert!(message.contains("--from-pad report.md"));
    // A heredoc only terminates on a column-0 EOF, so the copied block must stay unindented.
    assert_eq!(message.lines().filter(|line| *line == "EOF").count(), 2);
    let indented_command = message
        .lines()
        .find(|line| line.starts_with(' ') && line.trim_start().starts_with("codeterm agent"));
    assert_eq!(indented_command, None);
}

#[test]
fn refusal_hint_never_suggests_a_shell_expression_sha() {
    let refusal = ReportRefusal {
        reason: "readiness report refused".to_string(),
        dirty_files: Vec::new(),
    };

    for line in refusal_next_commands() {
        assert!(!line.contains("$(git "), "{line}");
    }
    assert!(refusal
        .message()
        .contains("--sha YOUR_OWN_COMMIT_SHA --from-pad"));
    assert!(!refusal.message().contains("--sha $("));
}

#[test]
fn json_next_commands_are_the_same_sequence_the_text_hint_prints() {
    let refusal = ReportRefusal {
        reason: "readiness report refused".to_string(),
        dirty_files: Vec::new(),
    };
    let message = refusal.message();

    let commands = refusal_next_commands();
    assert!(!commands.is_empty());
    for line in &commands {
        assert!(message.contains(*line), "{line}");
    }
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

/// A spawn brief is the widest coordination handoff there is, so the refusal
/// sits in the one resolver every spawn path shares — including the Team-add
/// path, which never had the advisory nudge.
#[test]
fn quoted_handoff_inline_spawn_task_is_refused_and_task_file_is_not() {
    let brief = "You are the Worker.\nRun `./build.ps1 test-one cli quoted_handoff` \
                 and keep $CARGO_TARGET_DIR untouched.\n";
    let err = resolve_spawn_task(Some(brief.to_string()), None, None, &mut &b""[..]).unwrap_err();
    assert!(err.starts_with(crate::cmd::quoted_handoff::REFUSAL_CODE));
    assert!(err.contains("--task-file"));
    assert!(
        !err.contains("--from-pad"),
        "spawn has no --from-pad to offer"
    );

    let mut stdin = brief.as_bytes();
    assert_eq!(
        resolve_spawn_task(None, Some("-".to_string()), None, &mut stdin).unwrap(),
        Some(brief.to_string()),
        "the staged transport delivers the same bytes the shell would have eaten"
    );
}

#[test]
fn quoted_handoff_scalar_spawn_task_still_goes_inline() {
    assert_eq!(
        resolve_spawn_task(Some("run tests".to_string()), None, None, &mut &b""[..]).unwrap(),
        Some("run tests".to_string())
    );
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
    assert_eq!(
        resolve_spawn_task(None, None, None, &mut &b""[..]).unwrap(),
        None
    );
}

#[test]
fn relative_task_file_resolves_from_spawn_cwd() {
    let caller = std::env::temp_dir().join(format!("ct-task-caller-{}", uuid::Uuid::new_v4()));
    let base = caller.join("agent-work");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("brief.txt"), "cwd-relative body\n").unwrap();

    let resolved = super::resolve_spawn_task_from(
        None,
        Some("brief.txt".into()),
        Some("agent-work"),
        &caller,
        &mut &b""[..],
    )
    .unwrap();

    std::fs::remove_dir_all(&caller).ok();
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

#[test]
fn agent_ack_prints_task_and_attempt() {
    let response = serde_json::json!({
        "task_id": "task_demo",
        "attempt_id": "attempt_demo",
        "already_acknowledged": false,
    });

    assert_eq!(
        render_agent_ack(&response).unwrap(),
        "ACKNOWLEDGED — task task_demo, attempt attempt_demo. Continue work."
    );
}

#[test]
fn repeated_agent_ack_prints_already_acknowledged_task_and_attempt() {
    let response = serde_json::json!({
        "task_id": "task_demo",
        "attempt_id": "attempt_demo",
        "already_acknowledged": true,
    });

    assert_eq!(
        render_agent_ack(&response).unwrap(),
        "ALREADY ACKNOWLEDGED — task task_demo, attempt attempt_demo. Continue work."
    );
}

#[test]
fn startup_hold_prints_the_authority_provenance() {
    let response = serde_json::json!({
        "ok": true,
        "held": true,
        "state": "held_by_creator",
        "tab_id": "child",
        "attempt_id": "launch-a",
        "authority": {
            "creator_tab_id": "creator",
            "child_tab_id": "child",
            "attempt_id": "launch-a",
            "team_id": "team_pair",
        },
    });

    assert_eq!(
        render_startup_hold(&response),
        "STARTUP HOLD ACCEPTED — tab child, attempt launch-a, creator creator, team team_pair."
    );
}

#[test]
fn startup_hold_without_a_team_lease_renders_a_placeholder() {
    let response = serde_json::json!({
        "authority": {
            "creator_tab_id": "creator",
            "child_tab_id": "child",
            "attempt_id": "launch-a",
            "team_id": serde_json::Value::Null,
        },
    });

    assert_eq!(
        render_startup_hold(&response),
        "STARTUP HOLD ACCEPTED — tab child, attempt launch-a, creator creator, team -."
    );
}

/// codeterm-releases#55: a daemon-resolved success (202) exits 0, asserted
/// on the `Result` discriminant, never on message wording.
#[test]
fn agent_restart_success_exits_zero() {
    let val = serde_json::json!({
        "session_action": "resume",
        "session_id": "sess-a",
        "full_access": false,
        "binding_restored": true,
    });

    let outcome = restart_response_outcome(reqwest::StatusCode::ACCEPTED, &val);

    assert!(outcome.is_ok(), "a successful restart must not be an Err");
}

/// Guard: a restart the daemon genuinely failed (e.g. a real foreign-session
/// refusal) must still surface as an `Err` so the CLI exits non-zero.
#[test]
fn agent_restart_failure_stays_an_error() {
    let val =
        serde_json::json!({ "error": "refusing to rebind plan/task callers to a foreign session" });

    let outcome = restart_response_outcome(reqwest::StatusCode::INTERNAL_SERVER_ERROR, &val);

    assert!(
        outcome.is_err(),
        "a daemon-reported failure must stay an Err"
    );
}

/// The CLI and the picker read one answer. A probe that could not run must
/// reach both as unknown, and never as the negative that disables the row.
#[test]
fn agent_models_reports_unknown_apart_from_unavailable() {
    let provider = serde_json::json!({
        "id": "claude",
        "default_model": "sonnet",
        "available_models": [
            { "id": "sonnet", "display_name": "Sonnet" },
            { "id": "fable", "display_name": "Fable", "availability": "unknown" },
            { "id": "legacy", "display_name": "Legacy", "available": false },
        ]
    });

    let out = format_agent_models(&provider, "claude");
    let line = |id: &str| {
        out.lines()
            .find(|line| line.starts_with(id))
            .unwrap_or_default()
            .to_string()
    };

    assert!(line("fable").ends_with(AVAILABILITY_UNKNOWN));
    assert!(!line("fable").contains(AVAILABILITY_UNAVAILABLE));
    assert!(line("legacy").ends_with(AVAILABILITY_UNAVAILABLE));
    assert!(!line("sonnet").contains("availability"));
}

/// An explicit verdict from the provider wins over the legacy boolean, so the
/// two fields cannot disagree on a surface.
#[test]
fn agent_models_prefers_the_explicit_availability_verdict() {
    let provider = serde_json::json!({
        "available_models": [
            { "id": "m", "display_name": "M", "available": true, "availability": "unknown" },
        ]
    });
    assert!(format_agent_models(&provider, "claude").contains(AVAILABILITY_UNKNOWN));
}

/// `agent providers` renders the descriptor field itself. Unknown gets its own
/// marker; a row from a peer that never carried the field is unknown too,
/// never the negative that would read as "this machine said no".
#[test]
fn agent_providers_renders_the_three_valued_descriptor_field() {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../fixtures/provider_availability.json")).unwrap();
    let marker = |id: &str| {
        provider_availability_marker(
            rows.iter()
                .find(|r| r["id"] == id)
                .expect("fixture row present"),
        )
    };

    assert_eq!(marker("fixture-available"), "");
    assert_eq!(marker("fixture-unavailable"), AVAILABILITY_UNAVAILABLE);
    assert_eq!(marker("fixture-unknown"), AVAILABILITY_UNKNOWN);
    assert_eq!(marker("fixture-legacy"), AVAILABILITY_UNKNOWN);
}
