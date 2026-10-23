use super::*;

#[test]
fn blueprint_memory_requests_offer_without_applying_and_preserve_identity() {
    for action in [
        BlueprintAction::New {
            kind: "watch".into(),
            like: Some("watch CI failure".into()),
        },
        BlueprintAction::Apply {
            source: String::new(),
            like: Some("watch CI failure".into()),
            set: vec![],
            note: None,
            dry_run: false,
        },
    ] {
        let (method, params) =
            request(action, Some("caller".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "blueprint.recall");
        assert_eq!(params["intent"], "watch CI failure");
        assert_eq!(params["caller_tab_id"], "caller");
        assert!(params.get("definition").is_none());
    }
    assert_eq!(
        request(BlueprintAction::Recipe { name: None }, None, None)
            .unwrap()
            .0,
        "blueprint.recall"
    );
    let (method, params) = request(
        BlueprintAction::Keep {
            id: "graph".into(),
            when: vec![
                "Watch CI failure".into(),
                "tell me when the build goes red".into(),
                "ping me about failing checks".into(),
            ],
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(method, "blueprint.keep");
    assert_eq!(params["id"], "graph");
    assert_eq!(
        params["when"],
        json!([
            "Watch CI failure",
            "tell me when the build goes red",
            "ping me about failing checks"
        ])
    );
    let six = (0..6).map(|index| format!("intent {index}")).collect();
    assert!(request(
        BlueprintAction::Keep {
            id: "graph".into(),
            when: six,
        },
        None,
        None,
    )
    .is_err());
    let (method, params) = request(
        BlueprintAction::Apply {
            source: "memory:skill_saved".into(),
            like: None,
            set: vec!["repo=owner/repo".into()],
            note: None,
            dry_run: true,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(method, "blueprint.apply_recipe");
    assert_eq!(params["id"], "skill_saved");
    assert_eq!(params["dry_run"], true);
    let offers = memory::offers(json!([]), "HTTP endpoint response code");
    assert!(offers
        .as_array()
        .unwrap()
        .iter()
        .any(|offer| offer["id"] == "http-health"));
    let yaml = authoring::scaffold("watch").unwrap();
    let stored = json!([{"id":"skill_vector","yaml":yaml,"intents":["Detect stalled deployments"],"uses":0,"successes":1,"why":"intent 0.81"}]);
    let offers = memory::offers(stored, "Identify frozen releases HTTP endpoint");
    assert_eq!(offers[0]["id"], "memory:skill_vector");
    assert_eq!(offers[0]["why"], "intent 0.81");
    assert_eq!(offers[0]["intents"], json!(["Detect stalled deployments"]));
    assert_eq!(
        memory::offers(json!([]), "unmatched opaque zebra"),
        json!([])
    );
}

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
fn blueprint_update_refuses_unversioned_definitions_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.json");
    for old in [
        json!({"buckets":["done"],"handlers":{}}),
        json!({"current_version":1,"versions":[]}),
        json!({"schema_version":1,"watch":{}}),
    ] {
        std::fs::write(&path, old.to_string()).unwrap();
        assert!(request(
            BlueprintAction::Update {
                note: None,
                id: "graph".into(),
                expect_version: 1,
                from_file: path.clone(),
                cause: None,
                dry_run: false
            },
            None,
            None
        )
        .is_err());
    }
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
    let definition = authoring::load_checked("echo", &["message=opaque".into()]).unwrap();
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
    assert_eq!(params["definition"], definition);
    assert_eq!(params["cause"], "transition_2");

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
    assert_eq!(method, "blueprint.update");

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
        assert_eq!(definition["steps"][0]["with"]["id"]["lit"], 42);
        assert_eq!(params["caller_tab_id"], "caller");
        assert_eq!(params["session_binding_nonce"], "nonce");
    }
}

#[test]
fn blueprint_apply_note_and_dry_run_are_explicit() {
    let (_, params) = request(
        BlueprintAction::Apply {
            source: "http-health".into(),
            like: None,
            set: vec!["url=https://example.com".into()],
            note: Some("Health check".into()),
            dry_run: true,
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(params["dry_run"], true);
    assert_eq!(params["definition"]["note"], "Health check");
}
#[test]
fn human_apply_diff_uses_authoring_paths_and_hides_wire_wrappers() {
    let before =
        authoring::load_checked("github", &["repo=owner/repo".into(), "id=1".into()]).unwrap();
    let mut after = before.clone();
    after["steps"][0]["with"]["id"]["lit"] = json!(2);
    let output = apply_output::summary(
        &json!({"action":"update","name":"CI watch","id":"blueprint_a","state":"running","before":before,"after":after}),
    );
    assert!(output.starts_with("update CI watch blueprint_a"));
    assert!(output.contains("steps[0].with.id.lit: 1 -> 2"));
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
    fn native_v2_update_preserves_explicit_data_inputs() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("update.yaml");
        let definition = authoring::load_checked("echo", &["message=opaque input".into()]).unwrap();
        std::fs::write(&path, serde_yaml::to_string(&definition).unwrap()).unwrap();
        let (method, params) = request(
            BlueprintAction::Update {
                note: None,
                id: "graph".into(),
                expect_version: 2,
                from_file: path,
                cause: Some("transition_1".into()),
                dry_run: true,
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(method, "blueprint.update");
        assert_eq!(params["definition"], definition);
        assert_eq!(params["cause"], "transition_1");
        assert_eq!(params["dry_run"], true);
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
    fn blueprint_history_json_is_not_an_authoring_document() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history.json");
        std::fs::write(
            &path,
            json!({"current_version":2,"versions":[]}).to_string(),
        )
        .unwrap();
        assert!(request(
            BlueprintAction::Update {
                note: None,
                id: "graph".into(),
                expect_version: 2,
                from_file: path,
                cause: None,
                dry_run: false
            },
            None,
            None
        )
        .is_err());
    }
}

#[test]
fn blueprint_save_receipt_surfaces_every_evicted_id() {
    let result =
        json!({"id":"saved-new", "status":"saved", "evicted_ids":["old-first", "old-second"]});
    let text = memory::save_receipt(&result);
    for id in ["saved-new", "old-first", "old-second"] {
        assert!(text.contains(id), "missing {id} in {text}");
    }
    assert!(text.contains("Archived"));
}

#[test]
fn blueprint_like_preserves_hybrid_recall_without_shared_words() {
    let query = "Transmit structured output downstream";
    let stored = json!([
        {"id":"skill_pipeline", "when_to_use":"Chain shell commands", "yaml":"opaque recipe", "why":"intent 0.92", "enrichment_pending":true},
        {"id":"skill_second", "when_to_use":"Parse JSON", "yaml":"another recipe", "why":"intent 0.81", "intents":["Parse JSON"]}
    ]);
    assert!(stored[0]["when_to_use"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .all(|word| !query.to_lowercase().contains(&word.to_lowercase())));
    let offers = memory::offers(stored.clone(), query);
    let offers = offers.as_array().unwrap();
    for (index, id) in ["skill_pipeline", "skill_second"].iter().enumerate() {
        let mut expected = stored[index].clone();
        expected["id"] = json!(format!("memory:{id}"));
        assert_eq!(offers[index], expected);
    }
    for offer in &offers[2..] {
        assert!(recipe_files::FILES.iter().any(|(id, _)| offer["id"] == *id));
    }
    let ids = offers
        .iter()
        .map(|offer| offer["id"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), offers.len());
}

#[test]
fn blueprint_like_appends_builtins_after_daemon_results_within_five_offers() {
    for count in [4, 5, 6] {
        let stored = (0..count)
            .map(|index| {
                json!({
                    "id":format!("saved-{index}"),
                    "yaml":format!("opaque recipe {index}"),
                    "intents":["A semantic match with no shared query words"],
                    "why":"intent 0.92",
                    "enrichment_pending":true
                })
            })
            .collect::<Vec<_>>();
        let offers = memory::offers(json!(stored), "HTTP endpoint response code");
        let offers = offers.as_array().unwrap();
        assert_eq!(offers.len(), 5);
        for (index, original) in stored.iter().take(5).enumerate() {
            let mut expected = original.clone();
            expected["id"] = json!(format!("memory:saved-{index}"));
            assert_eq!(offers[index], expected);
        }
        if count == 4 {
            assert_eq!(offers[4]["id"], "http-health");
        }
    }
}
