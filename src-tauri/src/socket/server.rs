/// JSON-RPC v2 server over named pipe (Windows) / Unix socket.

use std::io::{BufRead, BufReader, Write};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use interprocess::local_socket::{
    prelude::*,
    GenericNamespaced, ListenerOptions,
};

use crate::multiplexer::commands::MuxState;
use crate::notification::NotifState;
use crate::notification::store::NotificationLevel;
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
struct JsonRpcError {
    code: i32,
    message: String,
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
    // Dev builds use a stable socket name (no PID) so CLI tools in existing
    // PTY sessions survive dev server restarts. Only one dev instance at a time.
    // Prod builds include PID to allow multiple instances.
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
    // Must match what socket_path() produces so CLI can connect.
    let bare = socket_path(); // includes .sock suffix

    #[cfg(target_os = "windows")]
    {
        // GenericNamespaced on Windows creates: \\.\pipe\{bare_name}
        format!("\\\\.\\pipe\\{}", bare)
    }

    #[cfg(not(target_os = "windows"))]
    {
        // GenericNamespaced on Unix creates: /tmp/{bare_name}
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
                            // Extract PID from "{prefix}-{pid}.sock"
                            if let Some(pid_str) = name.strip_prefix(&sock_prefix).and_then(|s| s.strip_suffix(".sock")) {
                                if let Ok(pid) = pid_str.parse::<u32>() {
                                    // Check if process is alive via kill -0
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
        // Clean up stale sockets from crashed processes
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

        log::info!(
            "Socket server listening at {}",
            socket_display_path()
        );

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
                            handle_connection(stream, &mux, &pty, &notif, &app_handle, &out_buf, &svc);
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
    // Split into reader/writer via a reference-based approach.
    // interprocess streams implement Read+Write on the same type.
    // We need to use a BufReader and write responses after each line.
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
                        match dispatch(&req.method, req.params, mux, pty, notif, app, output_buf, svc) {
                            Ok(result) => JsonRpcResponse::success(id, result),
                            Err(err) => JsonRpcResponse::error(id, err.code, err.message),
                        }
                    }
                    Err(e) => JsonRpcResponse::error(
                        None,
                        -32700,
                        format!("Parse error: {}", e),
                    ),
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
    match method {
        "system.identify" => {
            Ok(serde_json::json!({
                "name": "CodeTerm",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
                "features": ["multiplexer", "agent-detection", "notifications"]
            }))
        }

        "system.capabilities" => {
            Ok(serde_json::json!({
                "methods": [
                    "system.identify",
                    "system.capabilities",
                    "workspace.list",
                    "workspace.create",
                    "workspace.focus",
                    "workspace.rename",
                    "workspace.close",
                    "workspace.move_tab",
                    "pane.list",
                    "pane.focus",
                    "pane.close",
                    "pane.send_text",
                    "pane.get_output",
                    "pane.rename",
                    "pane.resize",
                    "pane.kill_pty",
                    "browser.open",
                    "notification.create",
                    "notification.list",
                    "notification.clear",
                    "agent.list",
                    "agent.hierarchy",
                    "agent.spawn",
                    "agent.set_role",
                    "agent.clear_role",
                    "agent.status",
                    "agent.children",
                    "agent.link",
                    "agent.report",
                    "agent.reports",
                    "cron.list",
                    "cron.get",
                    "cron.create",
                    "cron.update",
                    "cron.delete",
                    "cron.enable",
                    "cron.disable",
                    "cron.trigger",
                    "cron.cancel",
                    "cron.history",
                    "cron.complete",
                    "bridge.send",
                    "bridge.react",
                    "bridge.restart"
                ]
            }))
        }

        "workspace.list" => {
            crate::dispatch::dispatch(&crate::dispatch::Command::WorkspaceList, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "workspace.create" => {
            let name = params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("New Workspace")
                .to_string();
            let cwd = params
                .as_ref()
                .and_then(|p| p.get("cwd"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            crate::dispatch::dispatch(
                &crate::dispatch::Command::WorkspaceCreate { name, cwd },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "workspace.focus" => {
            let id = get_string_param(&params, "workspace_id")?;
            {
                let mut mux = mux.lock();
                mux.set_active_workspace(&id).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;
            }
            { let h = app.clone(); std::thread::spawn(move || { let _ = h.emit("mux-state-changed", ()); }); }
            Ok(serde_json::json!({"ok": true}))
        }

        "workspace.rename" => {
            let workspace_id = get_string_param(&params, "workspace_id")?;
            let name = get_string_param(&params, "name")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::WorkspaceRename { workspace_id, name },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "workspace.close" => {
            let workspace_id = get_string_param(&params, "workspace_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::WorkspaceClose { workspace_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "workspace.move_tab" => {
            let tab_id = get_string_param(&params, "tab_id")?;
            let from_id = get_string_param(&params, "from_workspace_id")?;
            let to_id = get_string_param(&params, "to_workspace_id")?;
            {
                let mut mux = mux.lock();
                mux.move_tab(&tab_id, &from_id, &to_id).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;
            }
            { let h = app.clone(); std::thread::spawn(move || { let _ = h.emit("mux-state-changed", ()); }); }
            Ok(serde_json::json!({"ok": true}))
        }

        "pane.list" => {
            crate::dispatch::dispatch(&crate::dispatch::Command::PaneList, svc.as_ref())
                .map(|r| r.into_json())
                .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.focus" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneFocus { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "pane.close" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneClose { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "agents.list_providers" => {
            crate::dispatch::dispatch(
                &crate::dispatch::Command::ListAgentProviders,
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.send_text" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            let text = get_string_param(&params, "text")?;
            let enter = params
                .as_ref()
                .and_then(|p| p.get("enter"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            crate::dispatch::dispatch(
                &crate::dispatch::Command::SendText { pane_id, text, enter },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.get_output" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            let max_bytes = params
                .as_ref()
                .and_then(|p| p.get("max_bytes"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .or(Some(65536));
            let strip_ansi = params.as_ref()
                .and_then(|p| p.get("strip_ansi"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            crate::dispatch::dispatch(
                &crate::dispatch::Command::GetOutput {
                    pane_id,
                    offset: None,
                    max_bytes,
                    strip_ansi,
                    full: false,
                },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "pane.rename" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            let title = get_string_param(&params, "title")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PaneRename { pane_id, title },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32602, message: e })
        }

        "pane.resize" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            let cols = params
                .as_ref()
                .and_then(|p| p.get("cols"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsonRpcError { code: -32602, message: "missing cols".into() })?
                as u16;
            let rows = params
                .as_ref()
                .and_then(|p| p.get("rows"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| JsonRpcError { code: -32602, message: "missing rows".into() })?
                as u16;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PtyResize { pane_id, cols, rows },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "pane.kill_pty" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            crate::dispatch::dispatch(
                &crate::dispatch::Command::PtyKill { pane_id },
                svc.as_ref(),
            )
            .map(|r| r.into_json())
            .map_err(|e| JsonRpcError { code: -32603, message: e })
        }

        "browser.open" => {
            let url = get_string_param(&params, "url")?;
            let new_pane_id = {
                let mut mux = mux.lock();
                mux.open_browser(Some(url.clone())).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: e,
                })?
            };
            { let h = app.clone(); std::thread::spawn(move || { let _ = h.emit("mux-state-changed", ()); }); }
            Ok(serde_json::json!({"pane_id": new_pane_id, "url": url}))
        }

        "notification.create" => {
            let title = get_string_param(&params, "title")?;
            let body = params
                .as_ref()
                .and_then(|p| p.get("body"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let level_str = params
                .as_ref()
                .and_then(|p| p.get("level"))
                .and_then(|v| v.as_str())
                .unwrap_or("info");
            let level = match level_str {
                "warning" => NotificationLevel::Warning,
                "error" => NotificationLevel::Error,
                _ => NotificationLevel::Info,
            };
            let pane_id = params
                .as_ref()
                .and_then(|p| p.get("pane_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let emit_level = match &level {
                NotificationLevel::Warning => "Warning",
                NotificationLevel::Error => "Error",
                _ => "Info",
            };
            let emit_title = title.clone();
            let emit_body = body.clone();
            let emit_pane = pane_id.clone();

            let mut store = notif.lock();
            let id = store.add(title, body, level, pane_id, None);

            // Emit event to frontend with full payload
            { let h = app.clone(); let p = serde_json::json!({"id": id, "title": emit_title, "body": emit_body, "level": emit_level, "pane_id": emit_pane, "workspace_id": serde_json::Value::Null}); std::thread::spawn(move || { let _ = h.emit("notification-created", p); }); }

            // OS notification if window unfocused
            {
                use tauri::Manager;
                let is_focused = app
                    .get_webview_window("main")
                    .and_then(|w| w.is_focused().ok())
                    .unwrap_or(true);
                if !is_focused {
                    use tauri_plugin_notification::NotificationExt;
                    let mut builder = app.notification().builder()
                        .title(&emit_title);
                    if let Some(ref b) = emit_body {
                        builder = builder.body(b);
                    }
                    let _ = builder.show();
                }
            }

            Ok(serde_json::json!({"id": id}))
        }

        "notification.list" => {
            let unread_only = params
                .as_ref()
                .and_then(|p| p.get("unread_only"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let store = notif.lock();
            let notifications: Vec<&crate::notification::store::Notification> =
                store.list(unread_only);
            serde_json::to_value(&notifications).map_err(|e| JsonRpcError {
                code: -32603,
                message: format!("Serialization error: {}", e),
            })
        }

        "notification.clear" => {
            let id = params
                .as_ref()
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str());

            let mut store = notif.lock();
            store.clear(id);
            Ok(serde_json::json!({"ok": true}))
        }

        "agent.spawn" => {
            let p = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
            })?;
            let spawn_params = crate::service::SpawnAgentParams::from_json(p)
                .map_err(|e| JsonRpcError { code: -32602, message: e })?;

            // spawn_agent_sync: inline the sync parts, skip tokio runtime entirely
            let svc_ref = &**svc;
            let result = svc_ref.spawn_agent_sync(spawn_params)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            { let h = app.clone(); std::thread::spawn(move || { let _ = h.emit("mux-state-changed", ()); }); }
            let mut resp = serde_json::json!({
                "pane_id": result.pane_id,
                "tab_id": result.tab_id,
                "agent": result.agent,
                "title": result.title,
                "role": result.role,
            });
            if let Some(ref warning) = result.warning {
                resp["warning"] = serde_json::json!(warning);
            }
            Ok(resp)
        }

        "agent.report" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            let message = get_string_param(&params, "message")?;
            let svc_ref = &**svc;
            let result = svc_ref.submit_report(&pane_id, &message)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({
                "ok": true,
                "report_id": result.report_id,
            }))
        }

        "agent.reports" => {
            let pane_id = get_string_param(&params, "pane_id")?;
            let clear = params.as_ref()
                .and_then(|p| p.get("clear"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let svc_ref = &**svc;
            let reports = if clear {
                svc_ref.get_and_clear_reports(&pane_id)
            } else {
                svc_ref.get_reports(&pane_id)
            };
            serde_json::to_value(&reports).map_err(|e| JsonRpcError {
                code: -32603,
                message: e.to_string(),
            })
        }

        // ── Cron ────────────────────────────────────────────────

        "cron.list" => {
            let ws_id = params.as_ref().and_then(|p| p.get("workspace_id")).and_then(|v| v.as_str());
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let jobs = svc.cron_list(&cron_state, ws_id);
            serde_json::to_value(&jobs).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.get" => {
            let id = get_string_param(&params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_get(&cron_state, &id).map_err(|e| JsonRpcError { code: -32602, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.create" => {
            let p = params.ok_or_else(|| JsonRpcError { code: -32602, message: "params required".into() })?;
            let create_params: crate::cron::store::CronCreateParams = serde_json::from_value(p)
                .map_err(|e| JsonRpcError { code: -32602, message: format!("Invalid params: {}", e) })?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_create(&cron_state, &cron_wake, create_params)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.update" => {
            let id = get_string_param(&params, "id")?;
            let p = params.ok_or_else(|| JsonRpcError { code: -32602, message: "params required".into() })?;
            let patch: crate::cron::store::CronUpdateParams = serde_json::from_value(p)
                .map_err(|e| JsonRpcError { code: -32602, message: format!("Invalid params: {}", e) })?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_update(&cron_state, &cron_wake, &id, patch)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.delete" => {
            let id = get_string_param(&params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            svc.cron_delete(&cron_state, &cron_wake, &id)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({"ok": true}))
        }

        "cron.enable" => {
            let id = get_string_param(&params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_enable(&cron_state, &cron_wake, &id, true)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.disable" => {
            let id = get_string_param(&params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let job = svc.cron_enable(&cron_state, &cron_wake, &id, false)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&job).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.trigger" => {
            let id = get_string_param(&params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let run = svc.cron_trigger(&cron_state, &id)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            serde_json::to_value(&run).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.cancel" => {
            let id = get_string_param(&params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cancelled = svc.cron_cancel_running(&cron_state, &id)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({"cancelled": cancelled}))
        }

        "cron.history" => {
            let id = get_string_param(&params, "id")?;
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let history = svc.cron_history(&cron_state, &id);
            serde_json::to_value(&history).map_err(|e| JsonRpcError { code: -32603, message: e.to_string() })
        }

        "cron.complete" => {
            let run_id = get_string_param(&params, "run_id")?;
            let summary = params.as_ref().and_then(|p| p.get("summary")).and_then(|v| v.as_str());
            let cron_state = app.try_state::<crate::CronState>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            let cron_wake = app.try_state::<crate::CronWake>()
                .ok_or_else(|| JsonRpcError { code: -32603, message: "Cron not initialized".into() })?;
            svc.cron_complete(&cron_state, &cron_wake, &run_id, summary)
                .map_err(|e| JsonRpcError { code: -32603, message: e })?;
            Ok(serde_json::json!({"ok": true}))
        }

        "bridge.restart" => {
            crate::start_telegram_bridge(svc);
            Ok(serde_json::json!({"ok": true, "message": "Telegram bridge starting"}))
        }

        "bridge.send" => {
            let chat_id = params.as_ref().and_then(|p| p.get("chat_id")).and_then(|v| v.as_i64());
            let append = params.as_ref().and_then(|p| p.get("append")).and_then(|v| v.as_bool()).unwrap_or(false);
            let content_type = params.as_ref()
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("text")
                .to_string();

            let content = match content_type.as_str() {
                "text" => {
                    let text = get_string_param(&params, "text")?;
                    crate::bridge::traits::OutboundContent::Text { text }
                }
                "photo" | "document" | "voice" | "animation" | "video" | "sticker" => {
                    let file = get_string_param(&params, "file")?;
                    // Validate file exists
                    if !std::path::Path::new(&file).exists() {
                        return Err(JsonRpcError { code: -32602, message: format!("File not found: {}", file) });
                    }
                    let caption = params.as_ref().and_then(|p| p.get("caption")).and_then(|v| v.as_str()).map(|s| s.to_string())
                        .or_else(|| params.as_ref().and_then(|p| p.get("text")).and_then(|v| v.as_str()).map(|s| s.to_string()));
                    let filename = params.as_ref().and_then(|p| p.get("filename")).and_then(|v| v.as_str()).map(|s| s.to_string());

                    match content_type.as_str() {
                        "photo" => crate::bridge::traits::OutboundContent::Photo { file, caption },
                        "document" => crate::bridge::traits::OutboundContent::Document { file, caption, filename },
                        "voice" => crate::bridge::traits::OutboundContent::Voice { file, caption },
                        "animation" => crate::bridge::traits::OutboundContent::Animation { file, caption },
                        "video" => crate::bridge::traits::OutboundContent::Video { file, caption },
                        "sticker" => crate::bridge::traits::OutboundContent::Sticker { file },
                        _ => unreachable!(),
                    }
                }
                _ => {
                    return Err(JsonRpcError { code: -32602, message: format!("Unknown content type: {}", content_type) });
                }
            };

            // Parse inline keyboard buttons (JSON array of rows)
            let buttons: Option<crate::bridge::traits::InlineKeyboard> = params.as_ref()
                .and_then(|p| p.get("buttons"))
                .and_then(|v| {
                    match serde_json::from_value(v.clone()) {
                        Ok(b) => Some(b),
                        Err(e) => {
                            log::warn!("[bridge.send] failed to parse buttons: {} — raw: {}", e, v);
                            None
                        }
                    }
                });

            if let Some(tx) = svc.bridge_outbound.lock().as_ref() {
                let msg = crate::bridge::manager::OutboundMessage {
                    user_id: chat_id,
                    content,
                    append,
                    buttons,
                };
                tx.send(msg).map_err(|e| JsonRpcError {
                    code: -32603,
                    message: format!("Bridge send failed: {}", e),
                })?;
                Ok(serde_json::json!({"ok": true}))
            } else {
                Err(JsonRpcError {
                    code: -32603,
                    message: "Bridge not active".to_string(),
                })
            }
        }

        "bridge.react" => {
            let chat_id = params.as_ref().and_then(|p| p.get("chat_id")).and_then(|v| v.as_i64())
                .ok_or(JsonRpcError { code: -32602, message: "chat_id required".into() })?;
            let message_id = params.as_ref().and_then(|p| p.get("message_id")).and_then(|v| v.as_i64())
                .ok_or(JsonRpcError { code: -32602, message: "message_id required".into() })?;
            let emoji = get_string_param(&params, "emoji")?;

            let token = crate::bridge::telegram::read_token().unwrap_or_default();
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                let resp = client
                    .post(format!("https://api.telegram.org/bot{}/setMessageReaction", token))
                    .json(&serde_json::json!({
                        "chat_id": chat_id,
                        "message_id": message_id,
                        "reaction": [{"type": "emoji", "emoji": emoji}],
                    }))
                    .timeout(std::time::Duration::from_secs(10))
                    .send().await;
                match resp {
                    Ok(r) => {
                        let body: serde_json::Value = r.json().await.unwrap_or_default();
                        if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
                            log::warn!("[bridge] react failed: {}", body.get("description").and_then(|v| v.as_str()).unwrap_or("unknown"));
                        }
                    }
                    Err(e) => log::warn!("[bridge] react error: {}", e),
                }
            });
            Ok(serde_json::json!({"ok": true}))
        }

        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
        }),
    }
}

/// Extract a required string parameter from JSON-RPC params.
fn get_string_param(
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

use std::io::Read;
