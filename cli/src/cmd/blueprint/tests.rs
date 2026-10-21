use super::*;

#[test]
fn blueprint_text_input_wire_preserves_file_body_and_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input.txt");
    let body = "'quotes' $() `ticks`\n\u{03bb}";
    std::fs::write(&path, body).unwrap();
    let (method, params) = request(
        BlueprintAction::Input {
            id: "graph".into(),
            from_pad: None,
            text_file: Some(path),
        },
        Some("caller".into()),
        Some("nonce".into()),
    )
    .unwrap();
    assert_eq!(method, "blueprint.input");
    assert_eq!(
        params,
        json!({"id":"graph","text":body,"caller_tab_id":"caller","session_binding_nonce":"nonce"})
    );
}

#[test]
fn update_handler_definitions_generate_ids_for_buckets_and_other() {
    let mut definition = json!({
        "buckets": ["passed"],
        "handlers": {"passed": [{"type":"notify", "parameters":{}}]},
        "on_other": [{"type":"escalate", "parameters":{"target":"creator"}}]
    });

    normalize_update_handlers(definition.as_object_mut().unwrap()).unwrap();

    assert_eq!(
        definition["handlers"]["passed"][0]["id"],
        "notify-on-passed"
    );
    assert_eq!(
        definition["on_other"][0]["id"],
        "escalate-to-owner-on-other"
    );
}

#[test]
fn sentinel_escalation_resolve_message_prints_requested_transition_id() {
    assert_eq!(
        escalation_resolved_message(Some("transition_9"), &json!({"id":"sentinel_a"})),
        "Resolved escalation transition_9 for sentinel_a."
    );
}

#[test]
fn sentinel_escalate_resolve_request_preserves_stage_and_transition_ids() {
    let action = BlueprintAction::Escalation {
        action: BlueprintEscalationAction::Resolve {
            id: "sentinel_a".into(),
            transition_id: "transition_9".into(),
        },
    };

    let (method, params) = request(action, Some("creator".into()), Some("nonce".into())).unwrap();

    assert_eq!(method, "blueprint.escalation.resolve");
    assert_eq!(params["id"], "sentinel_a");
    assert_eq!(params["transition_id"], "transition_9");
}

#[test]
fn sentinel_update_cli_json_round_trip_preserves_definition_and_history_request() {
    use clap::Parser;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("definition.json");
    let definition = json!({
        "name": "Updated graph",
        "question": "Did it finish?",
        "buckets": ["done"],
        "every": "30s",
        "routes": {"done": "notify"},
        "matches": {"done": "^done$"},
        "until": "done",
        "on_other": [],
        "max_transitions": 8,
        "max_actions": 12,
        "note": "new completion condition"
    });
    std::fs::write(&path, serde_json::to_vec(&definition).unwrap()).unwrap();
    let cli = crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "update",
        "sentinel_a",
        "--expect-version",
        "3",
        "--from-file",
        path.to_str().unwrap(),
        "--cause",
        "transition_2",
    ])
    .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("unexpected command")
    };
    let (method, params) = request(action, Some("editor".into()), Some("nonce".into())).unwrap();
    assert_eq!(method, "blueprint.update");
    assert_eq!(params["caller_tab_id"], "editor");
    assert_eq!(params["session_binding_nonce"], "nonce");
    assert_eq!(params["expect_version"], 3);
    let mut expected = definition;
    expected["cause"] = json!("transition_2");
    assert_eq!(params["update"], expected);

    let cli = crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "update",
        "sentinel_a",
        "--expect-version",
        "3",
        "--from-file",
        path.to_str().unwrap(),
        "--dry-run",
    ])
    .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("unexpected command")
    };
    let (method, _) = request(action, Some("editor".into()), Some("nonce".into())).unwrap();
    assert_eq!(method, "blueprint.validate.update");

    let cli =
        crate::args::cli::Cli::try_parse_from(["codeterm", "blueprint", "history", "sentinel_a"])
            .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("unexpected command")
    };
    let (method, params) = request(action, Some("reader".into()), None).unwrap();
    assert_eq!(method, "blueprint.history");
    assert_eq!(params["id"], "sentinel_a");
}
fn parse_delete(args: &[&str]) -> Result<BlueprintAction, clap::Error> {
    use clap::Parser;
    let argv = ["codeterm", "blueprint", "delete"]
        .into_iter()
        .chain(args.iter().copied());
    let crate::args::cli::Commands::Blueprint { action } =
        crate::args::cli::Cli::try_parse_from(argv)?.command
    else {
        panic!("wrong command")
    };
    Ok(action)
}

#[test]
fn blueprint_delete_wire_carries_id_stop_and_caller() {
    let (method, params) = request(
        parse_delete(&["graph", "--stop"]).unwrap(),
        Some("caller".into()),
        Some("nonce".into()),
    )
    .unwrap();
    assert_eq!(method, "blueprint.delete");
    assert_eq!(
        params,
        json!({"id":"graph","stopped":false,"stop":true,"caller_tab_id":"caller","session_binding_nonce":"nonce"})
    );
    let (_, params) = request(
        parse_delete(&["--stopped"]).unwrap(),
        Some("caller".into()),
        None,
    )
    .unwrap();
    assert_eq!(params["id"], Value::Null);
    assert_eq!(params["stopped"], true);
}

#[test]
fn blueprint_delete_refuses_ambiguous_or_empty_targets() {
    assert!(parse_delete(&[]).is_err(), "a target is required");
    assert!(
        parse_delete(&["graph", "--stopped"]).is_err(),
        "id and --stopped conflict"
    );
    assert!(
        parse_delete(&["--stopped", "--stop"]).is_err(),
        "--stop needs one id"
    );
}

#[test]
fn blueprint_delete_message_lists_removed_roots() {
    assert_eq!(
        deleted_message(&json!({"deleted":["a","b"]})),
        "Deleted a, b."
    );
    assert_eq!(
        deleted_message(&json!({"deleted":[]})),
        "No blueprints deleted."
    );
}

#[test]
fn render_log_preserves_step_names_and_signed_indices() {
    let rows = json!([
        {"started_at_ms":100,"step_index":-1,"step_name":"$run","state":"done","outcome":"done"},
        {"started_at_ms":101,"step_index":-1,"step_name":"$result","state":"done","outcome":"handler_result"},
        {"started_at_ms":102,"step_index":0,"step_name":"emit","state":"done","outcome":"done"}
    ]);
    assert_eq!(log_text(&rows), "100 -1 $run done outcome=done result=- reason=-\n101 -1 $result done outcome=handler_result result=- reason=-\n102 0 emit done outcome=done result=- reason=-");
}

#[test]
fn blueprint_watch_summary_reports_progress_and_failed_probe_state() {
    let job = serde_json::json!({
        "id":"watch", "machine":{"stopped":false,"paused":false,"last_bucket":"waiting"},
        "probe_state":"error", "runs_since_transition":3, "last_probe_at":61000,
        "interval_ms":60000, "last_reason":"probe_timeout"
    });
    assert_eq!(
        super::watch_summary(&job),
        "watch [watching] bucket=error ×3 classification=unmatched every=60000ms last_probe_at=61000 reason=probe_timeout"
    );
}

#[test]
fn blueprint_watch_summary_exposes_explicit_classification_mode() {
    let mut job = serde_json::json!({"id":"watch", "interval_ms":60000, "model_classification":true, "matches":{}});
    assert!(super::watch_summary(&job).contains("classification=model"));
    job["matches"] = serde_json::json!({"ready":"^ready$"});
    assert!(super::watch_summary(&job).contains("classification=pattern"));
}

#[test]
fn blueprint_apply_and_import_use_the_same_checked_document() {
    use clap::Parser;
    for (verb, method) in [
        ("apply", "blueprint.apply"),
        ("create", "blueprint.apply"),
        ("import", "blueprint.import"),
    ] {
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "blueprint",
            verb,
            "github",
            "--set",
            "repo=owner/repo",
            "--set",
            "id=42",
        ])
        .unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command")
        };
        let (actual, params) =
            request(action, Some("caller".into()), Some("nonce".into())).unwrap();
        assert_eq!(actual, method);
        let definition = if verb == "import" {
            assert_eq!(params["stopped"], true);
            &params["recipe"]["definition"]
        } else {
            &params["definition"]
        };
        assert_eq!(definition["spec"]["steps"][0]["kind"]["probe"]["id"], 42);
        assert_eq!(params["caller_tab_id"], "caller");
        assert_eq!(params["session_binding_nonce"], "nonce");
    }
}

#[test]
fn blueprint_apply_note_and_dry_run_are_explicit() {
    let (_, params) = request(
        BlueprintAction::Apply {
            source: "http-health".into(),
            set: vec!["url=https://example.com".into()],
            note: Some("Health check".into()),
            dry_run: true,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(params["dry_run"], true);
    assert_eq!(params["definition"]["spec"]["note"], "Health check");
}
#[test]
fn human_apply_diff_uses_authoring_paths_and_hides_wire_wrappers() {
    let before = json!({"spec":{"name":"CI watch","owner":"team","trigger":{"type":"schedule","every":"20s"},"steps":[{"name":"ci","input":"text","kind":{"kind":"github","probe":{"repo":"owner/repo","resource":"run","id":1}},"on":{"done":"stop"}}]}});
    let mut after = before.clone();
    after["spec"]["steps"][0]["kind"]["probe"]["id"] = json!(2);
    let output = apply_output::summary(
        &json!({"action":"update","name":"CI watch","id":"blueprint_a","state":"running","before":before,"after":after}),
    );
    assert!(output.starts_with("update CI watch blueprint_a"));
    assert!(output.contains("pipeline.steps[0].with.id: 1 -> 2"));
    assert!(!output.contains("kind.probe"));
    assert!(!output.contains("spec."));
}

mod command_tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn blueprint_pipeline_nodes_and_log_select_their_scoped_methods() {
        let cli =
            crate::args::cli::Cli::try_parse_from(["codeterm", "blueprint", "nodes"]).unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command")
        };
        assert_eq!(
            request(action, Some("creator".into()), None).unwrap().0,
            "blueprint.pipeline.nodes"
        );

        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "log",
            "sentinel_echo",
            "--limit",
            "200",
        ])
        .unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command")
        };
        let (method, params) = request(action, Some("creator".into()), None).unwrap();
        assert_eq!(method, "blueprint.log");
        assert_eq!(params["limit"], 200);
    }

    #[test]
    fn sentinel_update_input_field_passes_through_read_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("definition.json");
        let definition = json!({
            "question": "transcribed?",
            "buckets": ["done"],
            "input": {"data": {"file": "/tmp/clip.wav"}, "convert_to": "text"}
        });
        std::fs::write(&path, serde_json::to_vec(&definition).unwrap()).unwrap();
        let update = read_update(&path, Some("transition_1")).unwrap();
        assert_eq!(
            update["input"],
            json!({"data": {"file": "/tmp/clip.wav"}, "convert_to": "text"})
        );
        assert_eq!(update["cause"], "transition_1");
    }

    #[test]
    fn sentinel_input_inline_text_renders_without_leaking_content() {
        let value = json!({"input": {"kind": "text", "data": {"text": "secret content"}, "convert_to": null}});
        let line = input_source_line(&value).unwrap();
        assert!(!line.contains("secret content"));
        assert!(line.contains("text"));
    }

    #[test]
    fn blueprint_pipeline_show_redacts_input_and_prints_conversion() {
        let value = json!({
            "pipeline": {"trigger": {
                "type":"input",
                "kind":"audio",
                "source":{"text":"secret content"},
                "convert_to":"text"
            }}
        });
        let line = pipeline_input_source_line(&value).unwrap();
        assert!(line.contains("audio"));
        assert!(line.contains("text"));
        assert!(!line.contains("secret content"));
        assert_eq!(
            input_ref_summary(&json!({"url":"https://user:secret@example.com/path?token=secret"})),
            "example.com"
        );
    }

    #[test]
    fn blueprint_history_json_round_trips_into_pipeline_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let spec = json!({
            "name":"echo",
            "trigger":{"type":"schedule","every":"20s"},
            "steps":[{"name":"emit","input":"text","kind":{"kind":"command","command":"printf ready"}}],
            "result":{"to":"owner"}
        });
        let history = json!({
            "sentinel_id":"sentinel_echo",
            "current_version":2,
            "versions":[{"version":2,"definition":{"pipeline":spec}}]
        });
        std::fs::write(&path, serde_json::to_vec(&history).unwrap()).unwrap();
        let (method, params) = request(
            BlueprintAction::Update {
                note: None,
                id: "sentinel_echo".into(),
                expect_version: 2,
                from_file: path,
                cause: None,
                dry_run: false,
            },
            Some("creator".into()),
            None,
        )
        .unwrap();
        assert_eq!(method, "blueprint.pipeline.update");
        assert_eq!(params["spec"], spec);
    }
}
