mod client;
mod cmd;
mod mesh_cli;

use cmd::print::{handle_result, print_history_timeline, print_human, print_object, print_self_update_outcome, print_taxonomy};

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
fn resolve_pane(client: &mut SocketClient, id: Option<String>) -> String {
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

        Commands::Pane { action } => match action {
            PaneAction::List => client.call("pane.list", None),
            PaneAction::Current { last } => {
                let result = client.call("pane.list", None);
                match result {
                    Ok(value) => {
                        if let Some(arr) = value.as_array() {
                            let pane = if last {
                                arr.last()
                            } else {
                                arr.iter()
                                    .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                                    .or_else(|| arr.first())
                            };
                            if let Some(id) = pane.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                                println!("{}", id);
                            }
                        }
                        return;
                    }
                    Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
            }
            PaneAction::Focus { id } => {
                client.call("pane.focus", Some(serde_json::json!({ "pane_id": id })))
            }
            PaneAction::Close { id } => {
                client.call("pane.close", Some(serde_json::json!({ "pane_id": id })))
            }
            PaneAction::Rename { pane, title } => {
                let pane_id = resolve_pane(&mut client, pane);
                client.call(
                    "pane.rename",
                    Some(serde_json::json!({ "pane_id": pane_id, "title": title })),
                )
            }
            PaneAction::GetText { pane, formatted } => {
                let pane_id = resolve_pane(&mut client, pane);
                let (api_url, api_token) = read_api_config();
                let http = build_http(&api_token);
                let url = format!("{}/api/panes/{}/text?formatted={}", api_url, pane_id, formatted);
                match http.get(url).send() {
                    Ok(resp) => {
                        if let Ok(val) = resp.json::<serde_json::Value>() {
                            if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                                if formatted {
                                    print!("{}", text);
                                } else {
                                    for line in text.lines() {
                                        if !line.trim().is_empty() {
                                            println!("{}", line);
                                        }
                                    }
                                }
                                return;
                            }
                        }
                        eprintln!("No pane text");
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Get-text request failed: {}", e); std::process::exit(1); }
                }
            }
            PaneAction::Exec { pane, command, timeout } => {
                let pane_id = resolve_pane(&mut client, pane);
                let (api_url, api_token) = read_api_config();
                let http = build_http(&api_token);
                match http.post(format!("{}/api/panes/{}/exec", api_url, pane_id))
                    .json(&serde_json::json!({
                        "command": command,
                        "timeout_ms": timeout,
                        "strip_ansi": true,
                    }))
                    .send()
                {
                    Ok(resp) => {
                        if let Ok(val) = resp.json::<serde_json::Value>() {
                            if let Some(output) = val.get("output").and_then(|v| v.as_str()) {
                                print!("{}", output);
                                return;
                            }
                        }
                        eprintln!("Exec failed");
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Exec request failed: {}", e); std::process::exit(1); }
                }
            }
            PaneAction::Status { pane } => {
                let pane_id = resolve_pane(&mut client, pane);
                let (api_url, api_token) = read_api_config();
                let http = build_http(&api_token);
                match http.get(format!("{}/api/panes", api_url)).send() {
                    Ok(resp) => {
                        if let Ok(panes) = resp.json::<Vec<serde_json::Value>>() {
                            if let Some(p) = panes.iter().find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&pane_id)) {
                                let at = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
                                let sid = p.get("agent_session_id").and_then(|v| v.as_str()).unwrap_or("-");
                                let pp = if p.get("pending_prompt").is_some() { "yes" } else { "no" };
                                let cv = p.get("chat_view").and_then(|v| v.as_bool()).unwrap_or(false);
                                println!("pane:    {}", pane_id);
                                println!("agent:   {}", at);
                                println!("session: {}", sid);
                                println!("prompt:  {}", pp);
                                println!("chat:    {}", cv);
                                return;
                            }
                        }
                        eprintln!("Pane not found");
                        std::process::exit(1);
                    }
                    Err(e) => { eprintln!("Status request failed: {}", e); std::process::exit(1); }
                }
            }
        },

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
            let (base, token) = read_api_config();
            let http = build_http(&token);
            match action {
                AgentAction::List => {
                    match http.get(format!("{}/api/agents", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No agents found."); }
                                for a in arr { print_object(a); println!(); }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Providers => {
                    match http.get(format!("{}/api/agents/providers", base)).send() {
                        Ok(resp) => {
                            let status = resp.status();
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                                return;
                            }
                            let Some(arr) = val.as_array() else {
                                if status == 404 {
                                    eprintln!(
                                        "Server doesn't know /api/agents/providers — update the running CodeTerm instance."
                                    );
                                } else {
                                    eprintln!("Unexpected response shape (HTTP {})", status);
                                }
                                std::process::exit(1);
                            };
                            if arr.is_empty() {
                                println!("No agent providers registered.");
                                return;
                            }
                            for p in arr {
                                let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                                let name = p.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                                let default_model = p.get("default_model").and_then(|v| v.as_str()).unwrap_or("-");
                                let models_count = p
                                    .get("available_models")
                                    .and_then(|v| v.as_array())
                                    .map(|a| a.len())
                                    .unwrap_or(0);
                                println!("{:<12} {:<20} default={}  models={}", id, name, default_model, models_count);
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Models { provider } => {
                    match http.get(format!("{}/api/agents/providers", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            let Some(arr) = val.as_array() else {
                                eprintln!("Unexpected response shape");
                                std::process::exit(1);
                            };
                            let target = arr.iter().find(|p| {
                                p.get("id").and_then(|v| v.as_str()) == Some(provider.as_str())
                            });
                            let Some(target) = target else {
                                let known: Vec<&str> = arr.iter()
                                    .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
                                    .collect();
                                eprintln!(
                                    "Unknown provider: '{}'. Known: [{}]",
                                    provider,
                                    known.join(", ")
                                );
                                std::process::exit(1);
                            };
                            let models = target.get("available_models").and_then(|v| v.as_array());
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&models).unwrap());
                                return;
                            }
                            let Some(models) = models else {
                                println!("No models advertised for provider '{}'.", provider);
                                return;
                            };
                            let default_model = target.get("default_model").and_then(|v| v.as_str());
                            for m in models {
                                let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                                let name = m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                                let marker = if default_model == Some(id) { " (default)" } else { "" };
                                let description = m.get("description").and_then(|v| v.as_str()).unwrap_or("");
                                if description.is_empty() {
                                    println!("{:<10} {}{}", id, name, marker);
                                } else {
                                    println!("{:<10} {}{}\n             {}", id, name, marker, description);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Hierarchy => {
                    match http.get(format!("{}/api/agents/hierarchy", base)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                fn print_tree(node: &serde_json::Value, indent: usize) {
                                    let prefix = "  ".repeat(indent);
                                    let role = node.get("role").and_then(|v| v.as_str()).unwrap_or("?");
                                    let title = node.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                                    let status = node.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                                    let icon = match role {
                                        "Orchestrator" => "★",
                                        "Lead" => "·",
                                        "Worker" => "⚙",
                                        _ => "·",
                                    };
                                    println!("{}{} {} [{}] ({})", prefix, icon, title, status, role);
                                    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
                                        for c in children { print_tree(c, indent + 1); }
                                    }
                                }
                                if let Some(arr) = val.as_array() {
                                    if arr.is_empty() { println!("No hierarchy found."); }
                                    for c in arr { print_tree(c, 0); }
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::SetConductor { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.post(format!("{}/api/panes/{}/role", base, pane_id))
                        .json(&serde_json::json!({"role": "Orchestrator"}))
                        .send()
                    {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Link { parent, pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    // Determine role based on parent's role
                    let parent_resp = http.get(format!("{}/api/panes/{}/status", base, parent)).send();
                    let role = match parent_resp {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            match val.get("role").and_then(|v| v.as_str()) {
                                Some("Orchestrator") | Some("Conductor") => "Worker",
                                Some("Lead") | Some("Worker") => "Worker",
                                _ => { eprintln!("Parent must be Orchestrator or Lead"); std::process::exit(1); }
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    };
                    match http.post(format!("{}/api/panes/{}/role", base, pane_id))
                        .json(&serde_json::json!({"role": role, "parent_pane_id": parent}))
                        .send()
                    {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Unlink { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.delete(format!("{}/api/panes/{}/role", base, pane_id)).send() {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Children { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.get(format!("{}/api/panes/{}/children", base, pane_id)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No children."); }
                                for c in arr { print_object(c); println!(); }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::AllowSpawn { pane, enable } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.post(format!("{}/api/panes/{}/can-spawn", base, pane_id))
                        .json(&serde_json::json!({"can_spawn": enable}))
                        .send()
                    {
                        Ok(resp) => { let v: serde_json::Value = resp.json().unwrap_or_default(); println!("{}", serde_json::to_string_pretty(&v).unwrap()); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Report { message, pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    // Use socket (not REST) for reliability
                    client.call("agent.report", Some(serde_json::json!({
                        "pane_id": pane_id,
                        "message": message,
                    })))
                }
                AgentAction::Reports { pane, clear, limit } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    // Use socket (not REST) for reliability
                    match client.call("agent.reports", Some(serde_json::json!({
                        "pane_id": pane_id,
                        "clear": clear,
                    }))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No reports."); }
                                let take = limit.unwrap_or(arr.len());
                                for r in arr.iter().rev().take(take) {
                                    let from = r.get("from_title").and_then(|v| v.as_str()).unwrap_or("?");
                                    let msg = r.get("message").and_then(|v| v.as_str()).unwrap_or("");
                                    println!("[{}] {}", from, msg);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Status { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    match http.get(format!("{}/api/panes/{}/status", base, pane_id)).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                print_object(&val);
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Spawn { agent, title, cwd, role, parent, can_spawn, task, args, workspace } => {
                    let mut body = serde_json::json!({
                        "agent": agent,
                    });
                    if let Some(t) = title { body["title"] = serde_json::json!(t); }
                    if let Some(c) = cwd { body["cwd"] = serde_json::json!(c); }
                    if let Some(r) = role { body["role"] = serde_json::json!(r); }
                    if let Some(p) = parent { body["parent_pane_id"] = serde_json::json!(p); }
                    if can_spawn { body["can_spawn"] = serde_json::json!(true); }
                    if let Some(t) = task { body["task"] = serde_json::json!(t); }
                    if let Some(a) = args { body["args"] = serde_json::json!(a); }
                    if let Some(w) = workspace { body["workspace_id"] = serde_json::json!(w); }
                    // Always record the spawning agent's pane (when this CLI runs
                    // inside a CodeTerm PTY). Independent of --parent, so a
                    // top-level Orchestrator (no parent in the role hierarchy)
                    // still has a route back for `agent report`.
                    if let Ok(spawner) = std::env::var("CODETERM_PANE_ID") {
                        if !spawner.is_empty() {
                            body["spawned_by_pane_id"] = serde_json::json!(spawner);
                        }
                    }
                    // Use socket API (not REST) — avoids tokio runtime contention
                    match client.call("agent.spawn", Some(body)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                if let Some(pid) = val.get("pane_id").and_then(|v| v.as_str()) {
                                    println!("Spawned: {} (pane {})", val.get("title").and_then(|v| v.as_str()).unwrap_or("?"), pid);
                                } else if let Some(err) = val.get("error").or_else(|| val.get("message")) {
                                    eprintln!("Error: {}", err);
                                } else {
                                    print_object(&val);
                                }
                                if let Some(warning) = val.get("warning").and_then(|v| v.as_str()) {
                                    eprintln!("Warning: {}", warning);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Compact { pane } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    let cmd = "/compact\n";
                    match http.post(format!("{}/api/panes/{}/send", base, pane_id))
                        .json(&serde_json::json!({"text": cmd}))
                        .send()
                    {
                        Ok(_) => { println!("Sent /compact to {}", pane_id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Restart { pane, force, grace_secs } => {
                    let pane_id = resolve_pane(&mut client, pane);
                    let body = serde_json::json!({"force": force, "grace_secs": grace_secs});
                    match http.post(format!("{}/api/panes/{}/agent/restart", base, pane_id))
                        .json(&body)
                        .send()
                    {
                        Ok(resp) => {
                            let status = resp.status();
                            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                                let retry = resp.headers()
                                    .get("retry-after")
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or("30");
                                eprintln!("Error: rate limited (retry after {}s)", retry);
                                std::process::exit(1);
                            }
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if !status.is_success() {
                                let msg = val.get("error").and_then(|v| v.as_str())
                                    .unwrap_or_else(|| val.as_str().unwrap_or("restart failed"));
                                eprintln!("Error: {}", msg);
                                std::process::exit(1);
                            }
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Restarting pane {}...", pane_id);
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                AgentAction::Model { name, pane } => {
                    let valid = ["sonnet", "haiku", "opus"];
                    if !valid.contains(&name.as_str()) {
                        eprintln!("Invalid model: {}. Use: sonnet, haiku, opus", name);
                        std::process::exit(1);
                    }
                    let pane_id = resolve_pane(&mut client, pane);
                    let cmd = format!("/model {}\n", name);
                    match http.post(format!("{}/api/panes/{}/send", base, pane_id))
                        .json(&serde_json::json!({"text": cmd}))
                        .send()
                    {
                        Ok(_) => { println!("Sent /model {} to {}", name, pane_id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
            }
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
            // Cron uses socket API
            match action {
                CronAction::List { workspace } => {
                    let mut params = serde_json::json!({});
                    if let Some(ws) = workspace { params["workspace_id"] = serde_json::json!(ws); }
                    match client.call("cron.list", Some(params)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No cron jobs."); }
                                for j in arr {
                                    let enabled = j.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let oneshot = j.get("oneshot").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let status = match (enabled, oneshot) {
                                        (_, true) => "oneshot",
                                        (true, false) => "enabled",
                                        (false, false) => "disabled",
                                    };
                                    println!("{} {} [{}] schedule={} agent={}",
                                        j.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                        j.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                        status,
                                        j.get("schedule").and_then(|v| v.as_str()).unwrap_or("?"),
                                        j.get("agent").and_then(|v| v.as_str()).unwrap_or("?"),
                                    );
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Create { name, schedule, task, workspace, agent, model, cwd, description, creator_pane, once } => {
                    let ws_id = workspace.unwrap_or_else(|| {
                        // Prefer CODETERM_WORKSPACE_ID (agent's own workspace), fall back to first
                        std::env::var("CODETERM_WORKSPACE_ID").ok()
                            .unwrap_or_else(|| {
                                client.call("workspace.list", None)
                                    .ok()
                                    .and_then(|v| v.as_array().and_then(|a| a.first().cloned()))
                                    .and_then(|w| w.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                                    .unwrap_or_default()
                            })
                    });
                    let mut params = serde_json::json!({
                        "name": name,
                        "schedule": schedule,
                        "task": task,
                        "workspace_id": ws_id,
                        "agent": agent,
                    });
                    if let Some(m) = model { params["model"] = serde_json::json!(m); }
                    if let Some(c) = cwd { params["cwd"] = serde_json::json!(c); }
                    if let Some(d) = description { params["description"] = serde_json::json!(d); }
                    if let Some(cp) = creator_pane { params["creator_pane_id"] = serde_json::json!(cp); }
                    if once { params["oneshot"] = serde_json::json!(true); }
                    match client.call("cron.create", Some(params)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Created: {} ({})",
                                    val.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                    val.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                );
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Edit { id, name, schedule, task, cwd, description, agent, model, jitter_max_ms } => {
                    let mut params = serde_json::json!({"id": id});
                    if let Some(n) = name { params["name"] = serde_json::json!(n); }
                    if let Some(s) = schedule { params["schedule"] = serde_json::json!(s); }
                    if let Some(t) = task { params["task"] = serde_json::json!(t); }
                    if let Some(c) = cwd { params["cwd"] = serde_json::json!(c); }
                    if let Some(d) = description { params["description"] = serde_json::json!(d); }
                    if let Some(a) = agent { params["agent"] = serde_json::json!(a); }
                    if let Some(m) = model { params["model"] = serde_json::json!(m); }
                    if let Some(j) = jitter_max_ms { params["jitter_max_ms"] = serde_json::json!(j); }
                    match client.call("cron.update", Some(params)) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Updated: {}", val.get("name").and_then(|v| v.as_str()).unwrap_or("?"));
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Delete { id } => {
                    match client.call("cron.delete", Some(serde_json::json!({"id": id}))) {
                        Ok(_) => { println!("Deleted: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Enable { id } => {
                    match client.call("cron.enable", Some(serde_json::json!({"id": id}))) {
                        Ok(_) => { println!("Enabled: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Disable { id } => {
                    match client.call("cron.disable", Some(serde_json::json!({"id": id}))) {
                        Ok(_) => { println!("Disabled: {}", id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Trigger { id } => {
                    match client.call("cron.trigger", Some(serde_json::json!({"id": id}))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Triggered: pane={}",
                                    val.get("pane_id").and_then(|v| v.as_str()).unwrap_or("?"));
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Cancel { id } => {
                    match client.call("cron.cancel", Some(serde_json::json!({"id": id}))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let n = val.get("cancelled").and_then(|v| v.as_u64()).unwrap_or(0);
                                if n == 0 {
                                    println!("No running executions for {}.", id);
                                } else {
                                    println!("Cancelled {} running execution(s) of {}.", n, id);
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::History { id } => {
                    match client.call("cron.history", Some(serde_json::json!({"id": id}))) {
                        Ok(val) => {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() { println!("No runs yet."); }
                                for r in arr {
                                    println!("{} {} started={} pane={}",
                                        r.get("run_id").and_then(|v| v.as_str()).unwrap_or("?"),
                                        r.get("status").and_then(|v| v.as_str()).unwrap_or("?"),
                                        r.get("started_at").and_then(|v| v.as_u64()).unwrap_or(0),
                                        r.get("pane_id").and_then(|v| v.as_str()).unwrap_or("-"),
                                    );
                                }
                            }
                            return;
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
                CronAction::Complete { run_id, summary } => {
                    let mut params = serde_json::json!({"run_id": run_id});
                    if let Some(s) = summary { params["summary"] = serde_json::json!(s); }
                    match client.call("cron.complete", Some(params)) {
                        Ok(_) => { println!("Completed: {}", run_id); return; }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                }
            }
        }

        Commands::Update { action, check, apply, channel, manifest_url } => {
            let (base_url, token) = read_api_config();
            let http = build_http(&token);

            let resolved = if let Some(ch) = channel {
                Some(UpdateAction::Channel {
                    channel: if ch.is_empty() { None } else { Some(ch) },
                })
            } else if apply {
                Some(UpdateAction::Install {
                    auto: true,
                    manifest_url: manifest_url.clone(),
                    from_file: None,
                    continue_prompt: None,
                    continue_pane: None,
                })
            } else if check {
                Some(UpdateAction::Check { force: false })
            } else {
                action
            };

            match resolved.unwrap_or(UpdateAction::Check { force: false }) {
                UpdateAction::Check { force } => {
                    let url = if force {
                        format!("{}/api/update/check?force=true", base_url)
                    } else {
                        format!("{}/api/update/check", base_url)
                    };
                    match http.get(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let body: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                            } else {
                                let current = body["current_version"].as_str().unwrap_or("?");
                                let latest = body["latest_version"].as_str().unwrap_or("?");
                                let method = body["install_method"].as_str().unwrap_or("unknown");
                                let available = body["update_available"].as_bool().unwrap_or(false);

                                println!("Current: v{}  Latest: v{}  Install: {}", current, latest, method);
                                if available {
                                    println!("Update available!");
                                    if let Some(cmd) = body["update_command"].as_str() {
                                        println!("Run: {}", cmd);
                                    } else if let Some(url) = body["download_url"].as_str() {
                                        println!("Download: {}", url);
                                    }
                                } else {
                                    println!("Up to date.");
                                }
                            }
                        }
                        Ok(resp) => {
                            eprintln!("Error: {}", resp.status());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
                UpdateAction::Install { auto, manifest_url, from_file, continue_prompt, continue_pane } => {
                    if let Some(path) = from_file {
                        let pane = continue_pane
                            .or_else(|| std::env::var("CODETERM_PANE_ID").ok());
                        cmd::install::run_install_from_file(&path, cli.json, continue_prompt.as_deref(), pane.as_deref());
                        return;
                    }
                    let mut params: Vec<(&str, String)> = Vec::new();
                    if auto {
                        params.push(("method", "auto".into()));
                    }
                    if let Some(m) = manifest_url {
                        params.push(("manifest_url", m));
                    }
                    let qs = if params.is_empty() {
                        String::new()
                    } else {
                        let pairs: Vec<String> = params
                            .iter()
                            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                            .collect();
                        format!("?{}", pairs.join("&"))
                    };
                    let url = format!("{}/api/update/install{}", base_url, qs);
                    match http.post(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let body: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                            } else {
                                let status = body["status"].as_str().unwrap_or("unknown");
                                match status {
                                    "up_to_date" => println!("Already up to date."),
                                    "command" => {
                                        if let Some(cmd) = body["command"].as_str() {
                                            println!("{}", cmd);
                                        }
                                    }
                                    "browser_opened" => {
                                        println!("Opened download page in browser.");
                                    }
                                    "updating" => {
                                        if let Some(msg) = body["message"].as_str() {
                                            println!("{}", msg);
                                        }
                                    }
                                    _ => println!("{}", body),
                                }
                            }
                        }
                        Ok(resp) => {
                            eprintln!("Error: {}", resp.status());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
                UpdateAction::Channel { channel } => {
                    let url = format!("{}/api/update/channel", base_url);
                    let result = match channel {
                        None => http.get(&url).send(),
                        Some(name) => http
                            .post(&url)
                            .json(&serde_json::json!({ "channel": name }))
                            .send(),
                    };
                    match result {
                        Ok(resp) if resp.status().is_success() => {
                            let body: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                            } else if let Some(c) = body["channel"].as_str() {
                                println!("Channel: {}", c);
                            } else {
                                println!("{}", body);
                            }
                        }
                        Ok(resp) => {
                            eprintln!("Error: {}", resp.status());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
            }
        }

        Commands::SelfUpdate { check, apply, rollback, channel } => {
            let (base_url, token) = read_api_config();
            let http = build_http(&token);

            if check {
                let url = format!("{}/api/update/check", base_url);
                match http.get(&url).send() {
                    Ok(resp) if resp.status().is_success() => {
                        let body: serde_json::Value = resp.json().unwrap_or_default();
                        if cli.json {
                            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                        } else {
                            let current = body["current_version"].as_str().unwrap_or("?");
                            let latest = body["latest_version"].as_str().unwrap_or("?");
                            let available = body["update_available"].as_bool().unwrap_or(false);
                            println!("Current: v{}  Latest: v{}", current, latest);
                            println!("{}", if available { "Update available." } else { "Up to date." });
                        }
                    }
                    Ok(resp) => { eprintln!("Error: {}", resp.status()); std::process::exit(1); }
                    Err(e)   => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
                return;
            }

            if rollback {
                let url = format!("{}/api/update/rollback", base_url);
                match http.post(&url).send() {
                    Ok(resp) if resp.status().is_success() => {
                        let body: serde_json::Value = resp.json().unwrap_or_default();
                        if cli.json {
                            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                        } else {
                            print_self_update_outcome(&body);
                        }
                    }
                    Ok(resp) => { eprintln!("Error: {}", resp.status()); std::process::exit(1); }
                    Err(e)   => { eprintln!("Error: {}", e); std::process::exit(1); }
                }
                return;
            }

            // Default: apply (also when --apply explicitly passed)
            let _ = apply;
            let mut params: Vec<(&str, String)> = Vec::new();
            if let Some(c) = channel { params.push(("channel", c)); }
            let qs = if params.is_empty() {
                String::new()
            } else {
                let pairs: Vec<String> = params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                    .collect();
                format!("?{}", pairs.join("&"))
            };
            let url = format!("{}/api/update/self_apply{}", base_url, qs);
            match http.post(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().unwrap_or_default();
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
                    } else {
                        print_self_update_outcome(&body);
                    }
                }
                Ok(resp) => { eprintln!("Error: {}", resp.status()); std::process::exit(1); }
                Err(e)   => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
            return;
        }

        // Config is handled before socket connect (uses HTTP API)
        Commands::Config { .. } => unreachable!(),


        Commands::Mesh { action } => {
            let (base, token) = read_api_config();
            let http = build_http(&token);
            match action {
                MeshAction::Status => {
                    match http.get(format!("{}/api/mesh/status", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let name = val.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                let id = val.get("instance_id").and_then(|v| v.as_str()).unwrap_or("?");
                                let paired = val.get("paired_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                let discovered = val.get("discovered_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("{} ({})", name, &id[..id.len().min(8)]);
                                println!("Paired: {}, Discovered: {}", paired, discovered);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Peers => {
                    match http.get(format!("{}/api/mesh/peers", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                if let Some(paired) = val.get("paired").and_then(|v| v.as_array()) {
                                    if !paired.is_empty() { println!("Paired:"); }
                                    for p in paired {
                                        let name = p.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                        let online = p.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
                                        let ip = p.get("addresses").and_then(|v| v.as_array())
                                            .and_then(|a| a.first())
                                            .and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("?");
                                        let dot = if online { "●" } else { "○" };
                                        println!("  {} {} ({}) {}", dot, name, ip,
                                            if p.get("is_primary").and_then(|v| v.as_bool()).unwrap_or(false) { "★" } else { "" });
                                    }
                                }
                                if let Some(discovered) = val.get("discovered").and_then(|v| v.as_array()) {
                                    if !discovered.is_empty() { println!("Discovered:"); }
                                    for d in discovered {
                                        let name = d.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                        let method = d.get("discovery_method").and_then(|v| v.as_str()).unwrap_or("?");
                                        let ip = d.get("addresses").and_then(|v| v.as_array())
                                            .and_then(|a| a.first())
                                            .and_then(|a| a.get("ip")).and_then(|v| v.as_str()).unwrap_or("?");
                                        let dev = d.get("dev").and_then(|v| v.as_bool()).unwrap_or(false);
                                        println!("  ◇ {} ({}) [{}]{}", name, ip, method, if dev { " [dev]" } else { "" });
                                    }
                                }
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Exec { target, command, pane } => {
                    let (action, target, command, pane) = ("exec", target, command, pane);
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, action, &command, &pane, cli.json);
                    return;
                }
                MeshAction::Send { target, text, pane } => {
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "send", &text, &pane, cli.json);
                    return;
                }
                MeshAction::Output { target, pane } => {
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "output", "", &pane, cli.json);
                    return;
                }
                MeshAction::Panes { target } => {
                    mesh_cli::mesh_relay_cmd(&http, &base, &token, &target, "panes", "", "", cli.json);
                    return;
                }
                MeshAction::Connect { instance_id } => {
                    let body = serde_json::json!({"instance_id": instance_id});
                    match http.post(format!("{}/api/mesh/connect", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Pairing request sent to {}", instance_id);
                                if let Some(rid) = val.get("request_id").and_then(|v| v.as_str()) {
                                    println!("Request ID: {}", rid);
                                }
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Approve { request_id } => {
                    let body = serde_json::json!({"request_id": request_id});
                    match http.post(format!("{}/api/mesh/approve", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Pairing request {} approved", request_id);
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Reject { request_id } => {
                    let body = serde_json::json!({"request_id": request_id});
                    match http.post(format!("{}/api/mesh/reject", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Pairing request {} rejected and blocked", request_id);
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Disconnect { instance_id } => {
                    let body = serde_json::json!({"instance_id": instance_id});
                    match http.post(format!("{}/api/mesh/disconnect", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .json(&body).send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if val.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                println!("Disconnected from {}", instance_id);
                            } else {
                                let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                                eprintln!("Error: {}", err);
                                std::process::exit(1);
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Discover => {
                    match http.post(format!("{}/api/mesh/discover", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(arr) = val.as_array() {
                                if arr.is_empty() {
                                    println!("No discovered peers");
                                } else {
                                    for p in arr {
                                        let name = p.get("instance_name").and_then(|v| v.as_str()).unwrap_or("?");
                                        let id = p.get("instance_id").and_then(|v| v.as_str()).unwrap_or("?");
                                        let method = p.get("discovery_method").and_then(|v| v.as_str()).unwrap_or("?");
                                        println!("  {} ({}) [{}]", name, id, method);
                                    }
                                }
                            } else {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Broadcast => {
                    match http.post(format!("{}/api/mesh/broadcast/start", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else if let Some(dur) = val.get("duration_secs").and_then(|v| v.as_u64()) {
                                println!("Broadcasting for {}s", dur);
                            } else {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::StopBroadcast => {
                    match http.post(format!("{}/api/mesh/broadcast/stop", base))
                        .header("Authorization", format!("Bearer {}", token.as_deref().unwrap_or("")))
                        .send() {
                        Ok(resp) => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                println!("Broadcast stopped");
                            }
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Refresh => {
                    match http.post(format!("{}/api/mesh/refresh", base)).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let ts = val.get("tailscale_ip").and_then(|v| v.as_str()).unwrap_or("(none)");
                                let lan = val.get("local_ip").and_then(|v| v.as_str()).unwrap_or("(none)");
                                println!("Local addresses re-detected:");
                                println!("  tailscale: {}", ts);
                                println!("  lan:       {}", lan);
                                if let Some(rep) = val.get("announce") {
                                    let sent = rep.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
                                    let ok = rep.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
                                    println!("Announced to {}/{} paired peers", ok, sent);
                                    if let Some(stale) = rep.get("stale_peers").and_then(|v| v.as_array()) {
                                        if !stale.is_empty() {
                                            println!("Stale peers (need manual update — too old to recognize announce):");
                                            for s in stale { println!("  - {}", s.as_str().unwrap_or("?")); }
                                        }
                                    }
                                    if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                                        if !errs.is_empty() {
                                            println!("Errors:");
                                            for e in errs { println!("  - {}", e.as_str().unwrap_or("?")); }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().unwrap_or_default();
                            eprintln!("Error ({}): {}", status, body);
                            std::process::exit(1);
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Announce { to } => {
                    let url = match to {
                        Some(t) => format!("{}/api/mesh/announce/all?to={}", base, urlencoding::encode(&t)),
                        None => format!("{}/api/mesh/announce/all", base),
                    };
                    match http.post(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            let val: serde_json::Value = resp.json().unwrap_or_default();
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&val).unwrap());
                            } else {
                                let rep = val.get("report").cloned().unwrap_or(serde_json::Value::Null);
                                let sent = rep.get("sent").and_then(|v| v.as_u64()).unwrap_or(0);
                                let ok = rep.get("ok").and_then(|v| v.as_u64()).unwrap_or(0);
                                println!("Announced to {}/{} paired peers", ok, sent);
                                if let Some(stale) = rep.get("stale_peers").and_then(|v| v.as_array()) {
                                    if !stale.is_empty() {
                                        println!("Stale peers (need manual update):");
                                        for s in stale { println!("  - {}", s.as_str().unwrap_or("?")); }
                                    }
                                }
                                if let Some(errs) = rep.get("errors").and_then(|v| v.as_array()) {
                                    if !errs.is_empty() {
                                        println!("Errors:");
                                        for e in errs { println!("  - {}", e.as_str().unwrap_or("?")); }
                                    }
                                }
                            }
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().unwrap_or_default();
                            eprintln!("Error ({}): {}", status, body);
                            std::process::exit(1);
                        }
                        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                    }
                    return;
                }
                MeshAction::Peer { action } => match action {
                    MeshPeerAction::SetAddr { peer, addr } => {
                        let (ip, port) = mesh_cli::parse_ip_port(&addr);
                        if ip.is_empty() {
                            eprintln!("Error: invalid address '{}'", addr);
                            std::process::exit(1);
                        }
                        let mut body = serde_json::json!({"ip": ip});
                        if let Some(p) = port { body["port"] = serde_json::json!(p); }
                        let url = format!("{}/api/mesh/peers/{}/addr", base, urlencoding::encode(&peer));
                        match http.post(&url).json(&body).send() {
                            Ok(resp) if resp.status().is_success() => {
                                let val: serde_json::Value = resp.json().unwrap_or_default();
                                if cli.json {
                                    println!("{}", serde_json::to_string_pretty(&val).unwrap());
                                } else {
                                    let prev = val.get("previous").and_then(|v| v.as_array())
                                        .map(|a| a.iter().filter_map(|x| x.get("ip").and_then(|v| v.as_str())).collect::<Vec<_>>().join(", "))
                                        .unwrap_or_default();
                                    println!("Set {} → {}{}",
                                        peer,
                                        ip,
                                        port.map(|p| format!(":{}", p)).unwrap_or_default()
                                    );
                                    if !prev.is_empty() {
                                        println!("  was: {}", prev);
                                    }
                                }
                            }
                            Ok(resp) => {
                                let status = resp.status();
                                let body = resp.text().unwrap_or_default();
                                eprintln!("Error ({}): {}", status, body);
                                std::process::exit(1);
                            }
                            Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
                        }
                        return;
                    }
                },
            }
        }
        Commands::Mem { action } => {
            if let MemAction::RunWorker { kind, db_path } = action {
                cmd::worker::run_mem_worker(&kind, db_path.as_deref());
                return;
            }
            let json_mode = cli.json;
            handle_mem(json_mode, action);
            return;
        }
        Commands::Sessions { action } => {
            let (base_url, token) = read_api_config();
            let http = build_http(&token);
            cmd::sessions::handle_sessions(cli.json, action, &http, &base_url, token.as_deref());
            return;
        }
    };

    handle_result(cli.json, result);
}

// ── Mem CLI handler ─────────────────────────────────────────────────

/// Lightweight frontmatter parser for SKILL.md.
/// Recognises `title:`, `area:`, `tags:` (comma-separated) between leading `---` lines.
fn strip_skill_frontmatter(body: &str) -> &str {
    let mut lines = body.lines();
    if lines.next().map(|l| l.trim()) != Some("---") { return body; }
    let mut consumed = body.lines().next().unwrap_or("").len() + 1;
    for line in lines {
        consumed += line.len() + 1;
        if line.trim() == "---" {
            return body.get(consumed..).unwrap_or("");
        }
    }
    body
}

fn parse_skill_frontmatter(body: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let mut lines = body.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return (None, None, Vec::new());
    }
    let mut title = None;
    let mut area = None;
    let mut tags = Vec::new();
    for line in lines {
        let t = line.trim();
        if t == "---" { break; }
        if let Some(v) = t.strip_prefix("title:") {
            title = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("area:") {
            area = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("tags:") {
            tags = v.trim().trim_matches('[').trim_matches(']')
                .split(',').map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty()).collect();
        }
    }
    (title, area, tags)
}

fn mem_get_request(http: &reqwest::blocking::Client, url: &str) -> Result<serde_json::Value, String> {
    let resp = http.get(url).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    let val: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        return Err(val.get("message").and_then(|v| v.as_str()).map(String::from)
            .unwrap_or_else(|| format!("HTTP {}", status)));
    }
    Ok(val)
}

fn mem_post_request(
    http: &reqwest::blocking::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = http.post(url).json(body).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    let val: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        return Err(val.to_string());
    }
    Ok(val)
}

fn handle_mem(json_mode: bool, action: MemAction) {
    use std::io::Read;
    let (base, token) = read_api_config();
    let http = build_http(&token);

    let result: Result<serde_json::Value, String> = match action {
        MemAction::Search { query, kind, area, scope, limit, rerank } => {
            let mut url = format!("{}/api/mem/search?q={}&limit={}",
                base, urlencoding::encode(&query), limit);
            if let Some(k) = &kind { url.push_str(&format!("&kind={}", k)); }
            if let Some(a) = &area { url.push_str(&format!("&area={}", a)); }
            if let Some(s) = &scope { url.push_str(&format!("&scope={}", s)); }
            if rerank { url.push_str("&rerank=true"); }
            mem_get_request(&http, &url)
        }
        MemAction::Get { id } => {
            mem_get_request(&http, &format!("{}/api/mem/items/{}", base, id))
        }
        MemAction::Delete { id } => {
            let resp = http.delete(format!("{}/api/mem/items/{}", base, id))
                .send().map_err(|e| e.to_string());
            resp.map(|r| serde_json::json!({"deleted": r.status().is_success()}))
        }
        MemAction::History { id } => {
            let val = mem_get_request(&http, &format!("{}/api/mem/items/{}/history", base, id));
            // Pretty-print timeline in human mode; keep raw JSON for --json.
            if !json_mode {
                match &val {
                    Ok(v) => {
                        if let Some(rows) = v.get("history").and_then(|h| h.as_array()) {
                            print_history_timeline(rows);
                            return;
                        }
                    }
                    Err(_) => {}
                }
            }
            val
        }
        MemAction::Taxonomy { workspace } => {
            let mut url = format!("{}/api/mem/taxonomy", base);
            if let Some(ws) = &workspace {
                url.push_str(&format!("?workspace={}", urlencoding::encode(ws)));
            }
            let val = mem_get_request(&http, &url);
            if !json_mode {
                if let Ok(ref v) = val {
                    print_taxonomy(v);
                    return;
                }
            }
            val
        }
        MemAction::Tail { follow, limit } => {
            return run_mem_tail(&http, &base, json_mode, follow, limit);
        }
        MemAction::Update { id, title, summary, area, tags, body, importance } => {
            let tag_list: Option<Vec<String>> = tags.map(|t|
                t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            );
            let payload = serde_json::json!({
                "title": title, "summary": summary, "area": area,
                "tags": tag_list, "body": body, "importance": importance,
            });
            let resp = http.patch(format!("{}/api/mem/items/{}", base, id))
                .json(&payload)
                .send()
                .map_err(|e| e.to_string());
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let val: serde_json::Value = r.json().unwrap_or_default();
                    if status.is_success() { Ok(val) } else { Err(val.to_string()) }
                }
                Err(e) => Err(e),
            }
        }
        MemAction::Save { kind, title, body, summary, tags, area, importance, scope, source_path, replace, skip_extract } => {
            // Body: positional arg, else read stdin if piped
            let resolved_body = match body {
                Some(b) => b,
                None => {
                    let mut s = String::new();
                    if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                        eprintln!("Error: no body provided (pass as arg or pipe to stdin)");
                        std::process::exit(1);
                    }
                    s
                }
            };
            let resolved_title = title.unwrap_or_else(|| {
                resolved_body.lines().next().unwrap_or("untitled").chars().take(80).collect()
            });
            let split_csv = |raw: Option<String>| -> Vec<String> {
                raw.map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default()
            };
            let tag_list = split_csv(tags);
            let payload = serde_json::json!({
                "kind": kind,
                "title": resolved_title,
                "summary": summary,
                "area": area,
                "tags": tag_list,
                "body": resolved_body,
                "importance": importance,
                "scope": scope,
                "source_path": source_path,
                "replace": replace,
                "skip_extract": skip_extract,
            });
            mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
        }
        MemAction::ExtractPending { limit } => {
            mem_post_request(
                &http,
                &format!("{}/api/mem/extract-pending?limit={}", base, limit),
                &serde_json::json!({}),
            )
        }
        MemAction::Skill { action } => match action {
            MemSkillAction::New { name, area } => {
                let skills_root = dirs::home_dir().unwrap_or_default().join(data_dir_name()).join("skills");
                let folder_path = skills_root.join(&name);
                if folder_path.exists() {
                    eprintln!("Error: skill folder already exists: {}", folder_path.display());
                    std::process::exit(1);
                }
                if let Err(e) = std::fs::create_dir_all(folder_path.join("scripts")) {
                    eprintln!("Error creating folder: {}", e);
                    std::process::exit(1);
                }
                let area_line = area.as_deref().unwrap_or("general");
                let template = format!(
                    "---\ntitle: {name}\narea: {area_line}\ntags: []\n---\n\n# {name}\n\n_One-paragraph summary that becomes the search snippet._\n\n## When to use\n\n…\n\n## Steps\n\n1. …\n2. …\n\n## Gotchas\n\n…\n\n## Scripts\n\n- `./scripts/foo.sh` — describe what it does\n",
                    name = name, area_line = area_line,
                );
                let skill_md = folder_path.join("SKILL.md");
                if let Err(e) = std::fs::write(&skill_md, template) {
                    eprintln!("Error writing SKILL.md: {}", e);
                    std::process::exit(1);
                }
                eprintln!("Created skill folder: {}", folder_path.display());
                eprintln!("Edit: {}", skill_md.display());
                eprintln!("Add scripts to: {}", folder_path.join("scripts").display());
                eprintln!();
                // Register immediately so it appears in mem search right away.
                let body = std::fs::read_to_string(&skill_md).unwrap_or_default();
                let (fm_title, fm_area, _fm_tags) = parse_skill_frontmatter(&body);
                let title = fm_title.unwrap_or(name.clone());
                let area_resolved = area.clone().or(fm_area);
                let summary = format!("Skill scaffold — edit {} to fill in.", skill_md.display());
                let source = format!("file:{}", skill_md.display());
                let mut payload = serde_json::json!({
                    "kind": "skill",
                    "title": title,
                    "body": body,
                    "summary": summary,
                    "source": source,
                    "source_path": source,
                    "tags": ["folder-skill", "scaffold"],
                    "importance": 4.0,
                    "skip_extract": true,
                    "replace": true,
                });
                if let Some(a) = area_resolved {
                    payload["area"] = serde_json::json!(a);
                }
                mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
            }
            MemSkillAction::Add { folder, area, importance } => {
                let folder_expanded = if let Some(stripped) = folder.strip_prefix("~/") {
                    dirs::home_dir().map(|h| h.join(stripped)).unwrap_or_else(|| std::path::PathBuf::from(&folder))
                } else if folder == "~" {
                    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"))
                } else {
                    std::path::PathBuf::from(&folder)
                };
                let folder_path = folder_expanded;
                let skill_md = folder_path.join("SKILL.md");
                if !skill_md.exists() {
                    eprintln!("Error: SKILL.md not found in {}", folder_path.display());
                    std::process::exit(1);
                }
                let body = match std::fs::read_to_string(&skill_md) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error reading SKILL.md: {}", e);
                        std::process::exit(1);
                    }
                };
                // Optional YAML-ish frontmatter parse: between two --- lines at top.
                // Picks `title:`, `area:`, `tags:` (comma list) if present.
                let (fm_title, fm_area, fm_tags) = parse_skill_frontmatter(&body);
                // Title priority: frontmatter → first H1 → folder name.
                let title = fm_title.or_else(|| body.lines()
                    .find(|l| l.trim_start().starts_with("# "))
                    .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string()))
                    .unwrap_or_else(|| folder_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "skill".into()));
                // Area priority: --area flag → frontmatter → parent folder name (if grandparent is skills/).
                let area_resolved = area.clone().or(fm_area).or_else(|| {
                    let parent = folder_path.parent()?.file_name()?.to_string_lossy().to_string();
                    if parent == "skills" { None } else { Some(parent) }
                });
                // Skip the YAML frontmatter block before picking summary.
                let body_after_fm = strip_skill_frontmatter(&body);
                let summary: String = body_after_fm.lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                    .next().unwrap_or("").to_string();
                let source = format!("file:{}", skill_md.display());
                let mut tags: Vec<String> = vec!["folder-skill".into()];
                tags.extend(fm_tags);
                let mut payload = serde_json::json!({
                    "kind": "skill",
                    "title": title,
                    "body": body,
                    "summary": summary,
                    "source": source,
                    "source_path": source,
                    "tags": tags,
                    "importance": importance,
                    "skip_extract": true,
                    "replace": true,
                });
                if let Some(a) = area_resolved {
                    payload["area"] = serde_json::json!(a);
                }
                mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
            }
            MemSkillAction::List => {
                match mem_get_request(&http, &format!("{}/api/mem/tail?limit=500", base)) {
                    Ok(resp) => {
                        if let Some(rows) = resp.get("rows").and_then(|v| v.as_array()) {
                            let skills: Vec<&serde_json::Value> = rows.iter()
                                .filter(|r| r.get("kind").and_then(|k| k.as_str()) == Some("skill"))
                                .collect();
                            Ok(serde_json::json!({"skills": skills}))
                        } else {
                            Ok(resp)
                        }
                    }
                    Err(e) => Err(e),
                }
            }
        },
        MemAction::WakeUp { budget, l0_only, no_l0 } => {
            let mut url = format!("{}/api/mem/wake-up", base);
            let mut sep = '?';
            if let Some(b) = budget {
                url.push_str(&format!("{}budget={}", sep, b));
                sep = '&';
            }
            if l0_only { url.push_str(&format!("{}l0_only=true", sep)); sep = '&'; }
            if no_l0 { url.push_str(&format!("{}no_l0=true", sep)); let _ = sep; }
            mem_get_request(&http, &url)
        }
        MemAction::Stats => {
            mem_get_request(&http, &format!("{}/api/mem/stats", base))
        }
        MemAction::Learn { text, kind, tags, error, feature } => {
            let (resolved_text, resolved_kind) = if let Some(t) = error {
                (t, "error".to_string())
            } else if let Some(t) = feature {
                (t, "feature".to_string())
            } else if let Some(t) = text {
                (t, kind.unwrap_or_else(|| "insight".into()))
            } else {
                let mut s = String::new();
                if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                    eprintln!("Error: provide text as arg, --error/--feature, or pipe to stdin");
                    std::process::exit(1);
                }
                (s, kind.unwrap_or_else(|| "insight".into()))
            };
            let tag_list: Vec<String> = tags
                .map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            mem_post_request(&http, &format!("{}/api/mem/learn", base), &serde_json::json!({
                "text": resolved_text, "kind": resolved_kind, "tags": tag_list,
            }))
        }
        MemAction::LearnList => {
            mem_get_request(&http, &format!("{}/api/mem/search?q=learning&kind=learning&limit=50", base))
        }
        MemAction::LearnPromote { id } => {
            mem_post_request(&http, &format!("{}/api/mem/promote/{}", base, id), &serde_json::json!({}))
        }
        MemAction::Flush { pane, title } => {
            let pane_id = pane
                .or_else(|| std::env::var("CODETERM_PANE_ID").ok())
                .unwrap_or_else(|| "current".to_string());
            mem_post_request(&http, &format!("{}/api/mem/flush", base), &serde_json::json!({
                "pane": pane_id, "title": title,
            }))
        }
        MemAction::Dream => {
            mem_post_request(&http, &format!("{}/api/mem/dream", base), &serde_json::json!({}))
        }
        MemAction::Sync { path } => {
            let mut url = format!("{}/api/mem/sync", base);
            if let Some(p) = path {
                url.push_str(&format!("?path={}", urlencoding::encode(&p)));
            }
            mem_post_request(&http, &url, &serde_json::json!({}))
        }
        MemAction::Graph { action } => match action {
            MemGraphAction::Create { id, entity_type, props, scope } => {
                let props_val: serde_json::Value =
                    serde_json::from_str(&props).unwrap_or(serde_json::json!({}));
                let payload = serde_json::json!({
                    "id": id, "type": entity_type, "scope": scope, "properties": props_val,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/entities", base), &payload)
            }
            MemGraphAction::Relate { from, rel, to, valid_from, valid_to, confidence, scope } => {
                let payload = serde_json::json!({
                    "from": from, "rel": rel, "to": to,
                    "valid_from": valid_from, "valid_to": valid_to,
                    "confidence": confidence, "scope": scope,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/relate", base), &payload)
            }
            MemGraphAction::Related { id, rel, as_of } => {
                let mut url = format!("{}/api/mem/graph/related?id={}", base, urlencoding::encode(&id));
                if let Some(r) = &rel { url.push_str(&format!("&rel={}", r)); }
                if let Some(d) = &as_of { url.push_str(&format!("&as_of={}", urlencoding::encode(d))); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Invalidate { from, rel, to, ended } => {
                let payload = serde_json::json!({
                    "from": from, "rel": rel, "to": to, "ended": ended,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/invalidate", base), &payload)
            }
            MemGraphAction::Timeline { entity } => {
                let url = format!("{}/api/mem/graph/timeline?entity={}", base, urlencoding::encode(&entity));
                mem_get_request(&http, &url)
            }
            MemGraphAction::Traverse { from, hops, max_nodes, as_of } => {
                let mut url = format!("{}/api/mem/graph/traverse?from={}&hops={}&max_nodes={}",
                    base, urlencoding::encode(&from), hops, max_nodes);
                if let Some(d) = &as_of { url.push_str(&format!("&as_of={}", urlencoding::encode(d))); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Path { from, to, max_hops } => {
                let url = format!("{}/api/mem/graph/path?from={}&to={}&max_hops={}",
                    base, urlencoding::encode(&from), urlencoding::encode(&to), max_hops);
                mem_get_request(&http, &url)
            }
            MemGraphAction::Tunnels => {
                mem_get_request(&http, &format!("{}/api/mem/graph/tunnels", base))
            }
            MemGraphAction::Alias { alias, canonical } => {
                let payload = serde_json::json!({ "alias": alias, "canonical": canonical });
                mem_post_request(&http, &format!("{}/api/mem/graph/alias", base), &payload)
            }
        }
        MemAction::Worker { action } => match action {
            MemWorkerAction::Spawn { intent, task, agent, parent } => {
                let payload = serde_json::json!({
                    "intent": intent,
                    "task": task,
                    "agent": agent,
                    "parent": parent,
                });
                mem_post_request(&http, &format!("{}/api/mem/workers", base), &payload)
            }
            MemWorkerAction::List => {
                mem_get_request(&http, &format!("{}/api/mem/workers", base))
            }
            MemWorkerAction::Get { id } => {
                mem_get_request(&http, &format!("{}/api/mem/workers/{}", base, id))
            }
            MemWorkerAction::Progress { text, status } => {
                match std::env::var("CODETERM_WORKER_ID") {
                    Ok(id) => {
                        let payload = serde_json::json!({ "text": text, "status": status });
                        mem_post_request(&http, &format!("{}/api/mem/workers/{}/progress", base, id), &payload)
                    }
                    Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
                }
            }
            MemWorkerAction::Report { summary } => {
                match std::env::var("CODETERM_WORKER_ID") {
                    Ok(id) => {
                        let payload = serde_json::json!({ "summary": summary });
                        mem_post_request(&http, &format!("{}/api/mem/workers/{}/report", base, id), &payload)
                    }
                    Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
                }
            }
            MemWorkerAction::Kill { id } => {
                let resp = http.delete(format!("{}/api/mem/workers/{}", base, id))
                    .send().map_err(|e| e.to_string());
                resp.map(|r| serde_json::json!({"ok": r.status().is_success()}))
            }
        }
        MemAction::RunWorker { .. } => unreachable!("RunWorker is short-circuited in main()"),
        MemAction::Secret { action } => {
            return cmd::mem_secret::handle_mem_secret(json_mode, &http, &base, action);
        }
    };

    // Wake-up returns plain text in `text` field — print it directly.
    if let Ok(ref val) = result {
        if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
            println!("{}", text);
            return;
        }
    }
    handle_result(json_mode, result);
}


// ── mem tail ────────────────────────────────────────────────────────
fn run_mem_tail(
    http: &reqwest::blocking::Client,
    base: &str,
    json_mode: bool,
    follow: bool,
    limit: usize,
) {
    let fetch = |since: Option<&str>| -> Result<Vec<serde_json::Value>, String> {
        let mut url = format!("{}/api/mem/tail?limit={}", base, limit);
        if let Some(s) = since {
            url.push_str(&format!("&since={}", urlencoding::encode(s)));
        }
        let val = mem_get_request(http, &url)?;
        Ok(val
            .get("rows")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    };
    let print_row = |row: &serde_json::Value| {
        let ts = row
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .split('.')
            .next()
            .unwrap_or("?")
            .replace('T', " ");
        let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let who = row.get("changed_by").and_then(|v| v.as_str()).unwrap_or("?");
        let title = row.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let area = row
            .get("area")
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        println!(
            "{} | {:<12} | {:<15} | {:<60} | {}",
            ts,
            kind,
            who,
            title.chars().take(60).collect::<String>(),
            area
        );
    };

    match fetch(None) {
        Ok(rows) => {
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
            } else if rows.is_empty() {
                println!("(no recent writes)");
            } else {
                // Oldest first so that follow-mode output streams naturally.
                for row in rows.iter().rev() {
                    print_row(row);
                }
            }
            if !follow {
                return;
            }
            // In follow mode we remember the newest updated_at we've printed
            // and poll with `since` every 500ms.
            let mut last_seen: Option<String> = rows
                .iter()
                .filter_map(|r| r.get("updated_at").and_then(|v| v.as_str()).map(String::from))
                .max();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let next = match fetch(last_seen.as_deref()) {
                    Ok(rows) => rows,
                    Err(e) => {
                        eprintln!("tail fetch: {}", e);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                };
                if !next.is_empty() {
                    for row in next.iter().rev() {
                        print_row(row);
                    }
                    if let Some(newest) = next
                        .iter()
                        .filter_map(|r| r.get("updated_at").and_then(|v| v.as_str()).map(String::from))
                        .max()
                    {
                        last_seen = Some(newest);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
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
