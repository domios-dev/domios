use std::io::Write;

mod client;
mod cmd;
mod mesh_cli;

use cmd::print::{handle_result, print_human, print_object};

use clap::Parser;
use client::{discover_socket, SocketClient};

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC sequences: ESC [ ... final_byte
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc.is_ascii_alphabetic() || nc == '~' || nc == 'h' || nc == 'l' {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                // OSC sequence: ESC ] ... BEL or ST
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == '\x07' {
                        break;
                    }
                    if nc == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
        } else if c == '\r' {
            // Skip carriage return
        } else {
            out.push(c);
        }
    }
    out
}

fn workspace_reuse_notice(value: &serde_json::Value, requested_name: &str) -> Option<String> {
    if value.get("existing").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let ws_name = value.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let ws_id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    Some(format!(
        "reused existing workspace {} '{}' (same cwd); requested name '{}' ignored",
        ws_id, ws_name, requested_name
    ))
}

fn write_workspace_create_reuse_output<W: Write, E: Write>(
    value: &serde_json::Value,
    requested_name: &str,
    json: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> std::io::Result<bool> {
    let Some(notice) = workspace_reuse_notice(value, requested_name) else {
        return Ok(false);
    };
    if json {
        writeln!(stdout, "{}", serde_json::to_string_pretty(value).unwrap())?;
    } else {
        writeln!(stderr, "{}", notice)?;
    }
    Ok(true)
}

pub use mesh_cli::{MeshAction, MeshPeerAction};

/// Determine the CodeTerm data directory.
/// Dev instances use `~/.codeterm-dev/`, prod uses `~/.codeterm/`.
/// Detection priority:
///   1. CODETERM_SOCKET_PATH containing "codeterm-dev" (set by dev PTYs)
///   2. CODETERM_API_PORT == 7686 (dev's port — useful when calling CLI from
///      outside a dev pane, e.g. tests)
///   3. Default: prod
pub fn data_dir_name() -> &'static str {
    if let Ok(sp) = std::env::var("CODETERM_SOCKET_PATH") {
        if sp.contains("codeterm-dev") {
            return ".codeterm-dev";
        }
    }
    if std::env::var("CODETERM_API_PORT").as_deref() == Ok("7686") {
        return ".codeterm-dev";
    }
    ".codeterm"
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

    // Override port from env — CodeTerm sets this in spawned PTYs
    if let Ok(env_port) = std::env::var("CODETERM_API_PORT") {
        if let Ok(p) = env_port.parse::<i64>() {
            port = p;
        }
    }

    // Try Ed25519 signed token first, fall back to simple API token from config
    let signed_token = read_mesh_signed_token();

    // Highest priority: explicit CODETERM_API_TOKEN env override. Useful for hitting dev (port 7686) from outside its PTY without the matching
    // mesh identity, or for tests with a manually scoped token.
    let token = std::env::var("CODETERM_API_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .or(signed_token)
        .or_else(|| {
            let config_path = config_dir.join("config.toml");
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
        });

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
    // No fallback — propagate the error so callers know auth headers were dropped
}

mod args;
pub use args::agent::AgentAction;
pub use args::cli::{Cli, Commands};
pub use args::cron::CronAction;
pub use args::mem::{
    MemAction, MemCalibrationAction, MemDreamAction, MemGraphAction, MemSecretAction,
    MemSkillAction, MemWorkerAction,
};
pub use args::misc::{
    BridgeAction, ConfigAction, KeepAwakeAction, NotificationAction, PaneAction, PasteAction,
    ThemeAction, WebAction, WorkspaceAction,
};
pub use args::plugin::{PluginAction, PluginChannelAction};
pub use args::service::ServiceAction;
pub use args::sessions::SessionsAction;
pub use args::system::SystemAction;
pub use args::update::UpdateAction;
pub use args::usage::UsageAction;

/// Resolve pane ID: explicit arg > $CODETERM_PANE_ID env var > focused pane from server.
///
/// When running inside CodeTerm, $CODETERM_PANE_ID is set automatically by the PTY.
/// This ensures commands like `pane split` always target the caller's pane,
/// not whatever pane the user happens to have focused.
pub fn resolve_pane(client: &mut SocketClient, id: Option<String>) -> String {
    // 1. Explicit --pane/--id arg
    if let Some(id) = id {
        return id;
    }
    // 2. $CODETERM_PANE_ID env var (set by CodeTerm PTY for each shell)
    if let Ok(env_id) = std::env::var("CODETERM_PANE_ID") {
        if !env_id.is_empty() {
            return env_id;
        }
    }
    // 3. Fallback: ask server for focused pane
    match client.call("pane.list", None) {
        Ok(value) => {
            if let Some(arr) = value.as_array() {
                let pane = arr
                    .iter()
                    .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                    .or_else(|| arr.first());
                if let Some(id) = pane.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                    return id.to_string();
                }
            }
            eprintln!("Error: no panes found");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = Cli::try_parse_from(std::env::args()).unwrap_or_else(|e| e.exit());

    // `plugin` is filesystem-local (no daemon) — handle before socket/mesh.
    if matches!(cli.command, Commands::Plugin { .. }) {
        match cli.command {
            Commands::Plugin { action } => cmd::plugin::handle_plugin(cli.json, action),
            _ => unreachable!(),
        }
        return;
    }

    // --mesh TARGET: proxy entire command to a remote peer via relay
    if let Some(ref mesh_target) = cli.mesh {
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
        mesh_cli::run_via_mesh(&cli, mesh_target);
        return;
    }

    // Config command uses HTTP API directly — no socket needed
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
                    .patch(&format!("{}/api/config", base_url))
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
                    // For config get: print just the value
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

    // `mem run-worker` opens knowledge.db directly via --db-path and never talks to the daemon socket — short-circuit before discover_socket so a
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

    // `system` writes OS-level autostart triggers (schtasks / launchd / systemd). No daemon dependency — short-circuit so it works pre-install and
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
            Commands::CliManifest { write } => cmd::manifest::handle(write),
            _ => unreachable!(),
        }
        return;
    }

    // `docs` reads embedded markdown bundled at compile time — no daemon dependency. Short-circuit so agents can read references even when the daemon is wedged or restarting.
    if matches!(cli.command, Commands::Docs { .. }) {
        match cli.command {
            Commands::Docs { name, full, head } => cmd::docs::handle(name, full, head),
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

        Commands::Workspace { action } => match action {
            WorkspaceAction::List => client.call("workspace.list", None),
            WorkspaceAction::Current => {
                let result = client.call("workspace.list", None);
                match result {
                    Ok(value) => {
                        if let Some(arr) = value.as_array() {
                            let id = arr
                                .iter()
                                .find(|w| {
                                    w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false)
                                })
                                .or_else(|| arr.first())
                                .and_then(|w| w.get("id").and_then(|v| v.as_str()));
                            if let Some(id) = id {
                                println!("{}", id);
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
            WorkspaceAction::Create { name, cwd } => {
                let requested_name = name.clone();
                let mut params = serde_json::json!({ "name": name });
                if let Some(cwd) = cwd {
                    params["cwd"] = serde_json::json!(cwd);
                }
                match client.call("workspace.create", Some(params)) {
                    Ok(val) if workspace_reuse_notice(&val, &requested_name).is_some() => {
                        let _ = write_workspace_create_reuse_output(
                            &val,
                            &requested_name,
                            cli.json,
                            &mut std::io::stdout(),
                            &mut std::io::stderr(),
                        );
                        return;
                    }
                    result => result,
                }
            }
            WorkspaceAction::Focus { id } => client.call(
                "workspace.focus",
                Some(serde_json::json!({ "workspace_id": id })),
            ),
            WorkspaceAction::Rename { id, name } => client.call(
                "workspace.rename",
                Some(serde_json::json!({ "workspace_id": id, "name": name })),
            ),
            WorkspaceAction::Close { id } => client.call(
                "workspace.close",
                Some(serde_json::json!({ "workspace_id": id })),
            ),
            WorkspaceAction::MoveTab { tab_id, from, to } => client.call(
                "workspace.move_tab",
                Some(serde_json::json!({
                    "tab_id": tab_id,
                    "from_workspace_id": from,
                    "to_workspace_id": to
                })),
            ),
        },

        Commands::Pane { action } => {
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

        Commands::GetOutput { pane, lines } => {
            let pane_id = resolve_pane(&mut client, pane);
            let max_bytes = (lines as u64) * 80;
            let result = client.call(
                "pane.get_output",
                Some(serde_json::json!({ "pane_id": pane_id, "max_bytes": max_bytes })),
            );
            match result {
                Ok(value) => {
                    if let Some(output) = value.get("output").and_then(|v| v.as_str()) {
                        if cli.json {
                            println!("{}", serde_json::to_string_pretty(&value).unwrap());
                        } else {
                            print!("{}", strip_ansi(output));
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

        Commands::SendText {
            pane,
            text,
            from_file,
            stdin,
        } => {
            cmd::send_text::handle_send_text(&mut client, cli.json, pane, text, from_file, stdin);
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
                    } => http
                        .post(format!("{}/api/keepawake/start", base))
                        .json(&serde_json::json!({
                            "duration_secs": duration_secs,
                            "display": display,
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

        // Docs is handled before socket connect (embedded markdown)
        Commands::Docs { .. } => unreachable!(),

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
            cmd::mem::handle_mem(json_mode, action);
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

#[cfg(test)]
fn test_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod subcommand_contract_tests {
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
        let root =
            std::env::temp_dir().join(format!("codeterm-data-root-{}", uuid::Uuid::new_v4()));
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
            workspace_reuse_notice(&value, "Requested Workspace").as_deref(),
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
        assert!(workspace_reuse_notice(&created, "Requested Workspace").is_none());
        assert!(workspace_reuse_notice(&unmarked, "Requested Workspace").is_none());
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

        assert!(write_workspace_create_reuse_output(
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

        assert!(write_workspace_create_reuse_output(
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
}
