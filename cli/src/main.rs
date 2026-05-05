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
                    if nc.is_ascii_alphabetic() || nc == '~' || nc == 'h' || nc == 'l' { break; }
                }
            } else if chars.peek() == Some(&']') {
                // OSC sequence: ESC ] ... BEL or ST
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == '\x07' { break; }
                    if nc == '\x1b' { chars.next(); break; }
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

/// Read API config (host, port, token).
/// Priority: $CODETERM_API_PORT env var (set by CodeTerm PTY) > config.toml.
/// This ensures CLI works in both dev (7686) and prod (7685) when called from inside CodeTerm.
pub fn read_api_config() -> (String, Option<String>) {
    let config_dir = dirs::home_dir().unwrap_or_default().join(data_dir_name());
    let config_path = config_dir.join("config.toml");

    let (mut host, mut port) = ("127.0.0.1".to_string(), 7685i64);

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(val) = content.parse::<toml::Table>() {
            if let Some(api) = val.get("api").and_then(|v| v.as_table()) {
                let raw_host = api.get("host").and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
                host = if raw_host == "0.0.0.0" { "127.0.0.1".to_string() } else { raw_host.to_string() };
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

    // Highest priority: explicit CODETERM_API_TOKEN env override.
    // Useful for hitting dev (port 7686) from outside its PTY without the
    // matching mesh identity, or for tests with a manually scoped token.
    let token = std::env::var("CODETERM_API_TOKEN").ok()
        .filter(|s| !s.is_empty())
        .or(signed_token)
        .or_else(|| {
            let config_path = config_dir.join("config.toml");
            std::fs::read_to_string(&config_path).ok()
                .and_then(|content| content.parse::<toml::Table>().ok())
                .and_then(|val| val.get("api")?.as_table()?.get("token")?.as_str().map(String::from))
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
    let config_dir = dirs::home_dir()?.join(data_dir_name());
    let identity_path = config_dir.join("mesh").join("identity.json");
    let content = std::fs::read_to_string(&identity_path).ok()?;
    let id: serde_json::Value = serde_json::from_str(&content).ok()?;
    let pk = id.get("private_key")?.as_str()?;
    let iid = id.get("instance_id")?.as_str()?;
    mesh_cli::sign_request(pk, iid).ok()
}

/// Build HTTP client with auth header
pub fn build_http(token: &Option<String>) -> reqwest::blocking::Client {
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
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

mod args;
pub use args::cli::{Cli, Commands};
pub use args::sessions::SessionsAction;
pub use args::mem::{MemAction, MemSecretAction, MemWorkerAction, MemSkillAction, MemGraphAction};
pub use args::agent::AgentAction;
pub use args::cron::CronAction;
pub use args::update::UpdateAction;
pub use args::misc::{WorkspaceAction, PaneAction, BrowserAction, NotificationAction, KeepAwakeAction, ThemeAction, BridgeAction, ConfigAction};

/// Detect content that's almost certainly the result of bash command-substitution
/// eating the agent's intended text — e.g. unquoted `mesh peers` inside a
/// double-quoted argv string causes bash to execute `mesh peers` and splice the
/// resulting "command not found" stderr into the payload before send-text sees
/// any of it.
///
/// Returns Err(reason) if the text matches a known mangled-shell pattern. The
/// caller surfaces a hint pointing at --from-file / --from-stdin.
fn guard_against_shell_mangle(text: &str) -> Result<(), String> {
    // Patterns that virtually never appear in legitimate user-typed messages
    // but do appear when bash's "command not found" stderr leaks into stdin.
    const SUSPICIOUS: &[&str] = &[
        ": command not found",
        "/usr/bin/bash: line ",
        "bash: line ",
        "syntax error near unexpected token",
    ];
    for needle in SUSPICIOUS {
        if text.contains(needle) {
            return Err(format!(
                "send-text payload looks shell-mangled (matched marker: {:?})",
                needle
            ));
        }
    }
    Ok(())
}

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
                let pane = arr.iter()
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
    let cli = Cli::parse();

    // --mesh TARGET: proxy entire command to a remote peer via relay
    if let Some(ref mesh_target) = cli.mesh {
        // Secrets are local-only — refuse to proxy them across the mesh.
        // Cross-mesh secret access needs a trust model we haven't designed yet.
        if matches!(cli.command, Commands::Mem { action: MemAction::Secret { .. } }) {
            eprintln!(
                "Error: secrets do not cross the mesh. Run on the local machine \
                 (vault peer), or design a trust model first."
            );
            std::process::exit(7);
        }
        if matches!(cli.command, Commands::Snip { .. }) {
            eprintln!(
                "Error: 'codeterm snip' wraps a local child process and cannot be \
                 mesh-forwarded. Run it on the target peer directly."
            );
            std::process::exit(7);
        }
        if matches!(cli.command, Commands::Hook { .. }) {
            eprintln!(
                "Error: 'codeterm hook' is invoked by the local agent CLI and \
                 cannot be mesh-forwarded."
            );
            std::process::exit(7);
        }
        if matches!(cli.command, Commands::Map { .. }) {
            eprintln!(
                "Error: 'codeterm map' scans local files and cannot be \
                 mesh-forwarded. Run it on the target peer directly."
            );
            std::process::exit(7);
        }
        mesh_cli::run_via_mesh(&cli, mesh_target);
        return;
    }

    // Config command uses HTTP API directly — no socket needed
    let json_output = cli.json;
    if let Commands::Config { ref action } = cli.command {
        let (base_url, token) = read_api_config();
        let http = build_http(&token);
        let result = match action {
            ConfigAction::Get { key } => {
                let url = format!("{}/api/config?key={}", base_url, key);
                match http.get(&url).send() {
                    Ok(resp) if resp.status().is_success() => resp.json().map_err(|e| e.to_string()),
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
                match http.patch(&format!("{}/api/config", base_url)).json(&body).send() {
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
            Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
        }
        return;
    }

    // `mem run-worker` opens knowledge.db directly via --db-path and never
    // talks to the daemon socket — short-circuit before discover_socket so
    // a stale CODETERM_SOCKET_PATH inherited from a previous daemon PID
    // (e.g. after update --apply restarts the binary) doesn't make the
    // worker exit 1 before it can do any work.
    if let Commands::Mem { action: MemAction::RunWorker { ref kind, ref db_path } } = cli.command {
        cmd::worker::run_mem_worker(kind, db_path.as_deref());
        return;
    }

    // Hooks run as the agent's stdin→stdout filter. They never need the
    // daemon socket, and the cold-start latency budget is <50 ms — short
    // circuit before discover_socket / SocketClient::connect.
    if let Commands::Hook { action } = cli.command {
        cmd::hook::handle_hook(action);
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
                            let id = arr.iter()
                                .find(|w| w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                                .or_else(|| arr.first())
                                .and_then(|w| w.get("id").and_then(|v| v.as_str()));
                            if let Some(id) = id {
                                println!("{}", id);
                            }
                        }
                        return;
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            }
            WorkspaceAction::Create { name, cwd } => {
                let mut params = serde_json::json!({ "name": name });
                if let Some(cwd) = cwd {
                    params["cwd"] = serde_json::json!(cwd);
                }
                client.call("workspace.create", Some(params))
            }
            WorkspaceAction::Focus { id } => {
                client.call("workspace.focus", Some(serde_json::json!({ "workspace_id": id })))
            }
            WorkspaceAction::Rename { id, name } => {
                client.call("workspace.rename", Some(serde_json::json!({ "workspace_id": id, "name": name })))
            }
            WorkspaceAction::Close { id } => {
                client.call("workspace.close", Some(serde_json::json!({ "workspace_id": id })))
            }
            WorkspaceAction::MoveTab { tab_id, from, to } => {
                client.call("workspace.move_tab", Some(serde_json::json!({
                    "tab_id": tab_id,
                    "from_workspace_id": from,
                    "to_workspace_id": to
                })))
            }
        },

        Commands::Browser { action } => match action {
            BrowserAction::Open { url } => {
                client.call("browser.open", Some(serde_json::json!({ "url": url })))
            }
        },

        Commands::Pane { action } => {
            cmd::pane::handle_pane(&mut client, cli.json, action);
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

        Commands::SendText { pane, text, from_file, from_stdin } => {
            let (resolved_text, source) = if from_stdin {
                use std::io::Read;
                let mut buf = String::new();
                if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                    eprintln!("Error: failed to read --from-stdin: {}", e);
                    std::process::exit(1);
                }
                (buf, "--from-stdin")
            } else if let Some(path) = from_file {
                match std::fs::read_to_string(&path) {
                    Ok(s) => (s, "--from-file"),
                    Err(e) => {
                        eprintln!("Error: failed to read --from-file {}: {}", path, e);
                        std::process::exit(1);
                    }
                }
            } else {
                (text, "positional")
            };
            // Reject empty payloads loudly. Silent zero-byte sends were the
            // bug behind several "mesh sent but recipient saw empty" reports
            // (shell heredoc dropped, --from-file pointed at empty file,
            // positional swallowed by command substitution that returned empty).
            if resolved_text.trim().is_empty() {
                eprintln!("Error: send-text payload is empty (source: {})", source);
                eprintln!("hint: if using --from-stdin, ensure the heredoc actually delivered content;");
                eprintln!("      if --from-file, verify the path exists and isn't 0 bytes;");
                eprintln!("      if positional, your shell may have eaten the content.");
                std::process::exit(2);
            }
            if let Err(msg) = guard_against_shell_mangle(&resolved_text) {
                eprintln!("Error: {}", msg);
                eprintln!("hint: your shell expanded a command-substitution before codeterm-CLI received it.");
                eprintln!("      Use --from-file <path> or --from-stdin <<'EOF' ... EOF for content with backticks/$/code.");
                std::process::exit(2);
            }
            let pane_id = resolve_pane(&mut client, pane);
            client.call(
                "pane.send_text",
                Some(serde_json::json!({ "pane_id": pane_id, "text": resolved_text })),
            )
        }

        Commands::Notify { title, body, level } => {
            let mut params = serde_json::json!({ "title": title, "level": level });
            if let Some(b) = body {
                params["body"] = serde_json::Value::String(b);
            }
            client.call("notification.create", Some(params))
        }

        Commands::Bridge { action } => match action {
            BridgeAction::Restart => {
                client.call("bridge.restart", None)
            }
            BridgeAction::Send { text, service, to, append, r#type, file, caption, filename, buttons } => {
                let mut params = serde_json::json!({
                    "service": service,
                    "chat_id": to,
                    "type": r#type,
                    "append": append,
                });
                if let Some(t) = text {
                    params["text"] = serde_json::Value::String(t);
                }
                if let Some(f) = file {
                    params["file"] = serde_json::Value::String(f);
                }
                if let Some(c) = caption {
                    params["caption"] = serde_json::Value::String(c);
                }
                if let Some(n) = filename {
                    params["filename"] = serde_json::Value::String(n);
                }
                if let Some(b) = buttons {
                    // Parse buttons JSON
                    match serde_json::from_str::<serde_json::Value>(&b) {
                        Ok(v) => { params["buttons"] = v; }
                        Err(e) => {
                            eprintln!("Invalid buttons JSON: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                client.call("bridge.send", Some(params))
            }
            BridgeAction::React { to, message_id, emoji } => {
                client.call("bridge.react", Some(serde_json::json!({
                    "chat_id": to,
                    "message_id": message_id,
                    "emoji": emoji,
                })))
            }
        },

        Commands::Notifications { action, unread } => match action {
            Some(NotificationAction::Clear) => client.call("notification.clear", None),
            None => client.call(
                "notification.list",
                Some(serde_json::json!({ "unread_only": unread })),
            ),
        },

        Commands::Keepawake { action } => {
            let (base, token) = read_api_config();
            let http = build_http(&token);
            let result: Result<serde_json::Value, String> = match action.unwrap_or(KeepAwakeAction::Status) {
                KeepAwakeAction::Status => http
                    .get(format!("{}/api/keepawake", base))
                    .send()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.json::<serde_json::Value>().map_err(|e| e.to_string())),
                KeepAwakeAction::Start { duration_secs, display } => http
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
                ThemeAction::List => {
                    match http.get(format!("{}/api/themes", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if let Some(themes) = val.get("themes").and_then(|v| v.as_array()) {
                                for t in themes {
                                    println!("{}", t.as_str().unwrap_or("?"));
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                ThemeAction::Apply { name } => {
                    match http.post(format!("{}/api/themes/apply", base))
                        .json(&serde_json::json!({ "name": name }))
                        .send()
                    {
                        Ok(_) => { println!("Theme applied: {}", name); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                ThemeAction::Save { id, file, apply } => {
                    let toml = match std::fs::read_to_string(&file) {
                        Ok(c) => c,
                        Err(e) => { eprintln!("Error reading {}: {}", file, e); std::process::exit(1); }
                    };
                    match http.post(format!("{}/api/themes", base))
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
                                match http.post(format!("{}/api/themes/apply", base))
                                    .json(&serde_json::json!({ "name": id }))
                                    .send()
                                {
                                    Ok(_) => println!("Theme applied: {}", id),
                                    Err(e) => eprintln!("Theme saved but failed to apply: {}", e),
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                ThemeAction::Delete { id } => {
                    match http.delete(format!("{}/api/themes/{}", base, id)).send() {
                        Ok(_) => { println!("Theme deleted: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
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

        Commands::Update { action, check, apply, channel, manifest_url } => {
            cmd::update::handle_update(cli.json, action, check, apply, channel, manifest_url);
            return;
        }

        Commands::SelfUpdate { check, apply, rollback, channel } => {
            cmd::update::handle_self_update(cli.json, check, apply, rollback, channel);
            return;
        }

        // Config is handled before socket connect (uses HTTP API)
        Commands::Config { .. } => unreachable!(),


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
            let http = build_http(&token);
            cmd::sessions::handle_sessions(cli.json, action, &http, &base_url, token.as_deref());
            return;
        }
        Commands::Snip { cmd: cmd_argv } => cmd::snip::handle_snip(cmd_argv),
        Commands::Hook { .. } => unreachable!(
            "hook short-circuits earlier in main.rs before discover_socket"
        ),
        Commands::Map { args } => {
            cmd::map::handle_map(args);
            return;
        }
    };

    handle_result(cli.json, result);
}




#[cfg(test)]
mod send_text_guard_tests {
    use super::guard_against_shell_mangle;

    #[test]
    fn legitimate_text_passes() {
        assert!(guard_against_shell_mangle("hello world").is_ok());
        assert!(guard_against_shell_mangle("PR #28 — looks fine").is_ok());
        assert!(guard_against_shell_mangle("status code: 200").is_ok());
    }

    #[test]
    fn bash_command_not_found_rejected() {
        let payload = "/usr/bin/bash: line 3: mesh: command not found\nactual content";
        assert!(guard_against_shell_mangle(payload).is_err());
    }

    #[test]
    fn bare_command_not_found_rejected() {
        let payload = "mesh: command not found\nfollowed by intended message";
        assert!(guard_against_shell_mangle(payload).is_err());
    }

    #[test]
    fn syntax_error_rejected() {
        let payload = "bash: -c: line 1: syntax error near unexpected token `('";
        assert!(guard_against_shell_mangle(payload).is_err());
    }
}

#[cfg(test)]
mod subcommand_contract_tests {
    use super::*;
    use clap::CommandFactory;

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
    fn mem_run_worker_accepts_all_three_kinds() {
        for kind in ["dreaming-light", "dreaming-deep", "librarian-once"] {
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
}
