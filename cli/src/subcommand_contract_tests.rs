use super::*;
use crate::args::agent::AgentAction;
use crate::args::delayed::DelayedAction;
use crate::args::inbox::InboxAction;
use crate::args::mem::MemAction;
use crate::args::misc::TabAction;
use crate::args::orchestrator::OrchestratorAction;
use crate::args::task::TaskAction;
use clap::{CommandFactory, Parser};

mod context;
mod transport;
mod workspace;

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
struct InboxOnly {
    #[command(subcommand)]
    action: InboxAction,
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
fn mem_recall_is_a_hidden_exact_alias_for_search() {
    fn parsed_search(
        argv: &[&str],
    ) -> (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        usize,
        bool,
    ) {
        match MemOnly::try_parse_from(argv)
            .expect("mem search spelling parses")
            .action
        {
            MemAction::Search {
                query,
                kind,
                area,
                scope,
                project,
                limit,
                rerank,
            } => (query, kind, area, scope, project, limit, rerank),
            _ => panic!("expected mem search"),
        }
    }

    let args = [
        "needle",
        "--kind",
        "fact",
        "--area",
        "release",
        "--scope",
        "global",
        "--project",
        "/tmp/project",
        "--limit",
        "7",
        "--rerank",
    ];
    let canonical = std::iter::once("mem")
        .chain(std::iter::once("search"))
        .chain(args)
        .collect::<Vec<_>>();
    let alias = std::iter::once("mem")
        .chain(std::iter::once("recall"))
        .chain(args)
        .collect::<Vec<_>>();

    assert_eq!(parsed_search(&canonical), parsed_search(&alias));
    let help = mem_subcommand().render_long_help().to_string();
    assert!(
        !help.contains("recall"),
        "hidden alias leaked into help: {help}"
    );
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
fn standalone_task_add_parses_qa_plan_file() {
    let parsed = InboxOnly::try_parse_from([
        "inbox",
        "add",
        "--name",
        "QA lifecycle",
        "--qa-plan-file",
        "qa-plan.md",
    ])
    .expect("inbox add --qa-plan-file parses");
    match parsed.action {
        InboxAction::Add { qa_plan_file, .. } => {
            assert_eq!(qa_plan_file.as_deref(), Some("qa-plan.md"))
        }
    }
}

#[test]
fn removed_spellings_are_rejected() {
    for argv in [
        vec!["codeterm", "report", "--status", "in_review", "x"],
        vec!["codeterm", "plan", "goal", "get", "pl_x"],
        vec!["codeterm", "notifications", "clear"],
        vec!["codeterm", "notify", "hello"],
        vec!["codeterm", "orchestration", "new"],
        vec!["codeterm", "task", "add", "--name", "x"],
    ] {
        assert!(
            Cli::try_parse_from(argv.clone()).is_err(),
            "{argv:?} must no longer parse"
        );
    }
}

#[test]
fn retained_spellings_still_parse() {
    let report = Cli::try_parse_from(["codeterm", "agent", "report", "done"])
        .expect("agent report still parses");
    assert!(matches!(
        report.command,
        Commands::Agent {
            action: AgentAction::Report(_)
        }
    ));

    let goal = Cli::try_parse_from(["codeterm", "plan", "goal", "show", "pl_x"])
        .expect("plan goal show still parses");
    assert!(matches!(
        goal.command,
        Commands::Plan {
            action: PlanAction::Goal {
                action: crate::args::plan::PlanGoalAction::Show { .. }
            }
        }
    ));

    let send = Cli::try_parse_from(["codeterm", "notify", "send", "hello", "--level", "warning"])
        .expect("notify send parses");
    assert!(matches!(
        send.command,
        Commands::Notify {
            action: NotifyAction::Send { ref title, ref level, .. }
        } if title == "hello" && level == "warning"
    ));

    let list = Cli::try_parse_from(["codeterm", "notify", "list", "--unread"])
        .expect("notify list parses");
    assert!(matches!(
        list.command,
        Commands::Notify {
            action: NotifyAction::List { unread: true }
        }
    ));

    let clear = Cli::try_parse_from(["codeterm", "notify", "clear"]).expect("notify clear parses");
    assert!(matches!(
        clear.command,
        Commands::Notify {
            action: NotifyAction::Clear
        }
    ));

    let inbox =
        Cli::try_parse_from(["codeterm", "inbox", "add", "--name", "x"]).expect("inbox add parses");
    assert!(matches!(
        inbox.command,
        Commands::Inbox {
            action: InboxAction::Add { ref name, .. }
        } if name == "x"
    ));

    for leaf in [
        vec!["codeterm", "orchestrator", "status"],
        vec!["codeterm", "orchestrator", "inspect"],
        vec!["codeterm", "orchestrator", "learn", "a lesson"],
        vec!["codeterm", "orchestrator", "heartbeat", "show"],
        vec!["codeterm", "orchestrator", "refresh"],
        vec!["codeterm", "orchestrator", "handoff"],
    ] {
        let parsed = Cli::try_parse_from(leaf.clone())
            .unwrap_or_else(|error| panic!("{leaf:?} must parse: {error}"));
        assert!(matches!(parsed.command, Commands::Orchestrator { .. }));
    }
}

// Form 1 — notify. DECISION: DOCUMENT `notify send` as canonical; do NOT ship a
// bare `notify "title"` alias. A default-subcommand + positional-title mix routes
// titles equal to a subcommand name (send/list/clear) to the wrong action
// silently, and there is no positional+optional-subcommand precedent in the tree.
#[test]
fn notify_send_is_canonical_and_bare_title_is_rejected() {
    let send = Cli::try_parse_from(["codeterm", "notify", "send", "Done", "--level", "info"])
        .expect("notify send parses");
    assert!(matches!(
        send.command,
        Commands::Notify {
            action: NotifyAction::Send { ref title, ref level, body: None, .. }
        } if title == "Done" && level == "info"
    ));
    // The intuitive bare form must stay a hard error, not silently do the wrong thing.
    assert!(
        Cli::try_parse_from(["codeterm", "notify", "Done"]).is_err(),
        "bare `notify \"title\"` must be rejected — `notify send` is canonical"
    );
}

// The push category is a closed value_enum: an invented category must fail at
// parse time rather than reach the daemon as a runtime string.
#[test]
fn notify_category_is_closed_and_progress_is_unexpressible() {
    use crate::args::misc::NotifyCategory;

    for (flag, expected) in [
        ("milestone", NotifyCategory::Milestone),
        ("blocker", NotifyCategory::Blocker),
        ("failure", NotifyCategory::Failure),
    ] {
        let parsed =
            Cli::try_parse_from(["codeterm", "notify", "send", "Done", "--category", flag])
                .unwrap_or_else(|e| panic!("--category {flag} must parse: {e}"));
        assert!(matches!(
            parsed.command,
            Commands::Notify {
                action: NotifyAction::Send { category: Some(c), .. }
            } if c == expected
        ));
    }

    for rejected in ["progress", "update", "info", "milestones"] {
        assert!(
            Cli::try_parse_from(["codeterm", "notify", "send", "Done", "--category", rejected])
                .is_err(),
            "--category {rejected} must be a parse error, not a pass-through string"
        );
    }

    let uncategorised = Cli::try_parse_from(["codeterm", "notify", "send", "Done"])
        .expect("category stays optional");
    assert!(matches!(
        uncategorised.command,
        Commands::Notify {
            action: NotifyAction::Send {
                category: None,
                event_key: None,
                ..
            }
        }
    ));
}

// Form 3 — config get. ACCEPT: key is optional; absent means "dump all".
#[test]
fn config_get_key_is_optional_and_scoped_forms_are_distinct() {
    let dump =
        Cli::try_parse_from(["codeterm", "config", "get"]).expect("keyless config get parses");
    assert!(matches!(
        dump.command,
        Commands::Config {
            action: ConfigAction::Get { key: None }
        }
    ));
    let scoped = Cli::try_parse_from(["codeterm", "config", "get", "api.port"])
        .expect("config get <key> parses");
    assert!(matches!(
        scoped.command,
        Commands::Config {
            action: ConfigAction::Get { key: Some(ref k) }
        } if k == "api.port"
    ));
    // Mechanism: the built request path omits `?key=` for the keyless dump and
    // scopes it when a key is present.
    assert_eq!(
        crate::transport::pre_socket::config_get_path(None),
        "/api/config"
    );
    assert_eq!(
        crate::transport::pre_socket::config_get_path(Some("api.port")),
        "/api/config?key=api.port"
    );
}

// Form 2 — paste get/rm. ACCEPT: a bare id normalizes to the stored `paste:`
// prefix, so the accepted (bare) form routes to the SAME request URL as the
// canonical (prefixed) form.
#[test]
fn paste_get_and_rm_normalize_bare_id_to_prefixed_url() {
    for verb in ["get", "rm"] {
        let bare = Cli::try_parse_from(["codeterm", "paste", verb, "fpu2zp"])
            .expect("bare paste id parses")
            .command;
        let prefixed = Cli::try_parse_from(["codeterm", "paste", verb, "paste:fpu2zp"])
            .expect("prefixed paste id parses")
            .command;
        let (bare_method, bare_path, _) = crate::mesh_cli::api_map::cli_command_to_api(&bare);
        let (pref_method, pref_path, _) = crate::mesh_cli::api_map::cli_command_to_api(&prefixed);
        assert_eq!(bare_method, pref_method, "`paste {verb}` method must match");
        assert_eq!(
            bare_path, pref_path,
            "`paste {verb}` bare and prefixed ids must build the same URL"
        );
        assert!(
            bare_path.contains("/api/pasteboard/paste:fpu2zp"),
            "`paste {verb}` must normalize to the stored paste: key, got {bare_path}"
        );
    }
}

#[test]
fn task_read_leaves_are_preserved() {
    assert!(matches!(
        TaskOnly::try_parse_from(["task", "list"])
            .expect("task list parses")
            .action,
        TaskAction::List { .. }
    ));
    assert!(matches!(
        TaskOnly::try_parse_from(["task", "show", "task_1"])
            .expect("task show parses")
            .action,
        TaskAction::Show { .. }
    ));
    assert!(matches!(
        TaskOnly::try_parse_from(["task", "take", "task_1"])
            .expect("task take parses")
            .action,
        TaskAction::Take { .. }
    ));
    assert!(matches!(
        TaskOnly::try_parse_from(["task", "drop", "task_1"])
            .expect("task drop parses")
            .action,
        TaskAction::Drop { .. }
    ));
    assert!(
        Cli::try_parse_from(["codeterm", "inbox", "list"]).is_err(),
        "the inbox group holds only `add`"
    );
}

#[test]
fn plan_task_add_is_unchanged() {
    let parsed = Cli::try_parse_from(["codeterm", "plan", "task", "add", "pl_x", "--name", "y"])
        .expect("plan task add still parses");
    assert!(matches!(
        parsed.command,
        Commands::Plan {
            action: PlanAction::Task {
                action: crate::args::plan::PlanTaskAction::Add { ref plan_id, ref name, .. }
            }
        } if plan_id == "pl_x" && name == "y"
    ));
}

fn pair_command(extra: &[&str]) -> Commands {
    let mut argv = vec![
        "codeterm",
        "team",
        "pair",
        "--orchestrator",
        "claude:opus",
        "--planner",
        "codex",
    ];
    argv.extend_from_slice(extra);
    Cli::try_parse_from(argv).expect("team pair parses").command
}

#[test]
fn team_pair_reaches_the_same_creation_path() {
    let command = pair_command(&["--workspace", "ws_1", "--title", "My Plan"]);
    let (method, path, body) = crate::mesh_cli::api_map::cli_command_to_api(&command);

    assert_ne!(method, crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/orchestration/new");

    let body: serde_json::Value =
        serde_json::from_str(&body.expect("pair posts a body")).expect("body is JSON");
    assert_eq!(body["orchestrator_agent"], "claude");
    assert_eq!(body["orchestrator_model"], "opus");
    assert_eq!(body["planner_agent"], "codex");
    assert!(body.get("planner_model").is_none());
    assert_eq!(body["workspace_id"], "ws_1");
    assert_eq!(body["title"], "My Plan");

    let minimal = pair_command(&[]);
    let (_, _, minimal_body) = crate::mesh_cli::api_map::cli_command_to_api(&minimal);
    let minimal_body: serde_json::Value =
        serde_json::from_str(&minimal_body.expect("pair posts a body")).expect("body is JSON");
    let minimal_body = minimal_body.as_object().expect("object body");
    for required in ["orchestrator_agent", "orchestrator_model", "planner_agent"] {
        assert!(minimal_body.contains_key(required), "missing {required}");
    }
    assert!(!minimal_body.contains_key("planner_model"));
    // The pre-change `orchestration new` body carried exactly these six fields.
    for key in minimal_body.keys() {
        assert!(
            [
                "orchestrator_agent",
                "orchestrator_model",
                "planner_agent",
                "planner_model",
                "workspace_id",
                "title",
            ]
            .contains(&key.as_str()),
            "unexpected body field {key}"
        );
    }
}

#[test]
fn team_non_pair_actions_keep_meshrpc_path() {
    for argv in [
        vec!["codeterm", "team", "list"],
        vec!["codeterm", "team", "show", "team_1"],
        vec!["codeterm", "team", "rename", "team_1", "name"],
        vec!["codeterm", "team", "close", "team_1"],
        vec!["codeterm", "team", "adopt-plan", "team_1", "pl_1"],
    ] {
        let command = Cli::try_parse_from(argv.clone())
            .unwrap_or_else(|error| panic!("{argv:?} must parse: {error}"))
            .command;
        assert!(
            crate::mesh_cli::is_socket_relayed(&command),
            "{argv:?} must keep the MeshRpcClient path"
        );
        let (method, reason, _) = crate::mesh_cli::api_map::cli_command_to_api(&command);
        assert_eq!(method, crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH);
        assert_eq!(
            reason,
            "team management is local-only — run it on the target machine"
        );
    }

    let pair = pair_command(&[]);
    assert!(
        !crate::mesh_cli::is_socket_relayed(&pair),
        "team pair must fall through to the REST mapping"
    );
}

#[test]
fn team_pair_starts_unbound() {
    let command = pair_command(&["--workspace", "ws_1", "--title", "My Plan"]);
    let (_, _, body) = crate::mesh_cli::api_map::cli_command_to_api(&command);
    let body: serde_json::Value =
        serde_json::from_str(&body.expect("pair posts a body")).expect("body is JSON");
    let object = body.as_object().expect("object body");

    assert!(object.contains_key("orchestrator_agent"));
    assert!(object.contains_key("planner_agent"));
    for plan_key in ["plan_id", "plan", "plan_task", "goal"] {
        assert!(
            !object.contains_key(plan_key),
            "pair creation must carry no plan binding, found {plan_key}"
        );
    }

    let params = crate::cmd::team::pair_params("claude:opus", "codex", None, None);
    let params = params.as_object().expect("object params");
    assert!(params.contains_key("orchestrator_agent"));
    assert!(params.contains_key("planner_agent"));
    for plan_key in ["plan_id", "plan", "plan_task", "goal"] {
        assert!(!params.contains_key(plan_key));
    }
}

#[test]
fn top_level_docs_parse() {
    let parsed = Cli::try_parse_from(["codeterm", "docs"]).expect("docs parses");
    assert!(matches!(parsed.command, Commands::Docs { name: None }));
}

#[test]
fn named_docs_parse_without_selectors() {
    let parsed =
        Cli::try_parse_from(["codeterm", "docs", "ORCHESTRATION"]).expect("named document parses");

    match parsed.command {
        Commands::Docs { name } => assert_eq!(name.as_deref(), Some("ORCHESTRATION")),
        _ => panic!("expected docs"),
    }
}

#[test]
fn removed_docs_selectors_and_alias_are_rejected() {
    for args in [
        vec!["codeterm", "docs", "ORCHESTRATION", "--section", "the-loop"],
        vec!["codeterm", "docs", "ORCHESTRATION", "--full"],
        vec!["codeterm", "docs", "ORCHESTRATION", "--head", "12"],
        vec!["codeterm", "doc", "ORCHESTRATION"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
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
fn tab_restart_parses_replacement_shell_parameters() {
    let parsed = TabOnly::try_parse_from([
        "tab",
        "restart",
        "--tab",
        "tab-1",
        "--cwd",
        "D:/repo",
        "--cmd",
        ".\\build.ps1 dev",
    ])
    .expect("tab restart parses");
    match parsed.action {
        TabAction::Restart { tab, cwd, cmd } => {
            assert_eq!(tab.as_deref(), Some("tab-1"));
            assert_eq!(cwd.as_deref(), Some("D:/repo"));
            assert_eq!(cmd.as_deref(), Some(".\\build.ps1 dev"));
        }
        _ => panic!("expected tab restart"),
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
        TabAction::Inspect { tabs, tab, lifecycle: false } if tabs == ["tab_1", "tab_2"] && tab.is_empty()
    ));
    let canonical = TabOnly::try_parse_from(["tab", "inspect", "--tab", "tab_1", "--tab", "tab_2"])
        .expect("canonical tab selectors parse");
    assert!(matches!(
        canonical.action,
        TabAction::Inspect { tabs, tab, lifecycle: false } if tabs.is_empty() && tab == ["tab_1", "tab_2"]
    ));
    let lifecycle = TabOnly::try_parse_from(["tab", "inspect", "--tab", "tab_1", "--lifecycle"])
        .expect("lifecycle inspection parses");
    assert!(matches!(
        lifecycle.action,
        TabAction::Inspect {
            lifecycle: true,
            ..
        }
    ));
    assert!(TabOnly::try_parse_from(["tab", "focus", "--tab", "tab_1"]).is_ok());
    assert!(TabOnly::try_parse_from(["tab", "close", "--tab", "tab_1"]).is_ok());
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
fn every_tab_command_accepts_canonical_tab_flag() {
    for command in ["focus", "close"] {
        let mut parsed = TabOnly::try_parse_from(["tab", command, "--tab", "tab_1"])
            .expect("canonical selector parses");
        crate::args::misc::normalize_required_tab_selector(&mut parsed.action)
            .expect("canonical selector normalizes");
        assert!(matches!(
            parsed.action,
            TabAction::Focus { id: Some(ref id), tab: None }
                | TabAction::Close { id: Some(ref id), tab: None }
                if id == "tab_1"
        ));
    }
    let mut positional = TabOnly::try_parse_from(["tab", "close", "tab_2"]).unwrap();
    crate::args::misc::normalize_required_tab_selector(&mut positional.action).unwrap();
    assert!(matches!(
        positional.action,
        TabAction::Close { id: Some(ref id), tab: None } if id == "tab_2"
    ));
    let mut missing = TabOnly::try_parse_from(["tab", "close"]).unwrap();
    let error = crate::args::misc::normalize_required_tab_selector(&mut missing.action)
        .expect_err("missing selector is a CLI error");
    assert_eq!(error, "tab close requires --tab <ID>");
}

#[test]
fn tab_command_alias_is_rejected() {
    assert!(Cli::try_parse_from(["codeterm", "pane", "list"]).is_err());
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
fn from_pad_parses_for_every_text_body_consumer() {
    for argv in [
        vec!["codeterm", "send", "--from-pad", "message.md"],
        vec![
            "codeterm",
            "delayed",
            "create",
            "--delay",
            "5m",
            "--from-pad",
            "message.md",
        ],
        vec![
            "codeterm",
            "delayed",
            "edit",
            "delayed-1",
            "--from-pad",
            "message.md",
        ],
        vec!["codeterm", "agent", "report", "--from-pad", "report.md"],
        vec!["codeterm", "paste", "put", "--from-pad", "message.md"],
        vec![
            "codeterm",
            "bridge",
            "send",
            "--to",
            "42",
            "--from-pad",
            "message.md",
        ],
    ] {
        Cli::try_parse_from(argv).expect("--from-pad consumer must parse");
    }
}

#[test]
fn from_pad_conflicts_with_other_body_sources() {
    for argv in [
        vec!["codeterm", "send", "inline", "--from-pad", "message.md"],
        vec![
            "codeterm",
            "send",
            "--from-file",
            "other.md",
            "--from-pad",
            "message.md",
        ],
        vec![
            "codeterm",
            "delayed",
            "create",
            "inline",
            "--delay",
            "5m",
            "--from-pad",
            "message.md",
        ],
        vec![
            "codeterm",
            "agent",
            "report",
            "inline",
            "--from-pad",
            "report.md",
        ],
        vec![
            "codeterm",
            "agent",
            "report",
            "--doc",
            "report.md",
            "--from-pad",
            "report.md",
        ],
        vec![
            "codeterm",
            "paste",
            "put",
            "inline",
            "--from-pad",
            "message.md",
        ],
        vec![
            "codeterm",
            "bridge",
            "send",
            "inline",
            "--to",
            "42",
            "--from-pad",
            "message.md",
        ],
    ] {
        match Cli::try_parse_from(argv) {
            Ok(_) => panic!("--from-pad body source conflict must fail"),
            Err(error) => assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict),
        }
    }
}

#[test]
fn pad_path_and_list_parse_without_a_tab_selector() {
    Cli::try_parse_from(["codeterm", "pad", "path"]).unwrap();
    Cli::try_parse_from(["codeterm", "pad", "list"]).unwrap();
}

#[test]
fn send_stdin_conflicts_with_positional_text() {
    match SendOnly::try_parse_from(["send", "--stdin", "hello"]) {
        Ok(_) => panic!("--stdin and positional text must conflict"),
        Err(err) => assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict),
    }
}
