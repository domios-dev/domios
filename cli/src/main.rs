mod client;
mod cmd;
mod mesh_cli;
mod stdout;

use cmd::print::{handle_result, print_human, print_object};

use clap::Parser;
use client::{discover_socket, SocketClient};

pub use mesh_cli::{MeshAction, MeshPeerAction};

pub fn resolve_pane(client: &mut SocketClient, id: Option<String>) -> String {
    cmd::local_io::resolve_pane(client, id)
}

/// Determine the CodeTerm data directory.
/// Dev instances use `~/.codeterm-dev[-<slug>]/`, prod uses `~/.codeterm/`.
/// Detection priority:
///   1. CODETERM_SOCKET_PATH containing "codeterm-dev" (set by dev PTYs)
///   2. CODETERM_API_PORT == 7686 (dev's port — useful when calling CLI from
///      outside a dev pane, e.g. tests)
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
    if let Ok(root) = std::env::var("CODETERM_DATA_DIR") {
        if !root.trim().is_empty() {
            let path = std::path::PathBuf::from(root);
            if path.is_absolute() {
                return path;
            }
        }
    }
    dirs::home_dir().unwrap_or_default().join(data_dir_name())
}

/// Read API config (host, port, token).
/// Priority: $CODETERM_API_PORT env var (set by CodeTerm PTY) > config.toml.
/// This ensures CLI works in both dev (7686) and prod (7685) when called from inside CodeTerm.
pub fn read_api_config() -> (String, Option<String>) {
    let config_dir = data_root();
    let config_path = config_dir.join("config.toml");

    let (mut host, mut port) = ("127.0.0.1".to_string(), 7685i64);

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(val) = content.parse::<toml::Table>() {
            if let Some(api) = val.get("api").and_then(|v| v.as_table()) {
                let raw_host = api
                    .get("host")
                    .and_then(|v| v.as_str())
                    .unwrap_or("127.0.0.1");
                host = if raw_host == "0.0.0.0" {
                    "127.0.0.1".to_string()
                } else {
                    raw_host.to_string()
                };
                port = api.get("port").and_then(|v| v.as_integer()).unwrap_or(7685);
            }
        }
    }

    if let Ok(env_port) = std::env::var("CODETERM_API_PORT") {
        if let Ok(p) = env_port.parse::<i64>() {
            port = p;
        }
    }

    let config_token = || {
        std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|content| content.parse::<toml::Table>().ok())
            .and_then(|val| {
                val.get("api")?
                    .as_table()?
                    .get("token")?
                    .as_str()
                    .map(String::from)
            })
    };

    let signed_token = read_mesh_signed_token();

    let env_token = std::env::var("CODETERM_API_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());

    let targets_dev = std::env::var("CODETERM_INSTANCE")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("CODETERM_SOCKET_PATH")
            .map(|sp| sp.contains("codeterm-dev"))
            .unwrap_or(false)
        || std::env::var("CODETERM_API_PORT")
            .map(|p| !p.trim().is_empty() && p.trim() != "7685")
            .unwrap_or(false);

    let token = if targets_dev {
        config_token().or(signed_token).or(env_token)
    } else {
        env_token.or(signed_token).or_else(config_token)
    };

    (format!("http://{}:{}", host, port), token)
}

/// Produce a fresh Ed25519 signed token from this machine's mesh identity.
///
/// Distinct from `read_api_config().1`: peer auth requires the signed token
/// regardless of `CODETERM_API_TOKEN` env override. The env token is the
/// LOCAL plain API token — paired peers don't recognise it, so using it for
/// outbound mesh calls produces a silent 401 with a `text/plain` body that
/// the JSON parser swallows.
pub fn read_mesh_signed_token() -> Option<String> {
    let config_dir = data_root();
    let identity_path = config_dir.join("mesh").join("identity.json");
    let content = std::fs::read_to_string(&identity_path).ok()?;
    let id: serde_json::Value = serde_json::from_str(&content).ok()?;
    let pk = id.get("private_key")?.as_str()?;
    let iid = id.get("instance_id")?.as_str()?;
    mesh_cli::sign_request(pk, iid).ok()
}

/// Build HTTP client with auth header
pub fn build_http(token: &Option<String>) -> Result<reqwest::blocking::Client, reqwest::Error> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(t) = token {
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)) {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }
    }
    if let Ok(pane) = std::env::var("CODETERM_PANE_ID") {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&pane) {
            headers.insert("X-CodeTerm-Pane-Id", v);
        }
    }
    if let Ok(worker) = std::env::var("CODETERM_WORKER_ID") {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&worker) {
            headers.insert("X-CodeTerm-Worker-Id", v);
        }
    }
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
}

mod args;
pub use args::agent::AgentAction;
pub use args::cli::{Cli, Commands};
pub use args::cron::CronAction;
pub use args::delayed::DelayedAction;
pub use args::effort::EffortAction;
pub use args::mem::{
    MemAction, MemCalibrationAction, MemDreamAction, MemGraphAction, MemSecretAction,
    MemSkillAction, MemWorkerAction,
};
pub use args::misc::{
    BridgeAction, ConfigAction, KeepAwakeAction, NotificationAction, PaneAction, PasteAction,
    ThemeAction, WebAction, WorkspaceAction,
};
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

/// Resolve pane ID: explicit arg > $CODETERM_PANE_ID env var > focused pane from server.
///
/// When running inside CodeTerm, $CODETERM_PANE_ID is set automatically by the PTY.
/// This ensures commands like `pane split` always target the caller's pane,
/// not whatever pane the user happens to have focused.
fn main() {
    stdout::run(main_impl);
}

fn main_impl() {
    let args: Vec<String> = normalize_agent_spawn_label_args(std::env::args().collect());
    let cli = Cli::try_parse_from(&args).unwrap_or_else(|e| {
        if is_plan_dash_leading_text_error(&args, &e) {
            println!(
                "✗ dash-leading free text was parsed as a flag.\nfix: use --field=value or --field-file PATH, for example --body='- note' or --body-file notes.md."
            );
            std::process::exit(2);
        }
        e.exit()
    });

    if let Commands::Plan {
        action: PlanAction::Delete { plan_id, yes },
    } = &cli.command
    {
        if let Err(e) = cmd::plan::validate_plan_delete_confirmation(plan_id, *yes) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    // `plugin` is filesystem-local (no daemon) — handle before socket/mesh.
    if matches!(cli.command, Commands::Plugin { .. }) {
        match cli.command {
            Commands::Plugin { action } => cmd::plugin::handle_plugin(cli.json, action),
            _ => unreachable!(),
        }
        return;
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

    let json_output = cli.json;
    if let Commands::Config { ref action } = cli.command {
        let (base_url, token) = read_api_config();
        let http = match build_http(&token) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: failed to build HTTP client: {}", e);
                std::process::exit(1);
            }
        };
        let result = match action {
            ConfigAction::Get { key } => {
                let url = format!("{}/api/config?key={}", base_url, key);
                match http.get(&url).send() {
                    Ok(resp) if resp.status().is_success() => {
                        resp.json().map_err(|e| e.to_string())
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().unwrap_or_default();
                        if status.as_u16() == 404 {
                            Err(format!("Key not found: {}", key))
                        } else {
                            Err(format!("API error ({}): {}", status, body))
                        }
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            ConfigAction::Set { key, value } => {
                let json_value: serde_json::Value = serde_json::from_str(value)
                    .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
                let body = serde_json::json!({ "key": key, "value": json_value });
                match http
                    .patch(format!("{}/api/config", base_url))
                    .json(&body)
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        Ok(serde_json::json!({"ok": true, "key": key, "value": json_value}))
                    }
                    Ok(resp) => Err(format!("Failed: {}", resp.text().unwrap_or_default())),
                    Err(e) => Err(e.to_string()),
                }
            }
        };
        match result {
            Ok(value) => {
                if json_output {
                    println!("{}", serde_json::to_string_pretty(&value).unwrap());
                } else if let Some(v) = value.get("value") {
                    match v {
                        serde_json::Value::String(s) => println!("{}", s),
                        serde_json::Value::Null => println!("null"),
                        other => println!("{}", other),
                    }
                } else {
                    print_human(&value);
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // stale CODETERM_SOCKET_PATH inherited from a previous daemon PID (e.g. after update install --auto restarts the binary) doesn't make the worker exit 1 before it can do any work.
    if let Commands::Mem {
        action: MemAction::RunWorker {
            ref kind,
            ref db_path,
        },
    } = cli.command
    {
        cmd::worker::run_mem_worker(kind, db_path.as_deref());
        return;
    }

    // Local-only graph mutation verbs — open knowledge.db directly, no daemon.
    if let Commands::Mem {
        action: MemAction::Graph { ref action },
    } = cli.command
    {
        if let Some(()) = cmd::graph_mutate::try_dispatch_local(action) {
            unreachable!("local graph dispatch must exit");
        }
    }

    // `mem dream` opens knowledge.db directly. Bridge ping is best-effort and routed through `discover_socket` itself so a wedged daemon doesn't break the planner.
    if let Commands::Mem {
        action: MemAction::Dream { .. },
    } = cli.command
    {
        match cli.command {
            Commands::Mem {
                action: MemAction::Dream { action },
            } => {
                cmd::dream::handle_dream(action);
            }
            _ => unreachable!(),
        }
    }

    // during recovery when the daemon is wedged.
    if matches!(cli.command, Commands::System { .. }) {
        let json_mode = cli.json;
        match cli.command {
            Commands::System { action } => cmd::system::handle_system(json_mode, action),
            _ => unreachable!(),
        }
        return;
    }

    // `cli-manifest` introspects the clap tree — no daemon dependency.
    if matches!(cli.command, Commands::CliManifest { .. }) {
        match cli.command {
            Commands::CliManifest { write, markdown } => cmd::manifest::handle(write, markdown),
            _ => unreachable!(),
        }
        return;
    }

    // `docs` / `doc` read embedded markdown — no daemon dependency.
    if matches!(cli.command, Commands::Docs { .. } | Commands::Doc { .. }) {
        match cli.command {
            Commands::Docs { name, full, head } => cmd::docs::handle(name, full, head),
            Commands::Doc { name } => cmd::docs::handle_doc(name),
            _ => unreachable!(),
        }
        return;
    }

    let socket_path = match discover_socket() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut client = match SocketClient::connect(&socket_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let result = match cli.command {
        Commands::Identify => client.call("system.identify", None),

        Commands::Capabilities => client.call("system.capabilities", None),

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

        Commands::Orchestration { action } => {
            match cmd::orchestration::handle(&mut client, cli.json, action) {
                Ok(Some(value)) => Ok(value),
                Ok(None) => return,
                Err(e) => Err(e),
            }
        }

        Commands::Tab { action } => {
            cmd::pane::handle_pane(&mut client, cli.json, action);
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

        Commands::SendText {
            pane,
            text,
            from_file,
            stdin,
            actions,
            delay,
            at,
        } => {
            cmd::local_io::handle_send_text(
                &mut client,
                cli.json,
                pane,
                text,
                from_file,
                stdin,
                actions,
                delay,
                at,
            );
            return;
        }

        Commands::Delayed { action } => {
            cmd::delayed::handle_delayed(&mut client, cli.json, action);
            return;
        }

        Commands::Notify { title, body, level } => {
            let mut params = serde_json::json!({ "title": title, "level": level });
            if let Some(b) = body {
                params["body"] = serde_json::Value::String(b);
            }
            client.call("notification.create", Some(params))
        }

        Commands::Bridge { action } => {
            cmd::bridge::handle_bridge(&mut client, cli.json, action);
            return;
        }

        Commands::Notifications { action, unread } => match action {
            Some(NotificationAction::Clear) => client.call("notification.clear", None),
            None => client.call(
                "notification.list",
                Some(serde_json::json!({ "unread_only": unread })),
            ),
        },

        Commands::Keepawake { action } => {
            let (base, token) = read_api_config();
            let http = match build_http(&token) {
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

        Commands::Report(args) => {
            cmd::agent::handle_agent(&mut client, cli.json, AgentAction::Report(args));
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

        Commands::Canvas { action } => {
            cmd::task::handle_canvas(&mut client, cli.json, action);
            return;
        }

        Commands::Theme { action } => {
            // Theme commands use REST API (port 7685)
            let base = "http://127.0.0.1:7685";
            let http = reqwest::blocking::Client::new();
            match action {
                ThemeAction::List => match http.get(format!("{}/api/themes", base)).send() {
                    Ok(resp) => {
                        let val: serde_json::Value = resp.json().unwrap_or_default();
                        if let Some(themes) = val.get("themes").and_then(|v| v.as_array()) {
                            for t in themes {
                                println!("{}", t.as_str().unwrap_or("?"));
                            }
                        }
                        return;
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                },
                ThemeAction::Apply { name } => {
                    match http
                        .post(format!("{}/api/themes/apply", base))
                        .json(&serde_json::json!({ "name": name }))
                        .send()
                    {
                        Ok(_) => {
                            println!("Theme applied: {}", name);
                            return;
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ThemeAction::Save { id, file, apply } => {
                    let toml = match std::fs::read_to_string(&file) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Error reading {}: {}", file, e);
                            std::process::exit(1);
                        }
                    };
                    match http
                        .post(format!("{}/api/themes", base))
                        .json(&serde_json::json!({ "id": id, "toml": toml }))
                        .send()
                    {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                                println!("Theme saved: {}", path);
                            } else {
                                println!("Theme saved: {}", id);
                            }
                            if apply {
                                match http
                                    .post(format!("{}/api/themes/apply", base))
                                    .json(&serde_json::json!({ "name": id }))
                                    .send()
                                {
                                    Ok(_) => println!("Theme applied: {}", id),
                                    Err(e) => eprintln!("Theme saved but failed to apply: {}", e),
                                }
                            }
                            return;
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ThemeAction::Delete { id } => {
                    match http.delete(format!("{}/api/themes/{}", base, id)).send() {
                        Ok(_) => {
                            println!("Theme deleted: {}", id);
                            return;
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ThemeAction::Dir => {
                    let dir = dirs::config_dir()
                        .map(|d| d.join("codeterm").join("themes"))
                        .unwrap_or_default();
                    println!("{}", dir.display());
                    return;
                }
            }
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
        Commands::Doc { .. } => unreachable!(),

        // Config is handled before socket connect (uses HTTP API)
        Commands::Config { .. } => unreachable!(),

        // cli-manifest is handled before socket connect (introspects clap tree)
        Commands::CliManifest { .. } => unreachable!(),

        // plugin is handled before socket connect (filesystem-local)
        Commands::Plugin { .. } => unreachable!(),

        Commands::Mesh { action } => {
            cmd::mesh::handle_mesh(cli.json, action);
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
            let http = match build_http(&token) {
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
            let http = match build_http(&token) {
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
        if arg == "--title" || arg == "--description" {
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
            | "--allow-shared-checkout"
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
