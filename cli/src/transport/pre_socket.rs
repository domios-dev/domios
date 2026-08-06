//! Handlers for the commands `transport_for` declares pre-socket. The table
//! decides which ones land here; this module only runs them.

use super::{transport_for, Local};
use crate::cmd::print::print_human;
use crate::{build_local_api, cmd, read_api_config, Cli, Commands, ConfigAction, MemAction};

/// Returns the CLI back when the command still needs the socket.
pub(crate) fn dispatch(cli: Cli) -> Option<Cli> {
    match transport_for(&cli.command).local {
        Local::PreMesh | Local::Socket => return Some(cli),
        Local::PreSocket | Local::PreSocketOrSocket => {}
    }

    let Cli {
        json,
        mesh,
        command,
    } = cli;
    match command {
        Commands::Config { action } => {
            handle_config(json, &action);
            None
        }

        // stale CODETERM_SOCKET_PATH inherited from a previous daemon PID (e.g. after update install --auto restarts the binary) doesn't make the worker exit 1 before it can do any work.
        Commands::Mem {
            action: MemAction::RunWorker { kind, db_path },
        } => {
            cmd::worker::run_mem_worker(&kind, db_path.as_deref());
            None
        }

        // Local-only graph mutation verbs — open knowledge.db directly, no daemon.
        Commands::Mem {
            action: MemAction::Graph { action },
        } => {
            if let Some(()) = cmd::graph_mutate::try_dispatch_local(&action) {
                unreachable!("local graph dispatch must exit");
            }
            Some(Cli {
                json,
                mesh,
                command: Commands::Mem {
                    action: MemAction::Graph { action },
                },
            })
        }

        // `mem dream` opens knowledge.db directly. Bridge ping is best-effort and routed through `discover_socket` itself so a wedged daemon doesn't break the planner.
        Commands::Mem {
            action: MemAction::Dream { action },
        } => cmd::dream::handle_dream(action),

        // during recovery when the daemon is wedged.
        Commands::System { action } => {
            cmd::system::handle_system(json, action);
            None
        }

        // `cli-manifest` introspects the clap tree — no daemon dependency.
        Commands::CliManifest { write, markdown } => {
            cmd::manifest::handle(write, markdown);
            None
        }

        // `docs` reads embedded markdown — no daemon dependency.
        Commands::Docs { name } => {
            cmd::docs::handle(name);
            None
        }

        _ => unreachable!("transport table declared a pre-socket command with no handler here"),
    }
}

/// Build the base-relative `/api/config` GET path. With a key it scopes the
/// query (`?key=<k>`); without one it omits the param so the backend returns
/// the full redacted config dump.
pub(crate) fn config_get_path(key: Option<&str>) -> String {
    match key {
        Some(k) => format!("/api/config?key={}", k),
        None => "/api/config".to_string(),
    }
}

fn handle_config(json_output: bool, action: &ConfigAction) {
    let (base_url, _) = read_api_config();
    let http = match build_local_api() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
            std::process::exit(1);
        }
    };
    let result = match action {
        ConfigAction::Get { key } => {
            let url = format!("{}{}", base_url, config_get_path(key.as_deref()));
            match http.get(&url).send() {
                Ok(resp) if resp.status().is_success() => resp.json().map_err(|e| e.to_string()),
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().unwrap_or_default();
                    if status.as_u16() == 404 {
                        Err(format!("Key not found: {}", key.as_deref().unwrap_or("")))
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
                    let applied: serde_json::Value = resp.json().unwrap_or_default();
                    let mut out = serde_json::json!({"ok": true, "key": key, "value": json_value});
                    if let Some(bind) = applied.get("api_bind") {
                        out["api_bind"] = bind.clone();
                    }
                    Ok(out)
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
            } else {
                if let Some(v) = value.get("value") {
                    match v {
                        serde_json::Value::String(s) => println!("{}", s),
                        serde_json::Value::Null => println!("null"),
                        other => println!("{}", other),
                    }
                } else {
                    print_human(&value);
                }
                // stdout stays the bare value for scripting; the divergence goes to stderr.
                if let Some(bind) = value.get("api_bind") {
                    let live = bind.get("live").and_then(|v| v.as_str()).unwrap_or("?");
                    let configured = bind
                        .get("configured")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    eprintln!(
                        "warning: NOT applied to the running listener — it is still bound to {live}, \
                         config stores {configured}. Restart CodeTerm to apply, and verify with \
                         `lsof -nP -iTCP:<port> -sTCP:LISTEN`."
                    );
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
mod tests {
    use super::config_get_path;

    #[test]
    fn config_get_path_omits_key_param_when_absent() {
        assert_eq!(config_get_path(None), "/api/config");
    }

    #[test]
    fn config_get_path_scopes_query_when_key_present() {
        assert_eq!(config_get_path(Some("api.port")), "/api/config?key=api.port");
    }
}
