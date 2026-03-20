/// JSON-RPC v2 server over named pipe (Windows) / Unix socket.

use std::io::{BufRead, BufReader, Write};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use interprocess::local_socket::{
    prelude::*,
    GenericNamespaced, ListenerOptions,
};

use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::Orientation;
use crate::notification::NotifState;
use crate::notification::store::NotificationLevel;
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

/// Get the socket path for the current process.
pub fn socket_path() -> String {
    let pid = std::process::id();

    #[cfg(target_os = "windows")]
    {
        format!("codeterm-{}", pid)
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("/tmp/codeterm-{}.sock", pid)
    }
}

/// Get the full display path (for env var).
pub fn socket_display_path() -> String {
    let pid = std::process::id();

    #[cfg(target_os = "windows")]
    {
        format!("\\\\.\\pipe\\codeterm-{}", pid)
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("/tmp/codeterm-{}.sock", pid)
    }
}

// ── Server ──────────────────────────────────────────────────────────

pub struct SocketServer;

impl SocketServer {
    /// Start the JSON-RPC server in a background thread.
    pub fn start(
        mux_state: MuxState,
        pty_state: PtyState,
        notif_state: NotifState,
        app: AppHandle,
        output_buf: OutputBufferState,
    ) -> Result<(), String> {
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

                        std::thread::spawn(move || {
                            handle_connection(stream, &mux, &pty, &notif, &app_handle, &out_buf);
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
                        match dispatch(&req.method, req.params, mux, pty, notif, app, output_buf) {
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
    output_buf: &OutputBufferState,
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
                    "pane.split",
                    "pane.rename",
                    "browser.open",
                    "notification.create",
                    "notification.list",
                    "notification.clear"
                ]
            }))
        }

        "workspace.list" => {
            let mux = mux.lock();
            let workspaces = mux.list_workspaces();
            serde_json::to_value(&workspaces).map_err(|e| JsonRpcError {
                code: -32603,
                message: format!("Serialization error: {}", e),
            })
        }

        "workspace.create" => {
            let name = params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("New Workspace")
                .to_string();

            let info = {
                let mut mux = mux.lock();
                mux.create_workspace(name)
            };
            let _ = app.emit("mux-state-changed", ());
            serde_json::to_value(&info).map_err(|e| JsonRpcError {
                code: -32603,
                message: format!("Serialization error: {}", e),
            })
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
            let _ = app.emit("mux-state-changed", ());
            Ok(serde_json::json!({"ok": true}))
        }

        "workspace.rename" => {
            let id = get_string_param(&params, "workspace_id")?;
            let name = get_string_param(&params, "name")?;
            {
                let mut mux = mux.lock();
                mux.rename_workspace(&id, &name).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;
            }
            let _ = app.emit("mux-state-changed", ());
            Ok(serde_json::json!({"ok": true}))
        }

        "workspace.close" => {
            let id = get_string_param(&params, "workspace_id")?;
            let pty_ids = {
                let mut mux = mux.lock();
                mux.close_workspace(&id).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?
            };
            for pty_id in &pty_ids {
                let _ = _pty.lock().kill(pty_id);
            }
            let _ = app.emit("mux-state-changed", ());
            Ok(serde_json::json!({"ok": true}))
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
            let _ = app.emit("mux-state-changed", ());
            Ok(serde_json::json!({"ok": true}))
        }

        "pane.list" => {
            let mux = mux.lock();
            let snapshot = mux.snapshot();
            // Collect panes with their status
            let panes: Vec<serde_json::Value> = snapshot
                .panes
                .values()
                .map(|p| {
                    serde_json::json!({
                        "id": p.id,
                        "pty_id": p.pty_id,
                        "pane_type": p.pane_type,
                        "title": p.title,
                        "cwd": p.cwd,
                    })
                })
                .collect();
            Ok(serde_json::json!(panes))
        }

        "pane.focus" => {
            let id = get_string_param(&params, "pane_id")?;
            {
                let mut mux = mux.lock();
                mux.set_focused_pane(&id).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;
            }
            let _ = app.emit("mux-state-changed", ());
            Ok(serde_json::json!({"ok": true}))
        }

        "pane.close" => {
            let id = get_string_param(&params, "pane_id")?;
            let pty_id = {
                let mut mux = mux.lock();
                mux.close_pane(&id).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?
            };
            if let Some(pty_id) = pty_id {
                let mut pty_mgr = _pty.lock();
                let _ = pty_mgr.kill(&pty_id);
            }
            let _ = app.emit("mux-state-changed", ());
            Ok(serde_json::json!({"ok": true}))
        }

        "pane.send_text" => {
            let id = get_string_param(&params, "pane_id")?;
            let text = get_string_param(&params, "text")?;

            // Find the PTY ID for this pane
            let pty_id = {
                let mux = mux.lock();
                mux.get_pane(&id)
                    .and_then(|p| p.pty_id.clone())
                    .ok_or_else(|| JsonRpcError {
                        code: -32602,
                        message: format!("Pane {} has no PTY", id),
                    })?
            };

            let mut pty_mgr = _pty.lock();
            pty_mgr.write(&pty_id, text.as_bytes()).map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
            })?;

            Ok(serde_json::json!({"ok": true}))
        }

        "pane.get_output" => {
            let id = get_string_param(&params, "pane_id")?;
            let max_bytes = params
                .as_ref()
                .and_then(|p| p.get("max_bytes"))
                .and_then(|v| v.as_u64())
                .unwrap_or(65536) as usize;

            let pty_id = {
                let mux = mux.lock();
                mux.get_pane(&id)
                    .and_then(|p| p.pty_id.clone())
                    .ok_or_else(|| JsonRpcError {
                        code: -32602,
                        message: format!("Pane {} not found or has no PTY", id),
                    })?
            };

            let buf = output_buf.lock();
            let text = buf.get_text(&pty_id).unwrap_or_default();
            // Return last max_bytes worth of text
            let output = if text.len() > max_bytes {
                text[text.len() - max_bytes..].to_string()
            } else {
                text
            };
            Ok(serde_json::json!({"output": output}))
        }

        "pane.split" => {
            let id = get_string_param(&params, "pane_id")?;
            let direction = params
                .as_ref()
                .and_then(|p| p.get("direction"))
                .and_then(|v| v.as_str())
                .unwrap_or("right");

            let orientation = match direction {
                "down" | "vertical" => Orientation::Vertical,
                _ => Orientation::Horizontal,
            };

            let new_pane_id = {
                let mut mux = mux.lock();
                mux.split_pane(&id, orientation).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?
            };

            // Emit event so frontend spawns PTY for the new pane
            let _ = app.emit("mux-state-changed", ());

            Ok(serde_json::json!({"new_pane_id": new_pane_id}))
        }

        "pane.rename" => {
            let id = get_string_param(&params, "pane_id")?;
            let title = get_string_param(&params, "title")?;
            {
                let mut mux = mux.lock();
                mux.rename_pane(&id, &title).map_err(|e| JsonRpcError {
                    code: -32602,
                    message: e,
                })?;
            }
            let _ = app.emit("pane-title-pinned", serde_json::json!({
                "pane_id": id,
                "title": title,
            }));
            let _ = app.emit("mux-state-changed", ());
            Ok(serde_json::json!({"ok": true}))
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
            let _ = app.emit("mux-state-changed", ());
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
            let _ = app.emit("notification-created", serde_json::json!({
                "id": id,
                "title": emit_title,
                "body": emit_body,
                "level": emit_level,
                "pane_id": emit_pane,
                "workspace_id": serde_json::Value::Null,
            }));

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
