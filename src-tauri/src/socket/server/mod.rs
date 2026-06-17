//! JSON-RPC v2 server over named pipe (Windows) / Unix socket.
//!
//! Outer dispatch routes by method-prefix to per-domain submodules. Children
//! own only the state they actually need (workspace + pane carry MuxState +
//! AppHandle, notification gets NotifState, cron/agent/bridge get ServiceState
//! + AppHandle).

mod agent;
mod agent_commands;
mod bridge;
mod cron;
mod notification;
mod pane;
mod services;
mod workspace;

use std::io::{BufRead, BufReader, Read, Write};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};

use crate::multiplexer::commands::MuxState;
use crate::notification::NotifState;
use crate::service::ServiceState;
use crate::{OutputBufferState, PtyState};

// ── JSON-RPC types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
pub(super) struct JsonRpcError {
    pub(super) code: i32,
    pub(super) message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

// ── Socket path ─────────────────────────────────────────────────────

/// Get the namespaced socket name for the current process.
///
/// `GenericNamespaced` on Unix prepends `/tmp/` automatically,
/// so we only provide the bare name here.
pub fn socket_path() -> String {
    // Dev builds use a stable socket name (no PID) so CLI tools in existing PTY sessions survive dev server restarts. Only one dev instance at a
    // time. Prod builds include PID to allow multiple instances.
    #[cfg(debug_assertions)]
    {
        format!("{}.sock", crate::paths::SOCKET_PREFIX)
    }
    #[cfg(not(debug_assertions))]
    {
        let pid = std::process::id();
        format!("{}-{}.sock", crate::paths::SOCKET_PREFIX, pid)
    }
}

/// Get the full display path (for env var / user display).
///
/// On Unix, `GenericNamespaced` places the socket at `/tmp/<name>`,
/// so the display path is `/tmp/{prefix}-{pid}.sock`.
/// On Windows, it's `\\.\pipe\{prefix}-{pid}`.
pub fn socket_display_path() -> String {
    let bare = socket_path();

    #[cfg(target_os = "windows")]
    {
        format!("\\\\.\\pipe\\{}", bare)
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("/tmp/{}", bare)
    }
}

// ── Server ──────────────────────────────────────────────────────────

pub struct SocketServer;

impl SocketServer {
    /// Clean up stale sockets from previous runs that didn't exit cleanly.
    pub fn cleanup_stale_sockets() {
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(entries) = std::fs::read_dir("/tmp") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        let sock_prefix = format!("{}-", crate::paths::SOCKET_PREFIX);
                        if name.starts_with(&sock_prefix) && name.ends_with(".sock") {
                            if let Some(pid_str) = name
                                .strip_prefix(&sock_prefix)
                                .and_then(|s| s.strip_suffix(".sock"))
                            {
                                if let Ok(pid) = pid_str.parse::<u32>() {
                                    let alive = std::process::Command::new("kill")
                                        .args(["-0", &pid.to_string()])
                                        .stdout(std::process::Stdio::null())
                                        .stderr(std::process::Stdio::null())
                                        .status()
                                        .map(|s| s.success())
                                        .unwrap_or(false);
                                    if !alive {
                                        let _ = std::fs::remove_file(&path);
                                        log::info!("Cleaned stale socket: {}", name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Start the JSON-RPC server in a background thread.
    pub fn start(
        mux_state: MuxState,
        pty_state: PtyState,
        notif_state: NotifState,
        app: AppHandle,
        output_buf: OutputBufferState,
        svc_state: ServiceState,
    ) -> Result<(), String> {
        Self::cleanup_stale_sockets();

        // Dev builds use a fixed socket name — remove stale file before binding
        #[cfg(all(debug_assertions, not(target_os = "windows")))]
        {
            let display = socket_display_path();
            let path = std::path::Path::new(&display);
            if path.exists() {
                let _ = std::fs::remove_file(path);
                log::info!("Removed stale dev socket: {}", display);
            }
        }

        let name = socket_path();

        let name = name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| format!("Failed to create namespaced name: {}", e))?;

        let opts = ListenerOptions::new().name(name);

        let listener = opts
            .create_sync()
            .map_err(|e| format!("Failed to create socket listener: {}", e))?;

        log::info!("Socket server listening at {}", socket_display_path());

        std::thread::spawn(move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(stream) => {
                        let mux = mux_state.clone();
                        let pty = pty_state.clone();
                        let notif = notif_state.clone();
                        let app_handle = app.clone();
                        let out_buf = output_buf.clone();
                        let svc = svc_state.clone();

                        std::thread::spawn(move || {
                            handle_connection(
                                stream,
                                &mux,
                                &pty,
                                &notif,
                                &app_handle,
                                &out_buf,
                                &svc,
                            );
                        });
                    }
                    Err(e) => {
                        log::error!("Socket accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }
}

/// Handle a single client connection: read lines, parse JSON-RPC, respond.
fn handle_connection(
    stream: impl Read + Write,
    mux: &MuxState,
    pty: &PtyState,
    notif: &NotifState,
    app: &AppHandle,
    output_buf: &OutputBufferState,
    svc: &ServiceState,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                    Ok(req) => {
                        let id = req.id.clone();
                        let id_for_panic = id.clone();
                        let method = req.method.clone();
                        // catch_unwind so a panic inside any dispatch handler does not silently drop the socket and leave the CLI with an "EOF while parsing" mystery.
                        // We surface a real JSON-RPC error instead.
                        let dispatched =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                dispatch(
                                    &req.method,
                                    req.params,
                                    mux,
                                    pty,
                                    notif,
                                    app,
                                    output_buf,
                                    svc,
                                )
                            }));
                        match dispatched {
                            Ok(Ok(result)) => JsonRpcResponse::success(id, result),
                            Ok(Err(err)) => JsonRpcResponse::error(id, err.code, err.message),
                            Err(panic) => {
                                let panic_msg = if let Some(s) = panic.downcast_ref::<&str>() {
                                    (*s).to_string()
                                } else if let Some(s) = panic.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "non-string panic payload".to_string()
                                };
                                log::error!(
                                    "JSON-RPC dispatch panicked for method '{}': {}",
                                    method,
                                    panic_msg
                                );
                                JsonRpcResponse::error(
                                    id_for_panic,
                                    -32603,
                                    format!("internal panic in '{}': {}", method, panic_msg),
                                )
                            }
                        }
                    }
                    Err(e) => JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e)),
                };

                if let Ok(json) = serde_json::to_string(&response) {
                    let writer = reader.get_mut();
                    let _ = writer.write_all(json.as_bytes());
                    let _ = writer.write_all(b"\n");
                    let _ = writer.flush();
                }
            }
            Err(e) => {
                log::error!("Socket read error: {}", e);
                break;
            }
        }
    }
}

// ── Dispatch ────────────────────────────────────────────────────────

fn dispatch(
    method: &str,
    params: Option<serde_json::Value>,
    mux: &MuxState,
    _pty: &PtyState,
    notif: &NotifState,
    app: &AppHandle,
    _output_buf: &OutputBufferState,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    // System methods are tiny — handle inline.
    match method {
        "system.identify" => {
            return Ok(serde_json::json!({
                "name": "CodeTerm",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
                "features": ["multiplexer", "agent-detection", "notifications"]
            }));
        }
        "system.capabilities" => {
            return Ok(serde_json::json!({
                "methods": [
                    "system.identify",
                    "system.capabilities",
                    "workspace.list", "workspace.create", "workspace.focus",
                    "workspace.rename", "workspace.close", "workspace.move_tab",
                    "pane.list", "pane.focus", "pane.close", "pane.send_text",
                    "pane.get_output", "pane.rename", "pane.resize", "pane.kill_pty",
                    "browser.open",
                    "notification.create", "notification.list", "notification.clear",
                    "agent.list", "agent.hierarchy", "agent.spawn", "agent.set_role",
                    "agent.clear_role", "agent.status", "agent.children", "agent.link",
                    "agent.report", "agent.reports",
                    "cron.list", "cron.get", "cron.create", "cron.update", "cron.delete",
                    "cron.enable", "cron.disable", "cron.trigger", "cron.cancel",
                    "cron.history", "cron.complete",
                    "services.list", "services.create", "services.get", "services.update",
                    "services.delete", "services.regenerate_token",
                    "activity.list",
                    "bridge.send", "bridge.react", "bridge.restart"
                ]
            }));
        }
        #[cfg(debug_assertions)]
        "debug.crash" => {
            let kind = get_string_param(&params, "kind")?;
            crate::crash::trigger_test_crash(&kind).map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
            })?;
            return Ok(serde_json::json!({"ok": true, "kind": kind}));
        }
        _ => {}
    }

    // Domain prefix → submodule. Each submodule owns its own per-method match (and returns Method-not-found for anything within its prefix it
    // doesn't know). The split also breaks on `:` so the reserved `plugin:<id>.<verb>`
    // inverse-seam namespace (F5) yields the `plugin` prefix.
    let prefix = method.split(['.', ':']).next().unwrap_or("");
    match prefix {
        "workspace" => workspace::dispatch(method, &params, mux, app, svc),
        "pane" | "agents" | "browser" => pane::dispatch(method, &params, mux, app, svc),
        "notification" => notification::dispatch(method, &params, notif, app),
        "agent" => agent::dispatch(method, &params, app, svc),
        "plugin" => agent_commands::dispatch(method, &params, svc),
        "cron" => cron::dispatch(method, &params, app, svc),
        "services" | "activity" => services::dispatch(method, &params, app, svc),
        "bridge" => bridge::dispatch(method, &params, svc),
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}

/// Extract a required string parameter from JSON-RPC params.
pub(super) fn get_string_param(
    params: &Option<serde_json::Value>,
    key: &str,
) -> Result<String, JsonRpcError> {
    params
        .as_ref()
        .and_then(|p| p.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| JsonRpcError {
            code: -32602,
            message: format!("Missing required parameter: {}", key),
        })
}
