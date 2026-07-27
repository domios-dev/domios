use super::*;
use crate::args::agent::AgentAction;
use crate::args::delayed::DelayedAction;
use crate::args::mem::MemAction;
use crate::args::misc::TabAction;
use crate::args::orchestrator::OrchestratorAction;
use crate::args::task::TaskAction;
use clap::{CommandFactory, Parser};

#[derive(Parser)]
struct MemOnly {
    #[command(subcommand)]
    action: MemAction,
}

#[derive(Parser)]
struct TaskOnly {
    #[command(subcommand)]
    action: TaskAction,
}

#[derive(Parser)]
struct OrchestratorOnly {
    #[command(subcommand)]
    action: OrchestratorAction,
}

#[derive(Parser)]
struct TabOnly {
    #[command(subcommand)]
    action: TabAction,
}

#[derive(Parser)]
struct AgentOnly {
    #[command(subcommand)]
    action: AgentAction,
}

#[derive(Parser)]
struct DelayedOnly {
    #[command(subcommand)]
    action: DelayedAction,
}

#[derive(Parser)]
enum DocsOnly {
    Docs,
    Doc { name: String },
}

#[derive(Parser)]
struct SendOnly {
    #[arg(allow_hyphen_values = true)]
    text: Option<String>,
    #[arg(long = "tab")]
    tab: Option<String>,
    #[arg(long, value_name = "PATH")]
    from_file: Option<String>,
    #[arg(long, visible_alias = "from-stdin", conflicts_with = "text")]
    stdin: bool,
    #[arg(long, conflicts_with_all = ["delay", "at"])]
    actions: Option<String>,
    #[arg(long, conflicts_with = "at")]
    delay: Option<String>,
    #[arg(long, conflicts_with = "delay")]
    at: Option<String>,
}

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
    MemOnly::command()
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
        let res = MemOnly::try_parse_from(["mem", "run-worker", kind, "--db-path", "/tmp/x.db"]);
        assert!(
                res.is_ok(),
                "`mem run-worker {}` MUST parse cleanly — supervisor.rs sends this exact arg list ({:?})",
                kind,
                res.err()
            );
    }
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
fn task_show_parses_siblings_flags() {
    let parsed = TaskOnly::try_parse_from(["task", "show", "task_1", "--siblings", "--all"])
        .expect("task show --siblings --all parses");
    match parsed.action {
        TaskAction::Show {
            task_id,
            siblings,
            all,
        } => {
            assert_eq!(task_id, "task_1");
            assert!(siblings);
            assert!(all);
        }
        _ => panic!("expected task show"),
    }
}

#[test]
fn top_level_docs_parse() {
    let parsed = DocsOnly::try_parse_from(["codeterm", "docs"]).expect("docs parses");
    assert!(matches!(parsed, DocsOnly::Docs));
}

#[test]
fn top_level_doc_parse() {
    let parsed =
        DocsOnly::try_parse_from(["codeterm", "doc", "orchestration"]).expect("doc parses");
    match parsed {
        DocsOnly::Doc { name } => assert_eq!(name, "orchestration"),
        _ => panic!("expected doc"),
    }
}

mod plan;

#[test]
fn orchestrator_handoff_reseed_dry_run_parses() {
    let parsed = OrchestratorOnly::try_parse_from([
        "orchestrator",
        "handoff",
        "--plan",
        "pl_1",
        "--task",
        "task_1",
        "--tab",
        "tab_1",
        "--reseed",
        "--dry-run",
        "--mode",
        "new",
        "--next",
        "resume from latest handoff",
    ])
    .expect("orchestrator handoff parses");
    match parsed.action {
        OrchestratorAction::Handoff {
            plan,
            task,
            tab,
            reseed,
            dry_run,
            mode,
            next_actions,
            ..
        } => {
            assert_eq!(plan.as_deref(), Some("pl_1"));
            assert_eq!(task.as_deref(), Some("task_1"));
            assert_eq!(tab.as_deref(), Some("tab_1"));
            assert!(reseed);
            assert!(dry_run);
            assert_eq!(mode, "new");
            assert_eq!(next_actions, vec!["resume from latest handoff"]);
        }
        _ => panic!("expected orchestrator handoff"),
    }
}

#[test]
fn tab_new_chat_backend_parses() {
    // entry point that opens a plugin-driven chat tab.
    let parsed = TabOnly::try_parse_from([
        "tab",
        "new",
        "--chat-backend",
        "local-shell",
        "--config",
        r#"{"model":"llama"}"#,
    ])
    .expect("tab new --chat-backend parses");
    match parsed.action {
        TabAction::New {
            chat_backend,
            config,
            ..
        } => {
            assert_eq!(chat_backend.as_deref(), Some("local-shell"));
            assert_eq!(config.as_deref(), Some(r#"{"model":"llama"}"#));
        }
        _ => panic!("expected tab new"),
    }
}

#[test]
fn tab_new_without_chat_backend_defaults_to_none() {
    let parsed = TabOnly::try_parse_from(["tab", "new"]).expect("plain tab new parses");
    match parsed.action {
        TabAction::New { chat_backend, .. } => assert!(chat_backend.is_none()),
        _ => panic!("expected tab new"),
    }
}

#[test]
fn tab_list_defaults_to_compact_current_workspace_scope() {
    let parsed = TabOnly::try_parse_from(["tab", "list"]).expect("tab list parses");
    assert!(matches!(
        parsed.action,
        TabAction::List {
            workspace: None,
            all_workspaces: false,
            agents: false,
        }
    ));
}

#[test]
fn tab_list_and_inspect_parse_explicit_scopes() {
    let listed = TabOnly::try_parse_from(["tab", "list", "--workspace", "ws_1", "--agents"])
        .expect("scoped tab list parses");
    assert!(matches!(
        listed.action,
        TabAction::List {
            workspace: Some(ref workspace),
            all_workspaces: false,
            agents: true,
        } if workspace == "ws_1"
    ));
    let inspected =
        TabOnly::try_parse_from(["tab", "inspect", "tab_1", "tab_2"]).expect("tab inspect parses");
    assert!(matches!(
        inspected.action,
        TabAction::Inspect { tabs } if tabs == ["tab_1", "tab_2"]
    ));
    let all = TabOnly::try_parse_from(["tab", "list", "--all-workspaces"])
        .expect("all-workspaces tab list parses");
    assert!(matches!(
        all.action,
        TabAction::List {
            workspace: None,
            all_workspaces: true,
            agents: false,
        }
    ));
    assert!(
        TabOnly::try_parse_from(["tab", "list", "--workspace", "ws_1", "--all-workspaces"])
            .is_err()
    );
}

#[test]
fn tab_command_alias_is_rejected() {
    assert!(Cli::try_parse_from(["codeterm", "tab", "list"]).is_err());
}

#[test]
fn tab_new_is_primary_command() {
    let tab = Cli::try_parse_from(["codeterm", "tab", "new"]).expect("tab new parses");
    assert!(matches!(
        tab.command,
        Commands::Tab {
            action: TabAction::New { .. }
        }
    ));
}

#[test]
fn send_stdin_without_positional_parses() {
    let parsed = SendOnly::try_parse_from(["send", "--stdin", "--tab", "abc123"])
        .expect("send --stdin parses");
    assert!(parsed.text.is_none());
    assert!(parsed.stdin);
}

#[test]
fn send_tab_target_parses() {
    let parsed = SendOnly::try_parse_from(["send", "--stdin", "--tab", "abc123"])
        .expect("send --tab parses");
    assert_eq!(parsed.tab.as_deref(), Some("abc123"));
}

#[test]
fn send_actions_without_text_parses() {
    let parsed = SendOnly::try_parse_from(["send", "--actions", "enter", "--tab", "abc123"])
        .expect("send --actions parses");
    assert!(parsed.text.is_none());
    assert_eq!(parsed.actions.as_deref(), Some("enter"));
}

#[test]
fn send_actions_conflicts_with_delay() {
    let parsed = SendOnly::try_parse_from([
        "send",
        "--actions",
        "enter",
        "--delay",
        "10m",
        "--tab",
        "abc123",
    ]);
    assert!(parsed.is_err());
}

#[test]
fn send_delay_parses() {
    let parsed =
        SendOnly::try_parse_from(["send", "--delay", "10m", "hello"]).expect("send --delay parses");
    assert_eq!(parsed.delay.as_deref(), Some("10m"));
}

#[test]
fn send_delay_conflicts_with_at() {
    match SendOnly::try_parse_from([
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
    let parsed = DelayedOnly::try_parse_from(["delayed", "create", "--delay", "5m", "hello"])
        .expect("delayed create parses");
    assert!(matches!(
        parsed.action,
        DelayedAction::Create {
            delay: Some(ref v),
            ..
        } if v == "5m"
    ));
}

#[test]
fn send_stdin_conflicts_with_positional_text() {
    match SendOnly::try_parse_from(["send", "--stdin", "hello"]) {
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
