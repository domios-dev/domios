use super::*;
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
fn top_level_docs_parse() {
    let parsed = Cli::try_parse_from(["codeterm", "docs"]).expect("docs parses");
    assert!(matches!(parsed.command, Commands::Docs { .. }));
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
        "lmstudio",
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
            assert_eq!(chat_backend.as_deref(), Some("lmstudio"));
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
