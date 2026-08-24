mod client;
mod cmd;
mod local_api;
mod mesh_cli;
mod protocol;
mod stdout;
mod transport;

use cmd::identify::{resolve_api_config, resolved_data_root, resolved_instance};
use cmd::print::{handle_result, print_object};

use clap::Parser;
use client::{discover_socket, discover_socket_resolution, SocketClient};

pub use mesh_cli::{MeshAction, MeshPeerAction};

/// Resolve tab ID: explicit arg > $CODETERM_TAB_ID env var > refuse.
///
/// When running inside CodeTerm, $CODETERM_TAB_ID is set automatically by the PTY.
/// There is no focused-tab fallback: a caller that can prove neither is refused,
/// so a command can never act on a tab the caller does not own.
pub fn resolve_tab(client: &mut SocketClient, id: Option<String>) -> String {
    cmd::local_io::resolve_tab(client, id)
}

/// Determine the CodeTerm data directory.
/// Dev instances use `~/.codeterm-dev[-<slug>]/`, prod uses `~/.codeterm/`.
/// Detection priority:
///   1. CODETERM_SOCKET_PATH containing "codeterm-dev" (set by dev PTYs)
///   2. CODETERM_API_PORT == 7686 (dev's port — useful when calling CLI from
///      outside a dev tab, e.g. tests)
///   3. Default: prod
///
/// When a dev context is detected and CODETERM_INSTANCE is set, the slug is
/// appended (`~/.codeterm-dev-<slug>`) so per-worktree instances stay isolated.
pub fn data_dir_name() -> String {
    let is_dev = std::env::var("CODETERM_SOCKET_PATH")
        .map(|sp| sp.contains("codeterm-dev"))
        .unwrap_or(false)
        || std::env::var("CODETERM_API_PORT").as_deref() == Ok("7686")
        || std::env::var("CODETERM_INSTANCE")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
    if is_dev {
        return format!(".codeterm-dev{}", instance_suffix());
    }
    ".codeterm".to_string()
}

/// Suffix for a named dev instance selected via CODETERM_INSTANCE.
/// Sanitized to `[a-z0-9-]+`; empty when unset.
fn instance_suffix() -> String {
    match std::env::var("CODETERM_INSTANCE") {
        Ok(s) if !s.trim().is_empty() => {
            let slug: String = s
                .trim()
                .to_lowercase()
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            format!("-{}", slug)
        }
        _ => String::new(),
    }
}

/// Resolve the CodeTerm data root used by local filesystem commands.
///
/// `CODETERM_DATA_DIR` is an explicit test/dev harness override. It must be
/// absolute so a typo cannot silently route plugin operations into a relative
/// workspace path.
pub fn data_root() -> std::path::PathBuf {
    resolved_data_root().value
}

/// Read API config (host, port, token).
/// Priority: $CODETERM_API_PORT env var (set by CodeTerm PTY) > config.toml.
/// This ensures CLI works in both dev (7686) and prod (7685) when called from inside CodeTerm.
pub fn read_api_config() -> (String, Option<String>) {
    let resolved = resolve_api_config();
    (
        format!("http://{}:{}", resolved.host, resolved.port),
        resolved.token,
    )
}

pub fn build_local_api() -> Result<local_api::LocalApiClient, local_api::LocalApiError> {
    local_api::LocalApiClient::connect()
}

mod args;
pub use args::agent::AgentAction;
pub use args::cli::{Cli, Commands};
pub use args::cron::CronAction;
pub use args::delayed::DelayedAction;
pub use args::effort::EffortAction;
pub use args::inbox::InboxAction;
pub use args::mem::{
    MemAction, MemCalibrationAction, MemDreamAction, MemGraphAction, MemSecretAction,
    MemSkillAction, MemWorkerAction,
};
pub use args::misc::{
    BridgeAction, ConfigAction, KeepAwakeAction, NotifyAction, PasteAction, TabAction, ThemeAction,
    WebAction, WorkspaceAction,
};
pub use args::pad::PadAction;
pub use args::plan::PlanAction;
pub use args::plugin::{PluginAction, PluginChannelAction};
pub use args::service::ServiceAction;
pub use args::sessions::SessionsAction;
pub use args::spec::SpecAction;
pub use args::system::SystemAction;
pub use args::task::{CanvasAction, TaskAction};
pub use args::team::TeamAction;
pub use args::update::UpdateAction;
pub use args::usage::UsageAction;

fn main() {
    stdout::run(main_impl);
}

fn main_impl() {
    let args: Vec<String> = normalize_agent_spawn_label_args(std::env::args().collect());
    if matches!(
        args.as_slice(),
        [_, command, flag]
            if command == "docs" && (flag == "--help" || flag == "-h")
    ) {
        print!("{}", cmd::docs::render_help());
        return;
    }
    hydrate_tab_task_binding_env(&args);
    let mut cli = Cli::try_parse_from(&args).unwrap_or_else(|e| {
        if is_plan_dash_leading_text_error(&args, &e) {
            println!(
                "✗ dash-leading free text was parsed as a flag.\nfix: use --field=value or --field-file PATH, for example --body='- note' or --body-file notes.md."
            );
            std::process::exit(2);
        }
        e.exit()
    });
    if let Commands::Tab { action } = &mut cli.command {
        if let Err(error) = args::misc::normalize_required_tab_selector(action) {
            eprintln!("Error: {error}");
            std::process::exit(2);
        }
    }

    if let Commands::Plan {
        action: PlanAction::Delete { plan_id, yes },
    } = &cli.command
    {
        if let Err(e) = cmd::plan::validate_plan_delete_confirmation(plan_id, *yes) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    if let Some(mesh_target) = cli.mesh.clone() {
        // Secrets are local-only — refuse to proxy them across the mesh. Cross-mesh secret access needs a trust model we haven't designed yet.
        if matches!(
            cli.command,
            Commands::Mem {
                action: MemAction::Secret { .. }
            }
        ) {
            eprintln!(
                "Error: secrets do not cross the mesh. Run on the local machine \
                 (vault peer), or design a trust model first."
            );
            std::process::exit(7);
        }
        if matches!(
            cli.command,
            Commands::Bridge {
                action: BridgeAction::Migrate { .. }
            }
        ) {
            eprintln!(
                "Error: bridge migrate must run locally on the source machine, not via --mesh."
            );
            std::process::exit(6);
        }
        mesh_cli::run_via_mesh(cli, &mesh_target);
        return;
    }

    if transport::should_dispatch_direct_local(&cli.command, cli.mesh.as_deref()) {
        match cli.command {
            Commands::Plugin { action } => cmd::plugin::handle_plugin(cli.json, action),
            _ => unreachable!(),
        }
        return;
    }

    let Some(cli) = transport::pre_socket::dispatch(cli) else {
        return;
    };

    let resolved_socket = match discover_socket_resolution() {
        Ok(resolved) => resolved,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut client = match SocketClient::connect(&resolved_socket.path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let result = match cli.command {
        Commands::Identify => cmd::identify::handle(&mut client, cli.json, resolved_socket),

        Commands::Capabilities => {
            let result = client.call("system.capabilities", None);
            if cli.json {
                result
            } else {
                match result {
                    Ok(value) => print!("{}", cmd::capabilities::render(&value)),
                    Err(error) => {
                        eprintln!("Error: {error}");
                        std::process::exit(1);
                    }
                }
                return;
            }
        }

        Commands::Workspace { action } => {
            match cmd::local_io::handle_workspace_action(&mut client, action, cli.json) {
                Some(result) => result,
                None => return,
            }
        }

        Commands::Team { action } => {
            cmd::team::handle(&mut client, cli.json, action);
            return;
        }

        Commands::Orchestrator { action } => {
            match cmd::orchestrator::handle(&mut client, cli.json, action) {
                Ok(Some(value)) => Ok(value),
                Ok(None) => return,
                Err(e) => Err(e),
            }
        }

        Commands::Persona { action } => match cmd::persona::handle(&mut client, cli.json, action) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => return,
            Err(e) => Err(e),
        },

        Commands::Tab { action } => {
            cmd::tab::handle_tab(&mut client, cli.json, action);
            return;
        }

        Commands::Web { action } => {
            cmd::web::handle_web(cli.json, action);
            return;
        }

        Commands::Paste { action } => {
            cmd::paste::handle_paste(cli.json, action);
            return;
        }

        Commands::Pad { action } => {
            cmd::pad::handle_pad(cli.json, action);
            return;
        }

        Commands::SendText {
            tab,
            text,
            from_file,
            from_pad,
            stdin,
            actions,
            delay,
            at,
            require_agent,
            to_shell,
        } => {
            cmd::local_io::handle_send_text(
                &mut client,
                cli.json,
                tab,
                text,
                from_file,
                from_pad,
                stdin,
                actions,
                delay,
                at,
                require_agent,
                to_shell,
            );
            return;
        }

        Commands::Delayed { action } => {
            cmd::delayed::handle_delayed(&mut client, cli.json, action);
            return;
        }

        Commands::Notify { action } => match action {
            NotifyAction::Send {
                title,
                from_pad,
                level,
                category,
                event_key,
                ..
            } => client.call(
                "notification.create",
                Some(cmd::notify::send_params(
                    &title,
                    &level,
                    category,
                    event_key.as_deref(),
                    from_pad.as_deref(),
                )),
            ),
            NotifyAction::List { unread } => client.call(
                "notification.list",
                Some(serde_json::json!({ "unread_only": unread })),
            ),
            NotifyAction::Clear => client.call("notification.clear", None),
        },

        Commands::Bridge { action } => {
            cmd::bridge::handle_bridge(&mut client, cli.json, action);
            return;
        }

        Commands::Keepawake { action } => {
            let (base, _) = read_api_config();
            let http = match build_local_api() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let result: Result<serde_json::Value, String> =
                match action.unwrap_or(KeepAwakeAction::Status) {
                    KeepAwakeAction::Status => http
                        .get(format!("{}/api/keepawake", base))
                        .send()
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.json::<serde_json::Value>().map_err(|e| e.to_string())),
                    KeepAwakeAction::Start {
                        duration_secs,
                        display,
                        gnome_policy,
                        system_block,
                    } => http
                        .post(format!("{}/api/keepawake/start", base))
                        .json(&serde_json::json!({
                            "duration_secs": duration_secs,
                            "display": display,
                            "gnome_policy": gnome_policy,
                            "system_block": system_block,
                        }))
                        .send()
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.json::<serde_json::Value>().map_err(|e| e.to_string())),
                    KeepAwakeAction::Stop => http
                        .post(format!("{}/api/keepawake/stop", base))
                        .send()
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.json::<serde_json::Value>().map_err(|e| e.to_string())),
                };
            match result {
                Ok(val) => {
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&val).unwrap());
                    } else {
                        print_object(&val);
                    }
                    return;
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Agent { action } => {
            cmd::agent::handle_agent(&mut client, cli.json, action);
            return;
        }

        Commands::Plan { action } => {
            cmd::plan::handle_plan(&mut client, cli.json, action);
            return;
        }

        Commands::Spec { action } => {
            cmd::spec::handle_spec(&mut client, cli.json, action);
            return;
        }

        Commands::Effort { action } => {
            cmd::effort::handle_effort(&mut client, cli.json, action);
            return;
        }

        Commands::Task { action } => {
            cmd::task::handle_task(&mut client, cli.json, action);
            return;
        }

        Commands::Inbox { action } => {
            cmd::task::handle_inbox(&mut client, cli.json, action);
            return;
        }

        Commands::Canvas { action } => {
            cmd::task::handle_canvas(&mut client, cli.json, action);
            return;
        }

        Commands::Theme { action } => {
            cmd::theme::handle_theme(cli.json, action);
            return;
        }

        Commands::Cron { action } => {
            cmd::cron::handle_cron(&mut client, cli.json, action);
            return;
        }

        Commands::Service { action } => {
            cmd::service::handle_service(&mut client, cli.json, action);
            return;
        }

        Commands::Update {
            action,
            manifest_url,
            after_update,
        } => {
            cmd::update::handle_update(cli.json, action, manifest_url, after_update);
            return;
        }

        // Docs are handled before socket connect (embedded markdown)
        Commands::Docs { .. } => unreachable!(),

        // Config is handled before socket connect (uses HTTP API)
        Commands::Config { .. } => unreachable!(),

        // cli-manifest is handled before socket connect (introspects clap tree)
        Commands::CliManifest { .. } => unreachable!(),

        // plugin is handled before socket connect (filesystem-local)
        Commands::Plugin { .. } => unreachable!(),

        Commands::Mesh { action } => {
            cmd::mesh::handle_mesh(&mut client, cli.json, action);
            return;
        }
        Commands::Mem { action } => {
            if let MemAction::RunWorker { kind, db_path } = action {
                cmd::worker::run_mem_worker(&kind, db_path.as_deref());
                return;
            }
            let json_mode = cli.json;
            cmd::mem::handle_mem(&mut client, json_mode, action);
            return;
        }
        Commands::Sessions { action } => {
            let (base_url, token) = read_api_config();
            let http = match build_local_api() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            cmd::sessions::handle_sessions(cli.json, action, &http, &base_url, token.as_deref());
            return;
        }
        Commands::Usage { action } => {
            let (base_url, token) = read_api_config();
            let http = match build_local_api() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            cmd::usage::handle_usage(cli.json, action, &http, &base_url, token.as_deref());
            return;
        }
        Commands::System { action } => {
            cmd::system::handle_system(cli.json, action);
            return;
        }
        Commands::Debug { action } => {
            use args::debug::DebugAction;
            match action {
                DebugAction::Crash { kind } => {
                    let res = client.call(
                        "debug.crash",
                        Some(serde_json::json!({ "kind": kind.as_str() })),
                    );
                    match res {
                        Ok(_) => {
                            eprintln!(
                                "[debug.crash] trigger dispatched (kind={}). For segv/abort the daemon will die; check ~/.codeterm[-dev]/crashes/ for the report.",
                                kind.as_str()
                            );
                        }
                        Err(e) => {
                            // Expected for segv/abort — connection drops mid-call.
                            eprintln!("[debug.crash] dispatch returned error (expected for segv/abort): {}", e);
                        }
                    }
                    return;
                }
            }
        }
    };

    handle_result(cli.json, result);
}

fn hydrate_tab_task_binding_env(args: &[String]) {
    if args
        .iter()
        .any(|arg| arg == "--mesh" || arg.starts_with("--mesh="))
        || !args
            .iter()
            .any(|arg| matches!(arg.as_str(), "agent" | "report" | "orchestrator" | "plan"))
    {
        return;
    }
    let Ok(tab_id) = std::env::var("CODETERM_TAB_ID") else {
        return;
    };
    if tab_id.trim().is_empty()
        || (nonempty_env("CODETERM_PLAN_ID").is_some()
            && nonempty_env("CODETERM_TASK_ID").is_some())
    {
        return;
    }
    let Ok(socket_path) = discover_socket() else {
        return;
    };
    let Ok(mut client) = SocketClient::connect(&socket_path) else {
        return;
    };
    let Ok(response) = client.call(
        "plan.binding.resolve",
        Some(serde_json::json!({"tab_id": tab_id})),
    ) else {
        return;
    };
    let Some(binding) = response.get("binding").and_then(|value| value.as_object()) else {
        return;
    };
    if let Some(plan_id) = binding.get("plan_id").and_then(|value| value.as_str()) {
        std::env::set_var("CODETERM_PLAN_ID", plan_id);
    }
    if nonempty_env("CODETERM_TASK_ID").is_none() {
        if let Some(task_id) = binding.get("task_id").and_then(|value| value.as_str()) {
            std::env::set_var("CODETERM_TASK_ID", task_id);
        }
    }
}

pub(crate) fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_agent_spawn_label_args(args: Vec<String>) -> Vec<String> {
    let Some(spawn_idx) = args
        .windows(2)
        .position(|pair| pair[0] == "agent" && pair[1] == "spawn")
    else {
        return args;
    };

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(&args[..spawn_idx + 2]);
    let mut i = spawn_idx + 2;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--args" || arg.starts_with("--args=") {
            out.extend_from_slice(&args[i..]);
            break;
        } else if arg == "--title" || arg == "--description" {
            let flag = arg.clone();
            i += 1;
            let start = i;
            let mut words = Vec::new();
            while i < args.len() && !is_agent_spawn_flag(&args[i]) {
                words.push(args[i].clone());
                i += 1;
            }
            if words.is_empty() {
                out.push(flag);
            } else {
                out.push(format!("{flag}={}", words.join(" ")));
            }
            if i == start {
                continue;
            }
        } else {
            out.push(arg.clone());
            i += 1;
        }
    }
    out
}

fn is_agent_spawn_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--title"
            | "--description"
            | "--cwd"
            | "--role"
            | "--parent-tab"
            | "--plan"
            | "--report-to"
            | "--can-spawn"
            | "--task"
            | "--task-file"
            | "--args"
            | "--workspace"
            | "--model"
            | "--system"
            | "--preset"
            | "--charter"
            | "--charter-file"
            | "--interval"
            | "--nudge-cooldown"
            | "--max-actions"
            | "--task-id"
            | "--plan-task"
            | "--"
    ) || arg.starts_with("--title=")
        || arg.starts_with("--description=")
}

fn is_plan_dash_leading_text_error(args: &[String], _err: &clap::Error) -> bool {
    args.iter().any(|arg| arg == "plan")
        && args.iter().skip(1).any(|arg| {
            arg.starts_with('-')
                && !arg.starts_with("--")
                && arg.len() > 1
                && arg.chars().nth(1).is_some_and(|ch| !ch.is_ascii_digit())
        })
}

#[cfg(test)]
fn test_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod plan_cli_error_tests {
    use super::*;

    #[test]
    fn detects_dash_leading_plan_text_parse_error() {
        let args = vec![
            "codeterm".to_string(),
            "plan".to_string(),
            "task".to_string(),
            "add".to_string(),
            "plan_1".to_string(),
            "--body".to_string(),
            "- starts with a dash".to_string(),
        ];
        let err = match Cli::try_parse_from(&args) {
            Ok(_) => panic!("dash-leading text should fail clap parsing"),
            Err(err) => err,
        };
        assert!(is_plan_dash_leading_text_error(&args, &err));
    }

    #[test]
    fn normalizes_agent_spawn_unquoted_label_words() {
        let args = normalize_agent_spawn_label_args(
            [
                "codeterm",
                "agent",
                "spawn",
                "claude",
                "--title",
                "perf",
                "brainstorm",
                "opus",
                "--task",
                "review",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        assert!(args.contains(&"--title=perf brainstorm opus".to_string()));
        let cli = Cli::try_parse_from(args).expect("normalized spawn parses");
        match cli.command {
            Commands::Agent {
                action:
                    AgentAction::Spawn {
                        title, task, agent, ..
                    },
            } => {
                assert_eq!(agent, "claude");
                assert_eq!(
                    title.map(|parts| parts.join(" ")).as_deref(),
                    Some("perf brainstorm opus")
                );
                assert_eq!(task.as_deref(), Some("review"));
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn normalizes_agent_spawn_dash_leading_label_without_eating_known_flags() {
        let args = normalize_agent_spawn_label_args(
            [
                "codeterm",
                "agent",
                "spawn",
                "claude",
                "--title",
                "--literal",
                "flag",
                "--task",
                "echo",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        assert!(args.contains(&"--title=--literal flag".to_string()));
        let cli = Cli::try_parse_from(args).expect("normalized dash-leading title parses");
        match cli.command {
            Commands::Agent {
                action: AgentAction::Spawn { title, task, .. },
            } => {
                assert_eq!(
                    title.map(|parts| parts.join(" ")).as_deref(),
                    Some("--literal flag")
                );
                assert_eq!(task.as_deref(), Some("echo"));
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn agent_spawn_provider_args_bypass_label_normalization() {
        let original: Vec<String> = [
            "codeterm",
            "agent",
            "spawn",
            "claude",
            "--role",
            "Worker",
            "--args",
            "--title",
            "provider",
            "session",
            "--description",
            "verbatim",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let args = normalize_agent_spawn_label_args(original.clone());

        assert_eq!(args, original);
        let cli = Cli::try_parse_from(args).expect("provider args should parse");
        match cli.command {
            Commands::Agent {
                action: AgentAction::Spawn { role, args, .. },
            } => {
                assert!(matches!(role, Some(crate::args::agent::SpawnRole::Worker)));
                assert_eq!(
                    args,
                    Some(vec![
                        "--title".into(),
                        "provider".into(),
                        "session".into(),
                        "--description".into(),
                        "verbatim".into(),
                    ])
                );
            }
            _ => panic!("expected agent spawn"),
        }
    }

    #[test]
    fn normalizes_agent_spawn_marker_label_without_treating_it_as_a_flag() {
        let args = normalize_agent_spawn_label_args(
            [
                "codeterm",
                "agent",
                "spawn",
                "claude",
                "--title",
                "-=-codeterm:tab",
                "label=main-=-",
                "--task",
                "echo",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        assert!(args.contains(&"--title=-=-codeterm:tab label=main-=-".to_string()));
        let cli = Cli::try_parse_from(args).expect("normalized marker title parses");
        match cli.command {
            Commands::Agent {
                action: AgentAction::Spawn { title, task, .. },
            } => {
                assert_eq!(
                    title.map(|parts| parts.join(" ")).as_deref(),
                    Some("-=-codeterm:tab label=main-=-")
                );
                assert_eq!(task.as_deref(), Some("echo"));
            }
            _ => panic!("expected agent spawn"),
        }
    }
}

#[cfg(test)]
mod attachment_contract_tests;
#[cfg(test)]
mod plan_planning_gate_tests;
#[cfg(test)]
mod plan_ref_contract_tests;
#[cfg(test)]
mod plan_skill_cli_tests;
#[cfg(test)]
mod plan_task_attempt_tests;
#[cfg(test)]
mod plan_task_role_guidance_tests;
#[cfg(test)]
mod plugin_cli_tests;
#[cfg(test)]
mod subcommand_contract_tests;
#[cfg(test)]
mod zero_flag_plan_context_tests;
