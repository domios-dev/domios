use super::*;

#[test]
fn blueprint_export_import_cli_parse_and_wire() {
    use clap::Parser;
    let cli = crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "export",
        "watch",
        "--out",
        "watch.blueprint.json",
    ])
    .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("Blueprint action required")
    };
    assert!(matches!(
        &action,
        BlueprintAction::Export { out: Some(_), .. }
    ));
    let (method, params) = request(action, Some("caller".into()), Some("nonce".into())).unwrap();
    assert_eq!(method, "blueprint.export");
    assert_eq!(params["id"], "watch");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("watch.blueprint.json");
    std::fs::write(&path, serde_json::to_vec(&json!({"schema_version":1, "placeholders":{"target":"Destination tab"}, "definition":{"create":{"observe":{"kind":"tab","id":"{{target}}"}}}})).unwrap()).unwrap();
    for flags in [vec![], vec!["--stopped"], vec!["--start"]] {
        let mut args = vec![
            "codeterm",
            "blueprint",
            "import",
            path.to_str().unwrap(),
            "--set",
            "target=scratch-tab",
        ];
        args.extend(&flags);
        let cli = crate::args::cli::Cli::try_parse_from(args).unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("Blueprint action required")
        };
        let (method, params) =
            request(action, Some("caller".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "blueprint.import");
        assert_eq!(params["stopped"], !flags.contains(&"--start"));
        assert_eq!(params["set"], json!(["target=scratch-tab"]));
        assert_eq!(params["caller_tab_id"], "caller");
    }
    assert!(crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "import",
        path.to_str().unwrap(),
        "--stopped",
        "--start"
    ])
    .is_err());
    let action = BlueprintAction::Import {
        file: path,
        set: vec![],
        stopped: true,
        start: false,
    };
    assert!(request(action, None, None).unwrap_err().contains("target"));
}

#[test]
fn blueprint_exported_recipe_uses_create_recipe_substitution_and_refuses_mixed_options() {
    use clap::Parser;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("watch.blueprint.json");
    std::fs::write(&path, serde_json::to_vec(&json!({"schema_version":1, "placeholders":{"command":"Probe command"}, "definition":{"create":{"probe":"{{command}}"}}})).unwrap()).unwrap();
    let args = [
        "codeterm",
        "blueprint",
        "create",
        "--recipe",
        path.to_str().unwrap(),
        "--set",
        "command=printf ready",
    ];
    let cli = crate::args::cli::Cli::try_parse_from(args).unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("Blueprint action required")
    };
    let (method, params) = request(action, None, None).unwrap();
    assert_eq!(method, "blueprint.import");
    assert_eq!(params["stopped"], true);
    let cli = crate::args::cli::Cli::try_parse_from(args.into_iter().chain(["--dry-run"])).unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("Blueprint action required")
    };
    let (method, params) = request(action, None, None).unwrap();
    assert_eq!(method, "blueprint.validate.create");
    assert_eq!(params["create"]["probe"], "printf ready");
    let cli =
        crate::args::cli::Cli::try_parse_from(args.into_iter().chain(["--name", "Ignored name"]))
            .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("Blueprint action required")
    };
    assert!(request(action, None, None).is_err());
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
fn blueprint_terminal_session_cli_wire_carries_options() {
    use clap::Parser;
    let cli = crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "terminal-adapter",
        "--observe",
        "tab:shell",
        "--terminal-pattern",
        "ready",
        "--output-settled",
        "200",
        "--session",
        "--question",
        "reply?",
        "--buckets",
        "reply",
        "--match",
        "reply=.*",
        "--route",
        "reply=send:shell",
    ])
    .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("wrong command")
    };
    let (_, params) = request(action, Some("caller".into()), None).unwrap();
    assert_eq!(
        params["create"]["terminal"],
        json!({"pattern":"ready","idle_ms":200})
    );
    assert_eq!(params["create"]["session"], true);
    assert!(
        crate::args::cli::Cli::try_parse_from(["codeterm", "blueprint", "input", "id"]).is_err()
    );
    assert!(crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "input",
        "id",
        "--from-pad",
        "a",
        "--text-file",
        "b"
    ])
    .is_err());
}

#[test]
fn sentinel_repeated_route_wire_preserves_order_and_stable_ids() {
    let (routes, handlers) = handlers::declared_outputs(
        vec!["done=notify".into(), "done=stop".into()],
        None,
        &["done".into()],
    )
    .unwrap();
    assert!(routes.is_empty());
    assert_eq!(
        handlers.unwrap(),
        json!({"done":[
            {"id":"notify-on-done","type":"notify","parameters":{}},
            {"id":"stop-on-done","type":"stop","parameters":{}}
        ]})
    );
}

#[test]
fn sentinel_none_route_is_an_empty_handler_array_and_can_mix_with_routes() {
    let quiet = ["quiet".into()];
    let (routes, handlers) =
        handlers::declared_outputs(vec!["quiet=none".into()], None, &quiet).unwrap();
    assert_eq!(routes, BTreeMap::from([("quiet".into(), "none".into())]));
    assert!(handlers.is_none());

    let buckets = ["quiet".into(), "done".into()];
    let (routes, handlers) = handlers::declared_outputs(
        vec![
            "quiet=none".into(),
            "done=notify".into(),
            "done=stop".into(),
        ],
        None,
        &buckets,
    )
    .unwrap();
    assert!(routes.is_empty());
    let handlers = handlers.unwrap();
    assert_eq!(handlers["quiet"], json!([]));
    assert_eq!(handlers["done"].as_array().unwrap().len(), 2);

    assert!(handlers::declared_outputs(
        vec!["done=none".into(), "done=notify".into()],
        None,
        &["done".into()]
    )
    .is_err());
}

#[test]
fn sentinel_handlers_file_refuses_mixed_duplicate_unknown_and_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("handlers.json");
    std::fs::write(
        &path,
        r#"{"done":[{"id":"h","type":"notify","parameters":{}}]}"#,
    )
    .unwrap();
    assert!(handlers::declared_outputs(Vec::new(), Some(&path), &["done".into()]).is_ok());
    assert!(
        handlers::declared_outputs(vec!["done=stop".into()], Some(&path), &["done".into()])
            .is_err()
    );
    for raw in [
        r#"{"done":[{"id":"h","type":"notify","parameters":{}},{"id":"h","type":"stop","parameters":{}}]}"#,
        r#"{"done":[{"id":"h","type":"notify","parameters":{},"extra":true}]}"#,
        r#"{"done":[{"id":"h","type":"notify","parameters":{"extra":true}}]}"#,
        r#"{"done":[{"id":"h","type":"command","parameters":{}}]}"#,
        r#"{"done":[{"id":"h","type":"unknown","parameters":{}}]}"#,
    ] {
        std::fs::write(&path, raw).unwrap();
        assert!(handlers::declared_outputs(Vec::new(), Some(&path), &["done".into()]).is_err());
    }
}

#[test]
fn sentinel_handlers_file_accepts_respond_prompt_kind_parameter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("handlers.json");
    std::fs::write(
            &path,
            r#"{"prompt":[{"id":"answer","type":"respond_prompt","parameters":{"target":"permission"}}]}"#,
        )
        .unwrap();

    let (_, handlers) =
        handlers::declared_outputs(Vec::new(), Some(&path), &["prompt".into()]).unwrap();

    assert_eq!(
        handlers.unwrap()["prompt"][0],
        json!({"id":"answer","type":"respond_prompt","parameters":{"target":"permission"}})
    );
    for parameters in ["{}", r#"{"target":"  "}"#, r#"{"target":"permission\n"}"#] {
        std::fs::write(
                &path,
                format!(r#"{{"prompt":[{{"id":"answer","type":"respond_prompt","parameters":{parameters}}}]}}"#),
            )
            .unwrap();
        assert!(handlers::declared_outputs(Vec::new(), Some(&path), &["prompt".into()]).is_err());
    }
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
fn sentinel_escalate_handlers_file_accepts_reserved_other_handler() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("handlers.json");
    std::fs::write(
        &path,
        r#"{"other":[{"id":"ask_creator","type":"escalate","parameters":{"target":"creator"}}]}"#,
    )
    .unwrap();

    let (_, handlers) = handlers::declared_outputs(Vec::new(), Some(&path), &[]).unwrap();

    assert_eq!(
        handlers.unwrap()["other"][0],
        json!({"id":"ask_creator","type":"escalate","parameters":{"target":"creator"}})
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
fn create_dry_run_uses_validation_rpc() {
    use clap::Parser;

    let cli = crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "ci-green-watch",
        "--probe",
        "status",
        "--question",
        "classify-result",
        "--buckets",
        "passed",
        "--match",
        "passed=.*",
        "--dry-run",
    ])
    .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("unexpected command")
    };

    let (method, params) = request(action, Some("creator".into()), Some("nonce".into())).unwrap();

    assert_eq!(method, "blueprint.validate.create");
    assert_eq!(params["create"]["name"], "ci-green-watch");
}
#[test]
fn sentinel_create_rpc_request_preserves_declared_handlers_and_tab_nonce() {
    use clap::Parser;
    let cli = crate::args::cli::Cli::try_parse_from([
        "codeterm",
        "blueprint",
        "create",
        "--name",
        "Supplied name",
        "--probe",
        "status",
        "--question",
        "state?",
        "--buckets",
        "done",
        "--route",
        "done=notify",
        "--route",
        "done=blueprint:sentinel_child",
        "--rearm-on-change",
        "done",
        "--paused",
        "--timeout",
        "1h",
        "--max-transitions",
        "7",
        "--max-actions",
        "9",
    ])
    .unwrap();
    let crate::args::cli::Commands::Blueprint { action } = cli.command else {
        panic!("unexpected command")
    };
    let (method, params) = request(
        action,
        Some("supplied-creator".into()),
        Some("supplied-nonce".into()),
    )
    .unwrap();
    assert_eq!(method, "blueprint.create");
    assert_eq!(params["caller_tab_id"], "supplied-creator");
    assert_eq!(params["session_binding_nonce"], "supplied-nonce");
    let create = &params["create"];
    assert_eq!(create["name"], "Supplied name");
    assert_eq!(create["paused"], true);
    assert_eq!(create["max_transitions"], 7);
    assert_eq!(create["max_actions"], 9);
    assert_eq!(create["routes"], json!({}));
    assert_eq!(create["rearm_on_change"], json!(["done"]));
    assert_eq!(
        create["handlers"]["done"][1],
        json!({"id":"activate-sentinel-child-on-done","type":"blueprint","parameters":{"target":"sentinel_child"}})
    );
    assert_eq!(create["timeout"], "1h");
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
#[cfg(test)]
mod command_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn blueprint_pipeline_spec_and_recipe_reduce_to_one_create_request() {
        let directory = tempfile::tempdir().unwrap();
        let spec_path = directory.path().join("pipeline.yaml");
        std::fs::write(
            &spec_path,
            "name: Echo pipeline\ntrigger:\n  type: schedule\n  every: 20s\nsteps:\n  - name: emit\n    input: text\n    kind:\n      kind: command\n      command: printf ready\n",
        )
        .unwrap();
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--spec",
            spec_path.to_str().unwrap(),
            "--owner",
            "team",
            "--dry-run",
        ])
        .unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command")
        };
        let (method, params) = request(action, Some("creator".into()), None).unwrap();
        assert_eq!(method, "blueprint.pipeline.validate");
        let spec: codeterm_types::blueprint_pipeline::BlueprintSpec =
            serde_json::from_value(params["spec"].clone()).unwrap();
        assert_eq!(
            spec.owner,
            codeterm_types::blueprint_pipeline::BlueprintOwner::Team
        );

        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--recipe",
            "echo",
            "--set",
            "message=ready",
            "--dry-run",
        ])
        .unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command")
        };
        let (method, params) = request(action, Some("creator".into()), None).unwrap();
        assert_eq!(method, "blueprint.pipeline.validate");
        assert_eq!(
            params["spec"]["steps"][0]["kind"]["command"],
            "printf 'ready'"
        );
    }

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
    fn sentinel_observe_request_roundtrips_tab_and_cron_run_bindings() {
        for (source, expected) in [
            ("tab:bound-agent", json!({"kind":"tab", "id":"bound-agent"})),
            (
                "cron-run:nightly",
                json!({"kind":"cron_run", "id":"nightly"}),
            ),
        ] {
            let cli = crate::args::cli::Cli::try_parse_from([
                "codeterm",
                "blueprint",
                "create",
                "--name",
                "agent-state-watch",
                "--observe",
                source,
                "--question",
                "tab state?",
                "--buckets",
                "waiting",
                "--match",
                "waiting=.*",
                "--json",
            ])
            .unwrap();
            let crate::args::cli::Commands::Blueprint { action } = cli.command else {
                panic!("wrong command")
            };
            let (method, params) =
                request(action, Some("creator".into()), Some("nonce".into())).unwrap();
            assert_eq!(method, "blueprint.create");
            assert_eq!(params["create"]["probe"], Value::Null);
            assert_eq!(params["create"]["observe"], expected);
            let encoded = serde_json::to_string(&params["create"]).unwrap();
            let decoded: Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, params["create"]);
        }
    }

    #[test]
    fn sentinel_command_handlers_file_reaches_authenticated_create_request_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handlers.json");
        let command = "printf '$model ${output} `raw`'";
        let definitions = json!({"done":[{"id":"fixed", "type":"command", "parameters":{"command":command, "label":"Fixed text", "timeout_ms":1000}}]});
        std::fs::write(&path, serde_json::to_vec(&definitions).unwrap()).unwrap();
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--name",
            "command-result-watch",
            "--probe",
            "status",
            "--question",
            "state?",
            "--buckets",
            "done",
            "--match",
            "done=done",
            "--timeout",
            "1h",
            "--handlers-file",
            path.to_str().unwrap(),
        ])
        .unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command");
        };
        let (method, params) =
            request(action, Some("creator".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "blueprint.create");
        assert_eq!(params["caller_tab_id"], "creator");
        assert_eq!(params["session_binding_nonce"], "nonce");
        assert_eq!(params["create"]["handlers"], definitions);
        assert_eq!(params["create"]["routes"], json!({}));
    }

    #[test]
    fn sentinel_create_input_request_carries_spec_and_resolves_relative_file() {
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--name",
            "voice-note-watch",
            "--input",
            "audio",
            "--input-file",
            "clips/voice.wav",
            "--convert-to",
            "text",
            "--convert-timeout",
            "15m",
            "--question",
            "transcribed?",
            "--buckets",
            "done",
        ])
        .unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command")
        };
        let (method, params) =
            request(action, Some("creator".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "blueprint.create");
        let input = &params["create"]["input"];
        assert_eq!(input["kind"], "audio");
        let file = input["data"]["file"].as_str().unwrap();
        assert!(
            std::path::Path::new(file).is_absolute(),
            "expected absolute path, got {file}"
        );
        assert!(file.ends_with("clips\\voice.wav") || file.ends_with("clips/voice.wav"));
        assert_eq!(input["convert_to"], "text");
        assert_eq!(input["convert_timeout_ms"], 900_000);
        assert_eq!(params["create"]["probe"], Value::Null);
        assert_eq!(params["create"]["observe"], Value::Null);
    }

    #[test]
    fn sentinel_create_input_dry_run_uses_validation_rpc() {
        let cli = crate::args::cli::Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--name",
            "voice-note-watch",
            "--input",
            "audio",
            "--input-url",
            "https://example.com/clip.wav",
            "--question",
            "transcribed?",
            "--buckets",
            "done",
            "--dry-run",
        ])
        .unwrap();
        let crate::args::cli::Commands::Blueprint { action } = cli.command else {
            panic!("wrong command")
        };
        let (method, params) =
            request(action, Some("creator".into()), Some("nonce".into())).unwrap();
        assert_eq!(method, "blueprint.validate.create");
        assert_eq!(params["create"]["input"]["kind"], "audio");
        assert_eq!(
            params["create"]["input"]["data"]["url"],
            "https://example.com/clip.wav"
        );
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
        "watch [watching] bucket=error ×3 every=60000ms last_probe_at=61000 reason=probe_timeout"
    );
}
