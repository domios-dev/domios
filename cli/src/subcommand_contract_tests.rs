use super::*;
use crate::args::orchestrator::OrchestratorAction;
use crate::args::plan::PlanHandoffAction;
use clap::CommandFactory;

fn env_lock() -> &'static std::sync::Mutex<()> {
    crate::test_env_lock()
}

fn restore_env(key: &str, value: Option<String>) {
    match value {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

fn mem_subcommand() -> clap::Command {
    Cli::command()
        .find_subcommand("mem")
        .expect("Cli must expose `mem` subcommand")
        .clone()
}

#[test]
fn mem_run_worker_subcommand_exists() {
    let mem = mem_subcommand();
    let run_worker = mem
        .find_subcommand("run-worker")
        .expect("`mem run-worker` MUST exist — supervisor.rs spawns it");

    let kind_arg = run_worker
        .get_arguments()
        .find(|a| a.get_id() == "kind")
        .expect("`mem run-worker` MUST take a positional <kind> arg");
    assert!(
        kind_arg.is_required_set(),
        "<kind> must stay required so supervisor.rs args[2] is meaningful"
    );

    let db_path = run_worker
        .get_arguments()
        .find(|a| a.get_id() == "db_path")
        .expect("`mem run-worker` MUST take --db-path");
    let long = db_path
        .get_long()
        .expect("--db-path must remain a long flag");
    assert_eq!(
        long, "db-path",
        "supervisor.rs hardcodes `--db-path`; renaming this flag breaks the daemon at runtime"
    );
}

#[test]
fn mem_run_worker_accepts_all_kinds() {
    for kind in [
        "dreaming-light",
        "dreaming-deep",
        "dreaming-normalize",
        "librarian-once",
    ] {
        let res = Cli::try_parse_from([
            "codeterm",
            "mem",
            "run-worker",
            kind,
            "--db-path",
            "/tmp/x.db",
        ]);
        assert!(
                res.is_ok(),
                "`mem run-worker {}` MUST parse cleanly — supervisor.rs sends this exact arg list ({:?})",
                kind,
                res.err()
            );
    }
}

fn parse_plugin(args: &[&str]) -> PluginAction {
    let mut argv = vec!["codeterm", "plugin"];
    argv.extend_from_slice(args);
    match Cli::try_parse_from(argv)
        .expect("plugin args parse")
        .command
    {
        Commands::Plugin { action } => action,
        _ => panic!("expected plugin command"),
    }
}

#[test]
fn plugin_subcommands_parse() {
    match parse_plugin(&[
        "install",
        "--from-dir",
        "./provider-demo",
        "--id-override",
        "demo",
    ]) {
        PluginAction::Install {
            from_dir,
            id_override,
            ..
        } => {
            assert_eq!(from_dir.as_deref(), Some("./provider-demo"));
            assert_eq!(id_override.as_deref(), Some("demo"));
        }
        _ => panic!("expected plugin install"),
    }

    // --json is a global flag (lives on Cli, not the action).
    let cli = Cli::try_parse_from(["codeterm", "plugin", "list", "--json"])
        .expect("plugin list --json parses");
    assert!(cli.json);
    assert!(matches!(
        cli.command,
        Commands::Plugin {
            action: PluginAction::List
        }
    ));

    assert!(matches!(
        parse_plugin(&["update-system", "codex"]),
        PluginAction::UpdateSystem { id: Some(id) } if id == "codex"
    ));
    // Legacy alias must keep working.
    assert!(matches!(
        parse_plugin(&["system-update", "codex"]),
        PluginAction::UpdateSystem { id: Some(id) } if id == "codex"
    ));

    match parse_plugin(&[
        "channel",
        "add",
        "https://example.invalid/channel.git",
        "--name",
        "example",
    ]) {
        PluginAction::Channel {
            command: PluginChannelAction::Add { url, name },
        } => {
            assert_eq!(url, "https://example.invalid/channel.git");
            assert_eq!(name.as_deref(), Some("example"));
        }
        _ => panic!("expected plugin channel add"),
    }

    assert!(matches!(
        parse_plugin(&["channel", "diff", "example"]),
        PluginAction::Channel { command: PluginChannelAction::Diff { name: Some(name) } }
            if name == "example"
    ));

    assert!(matches!(
        parse_plugin(&["channel", "ensure-default"]),
        PluginAction::Channel {
            command: PluginChannelAction::EnsureDefault
        }
    ));

    assert!(matches!(
        parse_plugin(&["channel", "remove", "codeterm-core", "--force"]),
        PluginAction::Channel {
            command: PluginChannelAction::Remove { name, force: true }
        } if name == "codeterm-core"
    ));
}

#[test]
fn data_root_uses_absolute_env_override() {
    let _guard = env_lock().lock().unwrap();
    let prev = std::env::var("CODETERM_DATA_DIR").ok();
    let root = std::env::temp_dir().join(format!("codeterm-data-root-{}", uuid::Uuid::new_v4()));
    std::env::set_var("CODETERM_DATA_DIR", &root);
    assert_eq!(data_root(), root);
    restore_env("CODETERM_DATA_DIR", prev);
}

#[test]
fn data_root_ignores_relative_env_override() {
    let _guard = env_lock().lock().unwrap();
    let prev = std::env::var("CODETERM_DATA_DIR").ok();
    std::env::set_var("CODETERM_DATA_DIR", "relative-data");
    assert!(data_root().ends_with(data_dir_name()));
    restore_env("CODETERM_DATA_DIR", prev);
}

#[test]
fn data_root_uses_named_dev_instance() {
    let _guard = env_lock().lock().unwrap();
    let prev_data = std::env::var("CODETERM_DATA_DIR").ok();
    let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
    let prev_port = std::env::var("CODETERM_API_PORT").ok();
    std::env::remove_var("CODETERM_DATA_DIR");
    std::env::set_var("CODETERM_INSTANCE", "handoff");
    std::env::set_var("CODETERM_API_PORT", "7691");
    assert!(data_root().ends_with(".codeterm-dev-handoff"));
    restore_env("CODETERM_DATA_DIR", prev_data);
    restore_env("CODETERM_INSTANCE", prev_instance);
    restore_env("CODETERM_API_PORT", prev_port);
}

#[test]
fn top_level_docs_parse() {
    let parsed = Cli::try_parse_from(["codeterm", "docs"]).expect("docs parses");
    assert!(matches!(parsed.command, Commands::Docs { .. }));
}

#[test]
fn top_level_doc_parse() {
    let parsed = Cli::try_parse_from(["codeterm", "doc", "orchestration"]).expect("doc parses");
    match parsed.command {
        Commands::Doc { name } => assert_eq!(name, "orchestration"),
        _ => panic!("expected doc"),
    }
}

#[test]
fn plan_record_parses_batch_sugar() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "record",
        "pl_1",
        "--upsert-task",
        "handoff-cli",
        "--name",
        "Handoff CLI",
        "--sections",
        "cli,backend",
        "--status",
        "in_progress",
        "--body-file",
        "body.md",
        "--step",
        "decision:ship batch first",
        "--link",
        "docs/spec.md",
        "--key",
        "stable-key",
    ])
    .expect("plan record parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Record {
                    plan_id,
                    upsert_task,
                    name,
                    sections,
                    status,
                    body_file,
                    steps,
                    links,
                    key,
                    ..
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(upsert_task.as_deref(), Some("handoff-cli"));
            assert_eq!(name.as_deref(), Some("Handoff CLI"));
            assert_eq!(sections.as_deref(), Some("cli,backend"));
            assert_eq!(status.as_deref(), Some("in_progress"));
            assert_eq!(body_file.as_deref(), Some("body.md"));
            assert_eq!(steps, vec!["decision:ship batch first"]);
            assert_eq!(links, vec!["docs/spec.md"]);
            assert_eq!(key.as_deref(), Some("stable-key"));
        }
        _ => panic!("expected plan record"),
    }
}

#[test]
fn plan_task_update_parses_sections() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "update",
        "pl_1",
        "task_1",
        "--sections",
        "sec_a,sec_b",
    ])
    .expect("plan task update parses sections");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Update { sections, .. },
                },
        } => assert_eq!(sections.as_deref(), Some("sec_a,sec_b")),
        _ => panic!("expected plan task update"),
    }
}

#[test]
fn plan_task_link_and_relink_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "link",
        "pl_1",
        "task_1",
        "--sections",
        "backend",
    ])
    .expect("plan task link parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::Link { sections, .. },
                },
        } => assert_eq!(sections, "backend"),
        _ => panic!("expected plan task link"),
    }

    let parsed = Cli::try_parse_from(["codeterm", "plan", "relink", "pl_1", "--apply"])
        .expect("plan relink parses");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::Relink { apply, .. },
        } => assert!(apply),
        _ => panic!("expected plan relink"),
    }
}

#[test]
fn plan_task_list_parses_plan_and_status() {
    let parsed = Cli::try_parse_from([
        "codeterm", "plan", "task", "list", "pl_1", "--status", "blocked",
    ])
    .expect("plan task list parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action: crate::args::plan::PlanTaskAction::List { plan_id, status },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(status.as_deref(), Some("blocked"));
        }
        _ => panic!("expected plan task list"),
    }
}

#[test]
fn plan_task_add_parses_quality_fields() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "task",
        "add",
        "pl_1",
        "--name",
        "Quality lane",
        "--criterion",
        "criteria persist",
        "--criterion",
        "task show surfaces them",
        "--test-plan",
        "cargo test service::plan::tests::task_quality_fields_persist_and_reload",
        "--correctness",
        "round-trip unchanged",
    ])
    .expect("plan task add parses quality fields");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::Add {
                            criterion,
                            test_plan,
                            correctness,
                            ..
                        },
                },
        } => {
            assert_eq!(
                criterion,
                vec!["criteria persist", "task show surfaces them"]
            );
            assert_eq!(
                test_plan.as_deref(),
                Some("cargo test service::plan::tests::task_quality_fields_persist_and_reload")
            );
            assert_eq!(correctness.as_deref(), Some("round-trip unchanged"));
        }
        _ => panic!("expected plan task add"),
    }
}

#[test]
fn plan_get_parses_sections_lookup() {
    let parsed = Cli::try_parse_from(["codeterm", "plan", "get", "pl_1", "--sections"])
        .expect("plan get --sections parses");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::Get { plan_id, sections },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert!(sections);
        }
        _ => panic!("expected plan get"),
    }
}

#[test]
fn orchestrator_heartbeat_commands_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "orchestrator",
        "learn",
        "resolve heartbeat at send time",
        "--plan",
        "pl_1",
        "--tag",
        "heartbeat",
    ])
    .expect("orchestrator learn parses");
    match parsed.command {
        Commands::Orchestrator {
            action: crate::args::orchestrator::OrchestratorAction::Learn { text, plan, tag },
        } => {
            assert_eq!(text, "resolve heartbeat at send time");
            assert_eq!(plan.as_deref(), Some("pl_1"));
            assert_eq!(tag.as_deref(), Some("heartbeat"));
        }
        _ => panic!("expected orchestrator learn"),
    }

    let parsed = Cli::try_parse_from([
        "codeterm",
        "orchestrator",
        "heartbeat",
        "show",
        "--plan",
        "pl_1",
    ])
    .expect("orchestrator heartbeat show parses");
    match parsed.command {
        Commands::Orchestrator {
            action:
                crate::args::orchestrator::OrchestratorAction::Heartbeat {
                    action: crate::args::orchestrator::OrchestratorHeartbeatAction::Show { plan },
                },
        } => assert_eq!(plan.as_deref(), Some("pl_1")),
        _ => panic!("expected orchestrator heartbeat show"),
    }
}

#[test]
fn plan_goal_subcommands_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "goal",
        "set",
        "pl_1",
        "--vector",
        "Ship foundation",
        "--vision-file",
        "goal.md",
        "--constraint",
        "backend only",
        "--postulate",
        "reread plan",
    ])
    .expect("plan goal set parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Goal {
                    action:
                        crate::args::plan::PlanGoalAction::Set {
                            plan_id,
                            vector,
                            vision_file,
                            constraint,
                            postulate,
                            ..
                        },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(vector, "Ship foundation");
            assert_eq!(vision_file.as_deref(), Some("goal.md"));
            assert_eq!(constraint, vec!["backend only"]);
            assert_eq!(postulate, vec!["reread plan"]);
        }
        _ => panic!("expected plan goal set"),
    }

    let parsed =
        Cli::try_parse_from(["codeterm", "plan", "goal", "show", "pl_1"]).expect("show parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Goal {
                    action: crate::args::plan::PlanGoalAction::Show { plan_id },
                },
        } if plan_id == "pl_1"
    ));

    let parsed =
        Cli::try_parse_from(["codeterm", "plan", "goal", "get", "pl_1"]).expect("get parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Goal {
                    action: crate::args::plan::PlanGoalAction::Get { plan_id },
                },
        } if plan_id == "pl_1"
    ));
}

#[test]
fn plan_handoff_subcommands_parse() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "handoff",
        "create",
        "pl_1",
        "--mode",
        "new",
        "--task",
        "backend",
        "--goal",
        "Resume backend handoff",
        "--next",
        "run handoff show",
        "--rule",
        "never reseed parent",
        "--decision",
        "choose ack source",
        "--from-file",
        "handoff.md",
    ])
    .expect("plan handoff create parses");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action:
                        PlanHandoffAction::Create {
                            plan_id,
                            mode,
                            task,
                            goal,
                            next_actions,
                            foundational_rules,
                            open_decisions,
                            from_file,
                            stdin,
                        },
                },
        } => {
            assert_eq!(plan_id, "pl_1");
            assert_eq!(mode, "new");
            assert_eq!(task.as_deref(), Some("backend"));
            assert_eq!(goal.as_deref(), Some("Resume backend handoff"));
            assert_eq!(next_actions, vec!["run handoff show"]);
            assert_eq!(foundational_rules, vec!["never reseed parent"]);
            assert_eq!(open_decisions, vec!["choose ack source"]);
            assert_eq!(from_file.as_deref(), Some("handoff.md"));
            assert!(!stdin);
        }
        _ => panic!("expected plan handoff create"),
    }

    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "handoff",
        "show",
        "pl_1",
        "--seq",
        "3",
        "--markdown",
    ])
    .expect("plan handoff show parses with seq");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action:
                        PlanHandoffAction::Show {
                            plan_id,
                            seq,
                            markdown: true,
                            ..
                        },
                },
        } if plan_id == "pl_1" && seq.as_deref() == Some("3")
    ));

    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "handoff",
        "show",
        "pl_1",
        "--latest",
        "--markdown",
    ])
    .expect("plan handoff show parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action:
                        PlanHandoffAction::Show {
                            plan_id,
                            latest: true,
                            markdown: true,
                            seq: None,
                            ..
                        },
                },
        } if plan_id == "pl_1"
    ));

    let parsed = Cli::try_parse_from(["codeterm", "plan", "handoff", "list", "pl_1"])
        .expect("plan handoff list parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action:
                PlanAction::Handoff {
                    action: PlanHandoffAction::List { plan_id },
                },
        } if plan_id == "pl_1"
    ));
}

#[test]
fn orchestrator_handoff_reseed_dry_run_parses() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "orchestrator",
        "handoff",
        "--plan",
        "pl_1",
        "--task",
        "task_1",
        "--pane",
        "pane_1",
        "--reseed",
        "--dry-run",
        "--mode",
        "new",
        "--next",
        "resume from latest handoff",
    ])
    .expect("orchestrator handoff parses");
    match parsed.command {
        Commands::Orchestrator {
            action:
                OrchestratorAction::Handoff {
                    plan,
                    task,
                    pane,
                    reseed,
                    dry_run,
                    mode,
                    next_actions,
                    ..
                },
        } => {
            assert_eq!(plan.as_deref(), Some("pl_1"));
            assert_eq!(task.as_deref(), Some("task_1"));
            assert_eq!(pane.as_deref(), Some("pane_1"));
            assert!(reseed);
            assert!(dry_run);
            assert_eq!(mode, "new");
            assert_eq!(next_actions, vec!["resume from latest handoff"]);
        }
        _ => panic!("expected orchestrator handoff"),
    }
}

#[test]
fn pane_new_chat_backend_parses() {
    // P1: `codeterm pane new --chat-backend <id> --config <json>` is the CLI
    // entry point that opens a plugin-driven chat pane.
    let parsed = Cli::try_parse_from([
        "codeterm",
        "pane",
        "new",
        "--chat-backend",
        "local-shell",
        "--config",
        r#"{"model":"llama"}"#,
    ])
    .expect("pane new --chat-backend parses");
    match parsed.command {
        Commands::Pane {
            action:
                PaneAction::New {
                    chat_backend,
                    config,
                    ..
                },
        } => {
            assert_eq!(chat_backend.as_deref(), Some("local-shell"));
            assert_eq!(config.as_deref(), Some(r#"{"model":"llama"}"#));
        }
        _ => panic!("expected pane new"),
    }
}

#[test]
fn pane_new_without_chat_backend_defaults_to_none() {
    let parsed = Cli::try_parse_from(["codeterm", "pane", "new"]).expect("plain pane new parses");
    match parsed.command {
        Commands::Pane {
            action: PaneAction::New { chat_backend, .. },
        } => assert!(chat_backend.is_none()),
        _ => panic!("expected pane new"),
    }
}

#[test]
fn send_stdin_without_positional_parses() {
    let parsed = Cli::try_parse_from(["codeterm", "send", "--stdin", "--pane", "abc123"])
        .expect("send --stdin parses");
    assert!(matches!(
        parsed.command,
        Commands::SendText {
            text: None,
            stdin: true,
            ..
        }
    ));
}

#[test]
fn send_actions_without_text_parses() {
    let parsed =
        Cli::try_parse_from(["codeterm", "send", "--actions", "enter", "--pane", "abc123"])
            .expect("send --actions parses");
    assert!(matches!(
        parsed.command,
        Commands::SendText {
            text: None,
            actions: Some(ref v),
            ..
        } if v == "enter"
    ));
}

#[test]
fn send_actions_conflicts_with_delay() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "send",
        "--actions",
        "enter",
        "--delay",
        "10m",
        "--pane",
        "abc123",
    ]);
    assert!(parsed.is_err());
}

#[test]
fn send_delay_parses() {
    let parsed = Cli::try_parse_from(["codeterm", "send", "--delay", "10m", "hello"])
        .expect("send --delay parses");
    assert!(matches!(
        parsed.command,
        Commands::SendText {
            delay: Some(ref v),
            ..
        } if v == "10m"
    ));
}

#[test]
fn send_delay_conflicts_with_at() {
    match Cli::try_parse_from([
        "codeterm",
        "send",
        "--delay",
        "10m",
        "--at",
        "2026-06-14T13:00:00Z",
        "hello",
    ]) {
        Ok(_) => panic!("--delay and --at must conflict"),
        Err(err) => assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict),
    }
}

#[test]
fn delayed_create_parses() {
    let parsed = Cli::try_parse_from(["codeterm", "delayed", "create", "--delay", "5m", "hello"])
        .expect("delayed create parses");
    assert!(matches!(
        parsed.command,
        Commands::Delayed {
            action: DelayedAction::Create {
                delay: Some(ref v),
                ..
            }
        } if v == "5m"
    ));
}

#[test]
fn send_stdin_conflicts_with_positional_text() {
    match Cli::try_parse_from(["codeterm", "send", "--stdin", "hello"]) {
        Ok(_) => panic!("--stdin and positional text must conflict"),
        Err(err) => assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict),
    }
}

#[test]
fn workspace_reuse_notice_mentions_actual_and_requested_names() {
    let value = serde_json::json!({
        "id": "abcd1234",
        "name": "Real Workspace",
        "existing": true
    });
    assert_eq!(
            cmd::local_io::workspace_reuse_notice(&value, "Requested Workspace").as_deref(),
            Some("reused existing workspace abcd1234 'Real Workspace' (same cwd); requested name 'Requested Workspace' ignored")
        );
}

#[test]
fn workspace_reuse_notice_absent_for_new_or_unmarked_workspace() {
    let created = serde_json::json!({
        "id": "abcd1234",
        "name": "Requested Workspace",
        "existing": false
    });
    let unmarked = serde_json::json!({
        "id": "abcd1234",
        "name": "Requested Workspace"
    });
    assert!(cmd::local_io::workspace_reuse_notice(&created, "Requested Workspace").is_none());
    assert!(cmd::local_io::workspace_reuse_notice(&unmarked, "Requested Workspace").is_none());
}

#[test]
fn workspace_reuse_human_output_goes_to_stderr_only() {
    let value = serde_json::json!({
        "id": "abcd1234",
        "name": "Real Workspace",
        "existing": true
    });
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    assert!(cmd::local_io::write_workspace_create_reuse_output(
        &value,
        "Requested Workspace",
        false,
        &mut stdout,
        &mut stderr
    )
    .unwrap());

    assert!(stdout.is_empty());
    assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "reused existing workspace abcd1234 'Real Workspace' (same cwd); requested name 'Requested Workspace' ignored\n"
        );
}

#[test]
fn workspace_reuse_json_output_stays_on_stdout_only() {
    let value = serde_json::json!({
        "existing": true,
        "id": "abcd1234",
        "name": "Real Workspace"
    });
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    assert!(cmd::local_io::write_workspace_create_reuse_output(
        &value,
        "Requested Workspace",
        true,
        &mut stdout,
        &mut stderr
    )
    .unwrap());

    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap())
    );
}
