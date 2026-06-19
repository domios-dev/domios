//! Unified command dispatch: one `Command` variant per operation, one `dispatch()`
//! entry point. REST, Socket, Tauri IPC, and CLI adapters deserialize into Command
//! and call dispatch — no duplicated glue per transport.

use crate::service::ServiceContext;
use serde::{Deserialize, Serialize};

/// Unified command enum — every operation CodeTerm supports.
/// Start with 5 operations, migrate the rest incrementally.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Health check / system identify
    Health,

    /// List all panes with metadata
    PaneList,

    /// Send text to a pane's PTY
    SendText {
        pane_id: String,
        text: String,
        #[serde(default = "default_true")]
        enter: bool,
    },

    /// Get pane output (terminal buffer)
    GetOutput {
        pane_id: String,
        #[serde(default)]
        offset: Option<usize>,
        #[serde(default)]
        max_bytes: Option<usize>,
        #[serde(default)]
        strip_ansi: bool,
        #[serde(default)]
        full: bool,
    },

    /// Get visible pane text (VT100-parsed terminal screen).
    /// When `formatted` is true, the string contains ANSI escape codes
    /// preserving colors, cursor position, and cursor visibility — used
    /// for full state restore on pane remount. Default returns plain text.
    GetText {
        pane_id: String,
        #[serde(default)]
        formatted: bool,
    },

    /// Close a pane and kill its PTY.
    PaneClose { pane_id: String },

    /// Focus a pane (move keyboard focus).
    PaneFocus { pane_id: String },

    /// Rename a pane's title (pinned until the user clears it).
    PaneRename { pane_id: String, title: String },

    /// List all registered agent providers as `ProviderDescriptor`s.
    /// First dispatch command for the plugin self-presentation system (Chunk B.3).
    ListAgentProviders,

    /// Create a workspace, optionally with a starting cwd inherited by spawned agents.
    WorkspaceCreate {
        name: String,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        activate: Option<bool>,
    },

    /// Rename an existing workspace.
    WorkspaceRename { workspace_id: String, name: String },

    /// Close a workspace and kill all of its PTYs.
    WorkspaceClose { workspace_id: String },

    /// List all workspaces with their metadata.
    WorkspaceList,

    /// Resize the PTY backing a pane.
    PtyResize {
        pane_id: String,
        cols: u16,
        rows: u16,
    },

    /// Kill the PTY process for a pane without removing the pane from the mux.
    /// Compare to `PaneClose`, which both stops the process and drops the pane.
    PtyKill { pane_id: String },
}

fn default_true() -> bool {
    true
}

/// Unified result type for all commands.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CommandResult {
    Json(serde_json::Value),
}

impl CommandResult {
    pub fn into_json(self) -> serde_json::Value {
        match self {
            CommandResult::Json(v) => v,
        }
    }
}

/// Dispatch a command against ServiceContext. Pure logic, no transport concerns.
// Run a synchronous (possibly blocking) dispatch off the async runtime so heavy commands never starve Tauri IPC.
pub async fn dispatch_blocking(
    cmd: Command,
    svc: crate::service::ServiceState,
) -> Result<CommandResult, String> {
    tokio::task::spawn_blocking(move || dispatch(&cmd, svc.as_ref()))
        .await
        .map_err(|e| e.to_string())?
}

pub fn dispatch(cmd: &Command, ctx: &ServiceContext) -> Result<CommandResult, String> {
    match cmd {
        Command::Health => cmd_health(),
        Command::PaneList => cmd_pane_list(ctx),
        Command::SendText {
            pane_id,
            text,
            enter,
        } => cmd_send_text(ctx, pane_id, text, *enter),
        Command::GetOutput {
            pane_id,
            offset,
            max_bytes,
            strip_ansi,
            full,
        } => cmd_get_output(ctx, pane_id, *offset, *max_bytes, *strip_ansi, *full),
        Command::GetText { pane_id, formatted } => cmd_get_text(ctx, pane_id, *formatted),
        Command::PaneClose { pane_id } => cmd_pane_close(ctx, pane_id),
        Command::PaneFocus { pane_id } => cmd_pane_focus(ctx, pane_id),
        Command::PaneRename { pane_id, title } => cmd_pane_rename(ctx, pane_id, title),
        Command::ListAgentProviders => cmd_list_agent_providers(ctx),
        Command::WorkspaceCreate {
            name,
            cwd,
            activate,
        } => cmd_workspace_create(ctx, name.clone(), cwd.clone(), activate.unwrap_or(false)),
        Command::WorkspaceRename { workspace_id, name } => {
            cmd_workspace_rename(ctx, workspace_id, name)
        }
        Command::WorkspaceClose { workspace_id } => cmd_workspace_close(ctx, workspace_id),
        Command::WorkspaceList => cmd_workspace_list(ctx),
        Command::PtyResize {
            pane_id,
            cols,
            rows,
        } => cmd_pty_resize(ctx, pane_id, *cols, *rows),
        Command::PtyKill { pane_id } => cmd_pty_kill(ctx, pane_id),
    }
}

// ── Command implementations ─────────────────────────────────────────

fn cmd_health() -> Result<CommandResult, String> {
    Ok(CommandResult::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "build": env!("BUILD_HASH"),
        "name": "CodeTerm",
        "platform": std::env::consts::OS,
    })))
}

fn cmd_pane_list(ctx: &ServiceContext) -> Result<CommandResult, String> {
    let mux = ctx.mux.write();
    let snapshot = mux.snapshot();
    let panes = build_pane_list_json(&snapshot);
    Ok(CommandResult::Json(serde_json::json!(panes)))
}

/// Enrich pane JSON with tab + workspace identifiers.
pub(crate) fn build_pane_list_json(
    snapshot: &crate::multiplexer::state::MultiplexerSnapshot,
) -> Vec<serde_json::Value> {
    let mut tab_index: std::collections::HashMap<String, (String, String, String, String)> =
        std::collections::HashMap::new();
    for ws in &snapshot.workspaces {
        for tab in &ws.tabs {
            tab_index.insert(
                tab.pane_id.clone(),
                (
                    tab.id.clone(),
                    tab.title.clone(),
                    ws.id.clone(),
                    ws.name.clone(),
                ),
            );
        }
    }
    let focused_id = snapshot.focused_pane_id.as_deref();
    snapshot
        .panes
        .values()
        .filter(|p| !p.hidden)
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert(
                    "focused".into(),
                    serde_json::Value::Bool(focused_id == Some(p.id.as_str())),
                );
                if let Some((tab_id, tab_title, ws_id, ws_name)) = tab_index.get(&p.id) {
                    obj.insert("tab_id".into(), serde_json::Value::String(tab_id.clone()));
                    obj.insert(
                        "tab_title".into(),
                        serde_json::Value::String(tab_title.clone()),
                    );
                    obj.insert(
                        "workspace_id".into(),
                        serde_json::Value::String(ws_id.clone()),
                    );
                    obj.insert(
                        "workspace_name".into(),
                        serde_json::Value::String(ws_name.clone()),
                    );
                }
            }
            v
        })
        .collect()
}

fn cmd_send_text(
    ctx: &ServiceContext,
    pane_id: &str,
    text: &str,
    enter: bool,
) -> Result<CommandResult, String> {
    ctx.send_text(pane_id, text, enter)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_get_output(
    ctx: &ServiceContext,
    pane_id: &str,
    offset: Option<usize>,
    max_bytes: Option<usize>,
    strip_ansi: bool,
    full: bool,
) -> Result<CommandResult, String> {
    // Routed through the PaneTransport seam: PTY panes read the output buffer,
    // chatBackend panes return their transcript (B1.2 [T]).
    let json = ctx
        .pane_transport(pane_id)?
        .read_output(offset, max_bytes, strip_ansi, full)?;
    Ok(CommandResult::Json(json))
}

fn cmd_get_text(
    ctx: &ServiceContext,
    pane_id: &str,
    formatted: bool,
) -> Result<CommandResult, String> {
    // PaneTransport seam: PTY panes read screen text, chatBackend panes return
    // their transcript (B1.2 [T]).
    let json = ctx.pane_transport(pane_id)?.read_screen(formatted)?;
    Ok(CommandResult::Json(json))
}

fn cmd_pane_close(ctx: &ServiceContext, pane_id: &str) -> Result<CommandResult, String> {
    ctx.close_pane(pane_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pane_focus(ctx: &ServiceContext, pane_id: &str) -> Result<CommandResult, String> {
    ctx.focus_pane(pane_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pane_rename(
    ctx: &ServiceContext,
    pane_id: &str,
    title: &str,
) -> Result<CommandResult, String> {
    ctx.rename_pane(pane_id, title)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_workspace_create(
    ctx: &ServiceContext,
    name: String,
    cwd: Option<String>,
    activate: bool,
) -> Result<CommandResult, String> {
    let ws = ctx.create_workspace(name, cwd, activate);
    let json = serde_json::to_value(&ws).map_err(|e| format!("serialize WorkspaceInfo: {}", e))?;
    Ok(CommandResult::Json(json))
}

fn cmd_workspace_rename(
    ctx: &ServiceContext,
    workspace_id: &str,
    name: &str,
) -> Result<CommandResult, String> {
    ctx.rename_workspace(workspace_id, name)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_workspace_close(ctx: &ServiceContext, workspace_id: &str) -> Result<CommandResult, String> {
    ctx.close_workspace(workspace_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_workspace_list(ctx: &ServiceContext) -> Result<CommandResult, String> {
    let workspaces = ctx.list_workspaces();
    let json =
        serde_json::to_value(&workspaces).map_err(|e| format!("serialize workspaces: {}", e))?;
    Ok(CommandResult::Json(json))
}

fn cmd_pty_resize(
    ctx: &ServiceContext,
    pane_id: &str,
    cols: u16,
    rows: u16,
) -> Result<CommandResult, String> {
    ctx.resize_pane(pane_id, cols, rows)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pty_kill(ctx: &ServiceContext, pane_id: &str) -> Result<CommandResult, String> {
    ctx.kill_pane_pty(pane_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_list_agent_providers(ctx: &ServiceContext) -> Result<CommandResult, String> {
    Ok(CommandResult::Json(serde_json::Value::Array(
        crate::api::agent_routes::list_agent_provider_values(&ctx.provider_registry),
    )))
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_returns_ok() {
        let result = cmd_health().unwrap();
        let json = result.into_json();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["name"], "CodeTerm");
        assert!(json["version"].is_string());
        assert!(json["platform"].is_string());
    }

    #[test]
    fn test_command_deserialize_health() {
        let json = r#"{"cmd": "health"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::Health));
    }

    #[test]
    fn test_command_deserialize_send_text() {
        let json = r#"{"cmd": "send_text", "pane_id": "abc", "text": "hello"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::SendText {
                pane_id,
                text,
                enter,
            } => {
                assert_eq!(pane_id, "abc");
                assert_eq!(text, "hello");
                assert!(enter); // default true
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_send_text_enter_false() {
        let json = r#"{"cmd": "send_text", "pane_id": "abc", "text": "hello", "enter": false}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::SendText { enter, .. } => assert!(!enter),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_get_output() {
        let json = r#"{"cmd": "get_output", "pane_id": "abc", "strip_ansi": true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetOutput {
                pane_id,
                strip_ansi,
                full,
                offset,
                max_bytes,
            } => {
                assert_eq!(pane_id, "abc");
                assert!(strip_ansi);
                assert!(!full); // default false
                assert!(offset.is_none());
                assert!(max_bytes.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_get_text() {
        let json = r#"{"cmd": "get_text", "pane_id": "abc", "formatted": true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetText { pane_id, formatted } => {
                assert_eq!(pane_id, "abc");
                assert!(formatted);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_pane_list() {
        let json = r#"{"cmd": "pane_list"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::PaneList));
    }

    #[test]
    fn test_command_result_into_json() {
        let result = CommandResult::Json(serde_json::json!({"test": true}));
        let json = result.into_json();
        assert_eq!(json["test"], true);
    }

    // ── Command enum round-trip tests ───────────────────────────────

    #[test]
    fn pane_close_deserialize() {
        let cmd: Command =
            serde_json::from_str(r#"{"cmd": "pane_close", "pane_id": "abc"}"#).unwrap();
        assert!(matches!(cmd, Command::PaneClose { .. }));
    }

    #[test]
    fn pane_focus_deserialize() {
        let cmd: Command =
            serde_json::from_str(r#"{"cmd": "pane_focus", "pane_id": "abc"}"#).unwrap();
        assert!(matches!(cmd, Command::PaneFocus { .. }));
    }

    #[test]
    fn pane_rename_deserialize() {
        let json = r#"{"cmd": "pane_rename", "pane_id": "abc", "title": "My Pane"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::PaneRename { pane_id, title } => {
                assert_eq!(pane_id, "abc");
                assert_eq!(title, "My Pane");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn get_output_deserialize_full_params() {
        let json = r#"{"cmd":"get_output","pane_id":"p","offset":100,"max_bytes":4096,"strip_ansi":true,"full":true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetOutput {
                pane_id,
                offset,
                max_bytes,
                strip_ansi,
                full,
            } => {
                assert_eq!(pane_id, "p");
                assert_eq!(offset, Some(100));
                assert_eq!(max_bytes, Some(4096));
                assert!(strip_ansi);
                assert!(full);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn next_char_boundary_handles_multibyte_offsets() {
        use crate::service::pane_transport::next_char_boundary;
        let s = "abкd";
        assert_eq!(next_char_boundary(s, 3), 4);
        assert_eq!(&s[next_char_boundary(s, 3)..], "d");
    }

    #[test]
    fn command_round_trips_through_json() {
        // Validates that every variant we migrated survives a JSON round trip with tag-based dispatch.
        let commands = vec![
            Command::Health,
            Command::PaneList,
            Command::SendText {
                pane_id: "p".into(),
                text: "hi".into(),
                enter: true,
            },
            Command::GetOutput {
                pane_id: "p".into(),
                offset: None,
                max_bytes: None,
                strip_ansi: false,
                full: false,
            },
            Command::GetText {
                pane_id: "p".into(),
                formatted: false,
            },
            Command::PaneClose {
                pane_id: "p".into(),
            },
            Command::PaneFocus {
                pane_id: "p".into(),
            },
            Command::PaneRename {
                pane_id: "p".into(),
                title: "t".into(),
            },
            Command::ListAgentProviders,
            Command::WorkspaceCreate {
                name: "w".into(),
                cwd: None,
                activate: None,
            },
            Command::WorkspaceRename {
                workspace_id: "w".into(),
                name: "n".into(),
            },
            Command::WorkspaceClose {
                workspace_id: "w".into(),
            },
            Command::WorkspaceList,
            Command::PtyResize {
                pane_id: "p".into(),
                cols: 80,
                rows: 24,
            },
            Command::PtyKill {
                pane_id: "p".into(),
            },
        ];
        for cmd in commands {
            let json = serde_json::to_value(&cmd).unwrap();
            let roundtrip: Command = serde_json::from_value(json).unwrap();
            // Same variant tag after roundtrip.
            assert_eq!(
                std::mem::discriminant(&cmd),
                std::mem::discriminant(&roundtrip),
            );
        }
    }

    #[test]
    fn workspace_create_deserialize() {
        let json = r#"{"cmd":"workspace_create","name":"proj","cwd":"/tmp"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::WorkspaceCreate {
                name,
                cwd,
                activate,
            } => {
                assert_eq!(name, "proj");
                assert_eq!(cwd.as_deref(), Some("/tmp"));
                assert!(activate.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn workspace_create_deserialize_no_cwd() {
        let json = r#"{"cmd":"workspace_create","name":"proj"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::WorkspaceCreate {
                name,
                cwd,
                activate,
            } => {
                assert_eq!(name, "proj");
                assert!(cwd.is_none());
                assert!(activate.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn workspace_create_deserialize_with_activate() {
        let json = r#"{"cmd":"workspace_create","name":"proj","activate":true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::WorkspaceCreate { activate, .. } => assert_eq!(activate, Some(true)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn workspace_rename_deserialize() {
        let json = r#"{"cmd":"workspace_rename","workspace_id":"w1","name":"new"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::WorkspaceRename { workspace_id, name } => {
                assert_eq!(workspace_id, "w1");
                assert_eq!(name, "new");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn workspace_close_deserialize() {
        let cmd: Command =
            serde_json::from_str(r#"{"cmd":"workspace_close","workspace_id":"w1"}"#).unwrap();
        assert!(matches!(cmd, Command::WorkspaceClose { .. }));
    }

    #[test]
    fn workspace_list_deserialize() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"workspace_list"}"#).unwrap();
        assert!(matches!(cmd, Command::WorkspaceList));
    }

    #[test]
    fn pty_resize_deserialize() {
        let json = r#"{"cmd":"pty_resize","pane_id":"p","cols":120,"rows":40}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::PtyResize {
                pane_id,
                cols,
                rows,
            } => {
                assert_eq!(pane_id, "p");
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn pty_kill_deserialize() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"pty_kill","pane_id":"p"}"#).unwrap();
        assert!(matches!(cmd, Command::PtyKill { .. }));
    }

    #[test]
    fn list_agent_providers_deserialize() {
        let cmd: Command = serde_json::from_str(r#"{"cmd": "list_agent_providers"}"#).unwrap();
        assert!(matches!(cmd, Command::ListAgentProviders));
    }

    // `cmd_pane_list` and `build_pane_list_json` enrich panes with sidebar labels.
    // Tests stay on the pure helper so both `pane list` and `pane find` keep full workspace context.

    #[test]
    fn pane_list_attaches_tab_title_and_workspace_name() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, initial_pane) = MultiplexerState::new();
        let snapshot = state.snapshot();
        let ws = snapshot.workspaces.first().expect("default ws").clone();
        let tab_id = state.find_tab_for_pane(&initial_pane).expect("default tab");
        state.rename_tab(&ws.id, &tab_id, "Dalton").unwrap();
        let snapshot = state.snapshot();
        let panes = build_pane_list_json(&snapshot);
        let pane = panes
            .iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(initial_pane.as_str()))
            .expect("default pane present in list");
        assert_eq!(
            pane.get("tab_title").and_then(|v| v.as_str()),
            Some("Dalton"),
            "tab_title must be attached so CLI can show the sidebar label",
        );
        assert_eq!(
            pane.get("tab_id").and_then(|v| v.as_str()),
            Some(tab_id.as_str()),
        );
        assert_eq!(
            pane.get("workspace_id").and_then(|v| v.as_str()),
            Some(ws.id.as_str()),
        );
        assert!(
            pane.get("workspace_name").and_then(|v| v.as_str()).is_some(),
            "workspace_name must be attached so user can disambiguate same-titled tabs across workspaces",
        );
    }

    #[test]
    fn pane_list_focused_field_tracks_mux_focused_pane_id() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, pane_a) = MultiplexerState::new();
        // Create a second pane.
        let pane_b = state.create_pane();
        // Focus pane_a explicitly.
        state.set_focused_pane(&pane_a).unwrap();
        let panes = build_pane_list_json(&state.snapshot());
        let get_focused = |panes: &[serde_json::Value], id: &str| {
            panes
                .iter()
                .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
                .and_then(|p| p.get("focused"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        assert!(
            get_focused(&panes, &pane_a),
            "focused pane must have focused=true"
        );
        assert!(
            !get_focused(&panes, &pane_b),
            "non-focused pane must have focused=false"
        );
        // Switch focus.
        state.set_focused_pane(&pane_b).unwrap();
        let panes = build_pane_list_json(&state.snapshot());
        assert!(
            !get_focused(&panes, &pane_a),
            "pane_a must be unfocused after switch"
        );
        assert!(
            get_focused(&panes, &pane_b),
            "pane_b must be focused after switch"
        );
    }

    #[test]
    fn pane_list_marks_exactly_one_focused_pane_from_each_snapshot() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, pane_a) = MultiplexerState::new();
        let pane_b = state.create_pane();

        for focused in [&pane_a, &pane_b] {
            state.set_focused_pane(focused).unwrap();
            let panes = build_pane_list_json(&state.snapshot());
            let focused_ids: Vec<&str> = panes
                .iter()
                .filter(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                .filter_map(|p| p.get("id").and_then(|v| v.as_str()))
                .collect();

            assert_eq!(
                focused_ids,
                vec![focused.as_str()],
                "pane list must derive focused=true from the snapshot being projected"
            );
        }
    }

    #[test]
    fn pane_list_keeps_tab_info_after_rename_to_fibonacci() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, visible) = MultiplexerState::new();
        let ws_id = state
            .snapshot()
            .workspaces
            .first()
            .expect("default ws")
            .id
            .clone();
        let tab_id = state.find_tab_for_pane(&visible).unwrap();
        state.rename_tab(&ws_id, &tab_id, "Fibonacci").unwrap();
        let snapshot = state.snapshot();
        let panes = build_pane_list_json(&snapshot);
        assert_eq!(panes.len(), 1);
        assert_eq!(
            panes[0].get("tab_title").and_then(|v| v.as_str()),
            Some("Fibonacci"),
        );
    }
}
