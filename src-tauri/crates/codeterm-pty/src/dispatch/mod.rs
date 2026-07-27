//! Unified command dispatch: one `Command` variant per operation, one `dispatch()`
//! entry point. REST, Socket, Tauri IPC, and CLI adapters deserialize into Command
//! and call dispatch — no duplicated glue per transport.

use serde::{Deserialize, Serialize};

/// Abstraction over the app-side context that dispatch commands execute against.
/// Defined group-side; implemented app-side (ServiceContext).
pub trait CommandPort: Send + Sync {
    fn mark_user_activity(&self);
    fn mux_snapshot(&self) -> crate::multiplexer::state::MultiplexerSnapshot;
    fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String>;
    fn read_pane_output(
        &self,
        pane_id: &str,
        offset: Option<usize>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
        full: bool,
    ) -> Result<serde_json::Value, String>;
    fn read_pane_screen(&self, pane_id: &str, formatted: bool)
        -> Result<serde_json::Value, String>;
    fn close_pane_guarded(&self, caller: Option<&str>, pane_id: &str) -> Result<(), String>;
    fn focus_pane(&self, pane_id: &str) -> Result<(), String>;
    fn rename_pane(&self, pane_id: &str, title: &str) -> Result<(), String>;
    fn set_pane_description(&self, pane_id: &str, desc: &str) -> Result<(), String>;
    fn create_workspace(
        &self,
        name: String,
        cwd: Option<String>,
        activate: bool,
    ) -> crate::multiplexer::state::WorkspaceInfo;
    fn rename_workspace(&self, ws_id: &str, name: &str) -> Result<(), String>;
    fn close_workspace_guarded(&self, caller: Option<&str>, ws_id: &str) -> Result<(), String>;
    fn list_workspaces(&self) -> Vec<crate::multiplexer::state::WorkspaceInfo>;
    fn list_recently_closed(&self, ws_id: &str) -> Vec<crate::multiplexer::types::ClosedPane>;
    fn reopen_closed_pane(
        &self,
        ws_id: &str,
        index: Option<usize>,
    ) -> Result<(String, String), String>;
    fn reopen_closed_group(&self, group_id: &str) -> Result<(String, String, String), String>;
    fn resize_pane(&self, pane_id: &str, cols: u16, rows: u16) -> Result<(), String>;
    fn kill_pane_pty(&self, pane_id: &str) -> Result<(), String>;
    fn list_agent_providers(&self) -> Vec<serde_json::Value>;
}

/// Unified command enum — every operation CodeTerm supports.
/// Start with 5 operations, migrate the rest incrementally.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Health check / system identify
    Health,

    /// List all panes with metadata
    PaneList,

    /// List compact pane summaries scoped to one workspace or the focused workspace.
    PaneSummary {
        #[serde(default)]
        workspace_id: Option<String>,
        #[serde(default)]
        focused_workspace: bool,
        #[serde(default)]
        agents_only: bool,
    },

    /// Inspect selected panes with the full pane payload.
    PaneInspect { pane_ids: Vec<String> },

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

    /// Close a pane and kill its PTY. `caller_pane_id` is the pane making the
    /// request (CODETERM_PANE_ID), used to refuse coordinator self-close.
    PaneClose {
        pane_id: String,
        #[serde(default)]
        caller_pane_id: Option<String>,
    },

    /// Focus a pane (move keyboard focus).
    PaneFocus { pane_id: String },

    /// Rename a pane's title (pinned until the user clears it).
    PaneRename { pane_id: String, title: String },

    /// Set or clear a pane's short description.
    PaneDescribe {
        pane_id: String,
        description: String,
    },

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

    /// Close a workspace and kill all of its PTYs. `caller_pane_id` refuses a
    /// coordinator closing the workspace it runs in.
    WorkspaceClose {
        workspace_id: String,
        #[serde(default)]
        caller_pane_id: Option<String>,
    },

    /// List all workspaces with their metadata.
    WorkspaceList,

    /// List a workspace's recently-closed panes (newest first).
    WorkspaceRecentlyClosed { workspace_id: String },

    /// Reopen a recently-closed pane (browser-style). `index` selects the ring
    /// entry (newest = 0, default).
    WorkspaceReopenClosedPane {
        workspace_id: String,
        #[serde(default)]
        index: Option<usize>,
    },

    /// Reopen a whole closed group (single / cascade subtree / workspace) by id.
    WorkspaceReopenClosedGroup { group_id: String },

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

/// Offloads a blocking dispatch call off the async runtime.
pub async fn dispatch_blocking(
    cmd: Command,
    ctx: std::sync::Arc<dyn CommandPort>,
) -> Result<CommandResult, String> {
    tokio::task::spawn_blocking(move || dispatch(&cmd, ctx.as_ref()))
        .await
        .map_err(|e| e.to_string())?
}

pub fn dispatch(cmd: &Command, ctx: &dyn CommandPort) -> Result<CommandResult, String> {
    if is_mutating_user_command(cmd) {
        ctx.mark_user_activity();
    }
    match cmd {
        Command::Health => cmd_health(),
        Command::PaneList => cmd_pane_list(ctx),
        Command::PaneSummary {
            workspace_id,
            focused_workspace,
            agents_only,
        } => cmd_pane_summary(
            ctx,
            workspace_id.as_deref(),
            *focused_workspace,
            *agents_only,
        ),
        Command::PaneInspect { pane_ids } => cmd_pane_inspect(ctx, pane_ids),
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
        Command::PaneClose {
            pane_id,
            caller_pane_id,
        } => cmd_pane_close(ctx, pane_id, caller_pane_id.as_deref()),
        Command::PaneFocus { pane_id } => cmd_pane_focus(ctx, pane_id),
        Command::PaneRename { pane_id, title } => cmd_pane_rename(ctx, pane_id, title),
        Command::PaneDescribe {
            pane_id,
            description,
        } => cmd_pane_describe(ctx, pane_id, description),
        Command::ListAgentProviders => cmd_list_agent_providers(ctx),
        Command::WorkspaceCreate {
            name,
            cwd,
            activate,
        } => cmd_workspace_create(ctx, name.clone(), cwd.clone(), activate.unwrap_or(false)),
        Command::WorkspaceRename { workspace_id, name } => {
            cmd_workspace_rename(ctx, workspace_id, name)
        }
        Command::WorkspaceClose {
            workspace_id,
            caller_pane_id,
        } => cmd_workspace_close(ctx, workspace_id, caller_pane_id.as_deref()),
        Command::WorkspaceList => cmd_workspace_list(ctx),
        Command::WorkspaceRecentlyClosed { workspace_id } => {
            cmd_workspace_recently_closed(ctx, workspace_id)
        }
        Command::WorkspaceReopenClosedPane {
            workspace_id,
            index,
        } => cmd_workspace_reopen_closed_pane(ctx, workspace_id, *index),
        Command::WorkspaceReopenClosedGroup { group_id } => {
            cmd_workspace_reopen_closed_group(ctx, group_id)
        }
        Command::PtyResize {
            pane_id,
            cols,
            rows,
        } => cmd_pty_resize(ctx, pane_id, *cols, *rows),
        Command::PtyKill { pane_id } => cmd_pty_kill(ctx, pane_id),
    }
}

// ── Command implementations ─────────────────────────────────────────

fn is_mutating_user_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::SendText { .. }
            | Command::PaneClose { .. }
            | Command::PaneFocus { .. }
            | Command::PaneRename { .. }
            | Command::PaneDescribe { .. }
            | Command::WorkspaceCreate { .. }
            | Command::WorkspaceRename { .. }
            | Command::WorkspaceClose { .. }
            | Command::WorkspaceReopenClosedPane { .. }
            | Command::WorkspaceReopenClosedGroup { .. }
            | Command::PtyResize { .. }
            | Command::PtyKill { .. }
    )
}

fn cmd_health() -> Result<CommandResult, String> {
    Ok(CommandResult::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("BUILD_HASH").unwrap_or("dev"),
        "name": "CodeTerm",
        "platform": std::env::consts::OS,
    })))
}

fn cmd_pane_list(ctx: &dyn CommandPort) -> Result<CommandResult, String> {
    let snapshot = ctx.mux_snapshot();
    let panes = build_pane_list_json(&snapshot);
    Ok(CommandResult::Json(serde_json::json!(panes)))
}

fn cmd_pane_summary(
    ctx: &dyn CommandPort,
    workspace_id: Option<&str>,
    focused_workspace: bool,
    agents_only: bool,
) -> Result<CommandResult, String> {
    let snapshot = ctx.mux_snapshot();
    Ok(CommandResult::Json(serde_json::json!(
        build_pane_summary_json(&snapshot, workspace_id, focused_workspace, agents_only,)
    )))
}

fn cmd_pane_inspect(ctx: &dyn CommandPort, pane_ids: &[String]) -> Result<CommandResult, String> {
    if pane_ids.is_empty() {
        return Err("pane.inspect requires at least one Tab id".into());
    }
    let snapshot = ctx.mux_snapshot();
    let requested: std::collections::HashSet<&str> = pane_ids.iter().map(String::as_str).collect();
    let panes = build_pane_list_json(&snapshot)
        .into_iter()
        .filter(|pane| {
            pane.get("id")
                .and_then(|value| value.as_str())
                .is_some_and(|id| requested.contains(id))
        })
        .collect::<Vec<_>>();
    if panes.len() != requested.len() {
        return Err("one or more requested Tabs were not found".into());
    }
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
            let tab_title = snapshot
                .panes
                .get(&tab.pane_id)
                .map(|pane| pane.title.clone())
                .unwrap_or_else(|| tab.pane_id.chars().take(8).collect());
            tab_index.insert(
                tab.pane_id.clone(),
                (tab.id.clone(), tab_title, ws.id.clone(), ws.name.clone()),
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

pub(crate) fn build_pane_summary_json(
    snapshot: &crate::multiplexer::state::MultiplexerSnapshot,
    workspace_id: Option<&str>,
    focused_workspace: bool,
    agents_only: bool,
) -> Vec<serde_json::Value> {
    let mut tab_index: std::collections::HashMap<&str, (&str, &str, &str)> =
        std::collections::HashMap::new();
    for workspace in &snapshot.workspaces {
        for tab in &workspace.tabs {
            tab_index.insert(
                tab.pane_id.as_str(),
                (
                    tab.id.as_str(),
                    workspace.id.as_str(),
                    workspace.name.as_str(),
                ),
            );
        }
    }
    let focused_workspace_id = snapshot
        .focused_pane_id
        .as_deref()
        .and_then(|pane_id| tab_index.get(pane_id).map(|(_, id, _)| *id));
    let selected_workspace =
        workspace_id.or_else(|| focused_workspace.then_some(focused_workspace_id).flatten());
    let focused_id = snapshot.focused_pane_id.as_deref();
    snapshot
        .panes
        .values()
        .filter(|pane| !pane.hidden)
        .filter(|pane| !agents_only || pane.agent_type.is_some())
        .filter(|pane| {
            selected_workspace.is_none_or(|selected| {
                tab_index
                    .get(pane.id.as_str())
                    .is_some_and(|(_, workspace, _)| *workspace == selected)
            })
        })
        .filter_map(|pane| {
            let (tab_id, workspace_id, workspace_name) = tab_index.get(pane.id.as_str())?;
            let backing_kind = match &pane.backing {
                crate::multiplexer::types::PaneBacking::Pty => "pty",
                crate::multiplexer::types::PaneBacking::ChatBackend { .. } => "chat_backend",
            };
            Some(serde_json::json!({
                "id": pane.id,
                "tab_id": tab_id,
                "title": pane.title,
                "description": pane.description,
                "workspace_id": workspace_id,
                "workspace_name": workspace_name,
                "parent_pane_id": pane.parent_pane_id,
                "agent_type": pane.agent_type,
                "agent_session_id": pane.agent_session_id,
                "agent_session_model": pane.agent_session_model,
                "boot_stage": pane.boot_stage,
                "status": pane.agent_state.as_ref().map(|state| state.state),
                "role": pane.role,
                "backing_kind": backing_kind,
                "focused": focused_id == Some(pane.id.as_str()),
            }))
        })
        .collect()
}

fn cmd_send_text(
    ctx: &dyn CommandPort,
    pane_id: &str,
    text: &str,
    enter: bool,
) -> Result<CommandResult, String> {
    ctx.send_text(pane_id, text, enter)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_get_output(
    ctx: &dyn CommandPort,
    pane_id: &str,
    offset: Option<usize>,
    max_bytes: Option<usize>,
    strip_ansi: bool,
    full: bool,
) -> Result<CommandResult, String> {
    let json = ctx.read_pane_output(pane_id, offset, max_bytes, strip_ansi, full)?;
    Ok(CommandResult::Json(json))
}

fn cmd_get_text(
    ctx: &dyn CommandPort,
    pane_id: &str,
    formatted: bool,
) -> Result<CommandResult, String> {
    let json = ctx.read_pane_screen(pane_id, formatted)?;
    Ok(CommandResult::Json(json))
}

fn cmd_pane_close(
    ctx: &dyn CommandPort,
    pane_id: &str,
    caller_pane_id: Option<&str>,
) -> Result<CommandResult, String> {
    ctx.close_pane_guarded(caller_pane_id, pane_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pane_focus(ctx: &dyn CommandPort, pane_id: &str) -> Result<CommandResult, String> {
    ctx.focus_pane(pane_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pane_rename(
    ctx: &dyn CommandPort,
    pane_id: &str,
    title: &str,
) -> Result<CommandResult, String> {
    ctx.rename_pane(pane_id, title)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pane_describe(
    ctx: &dyn CommandPort,
    pane_id: &str,
    description: &str,
) -> Result<CommandResult, String> {
    ctx.set_pane_description(pane_id, description)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_workspace_create(
    ctx: &dyn CommandPort,
    name: String,
    cwd: Option<String>,
    activate: bool,
) -> Result<CommandResult, String> {
    let ws = ctx.create_workspace(name, cwd, activate);
    let json = serde_json::to_value(&ws).map_err(|e| format!("serialize WorkspaceInfo: {}", e))?;
    Ok(CommandResult::Json(json))
}

fn cmd_workspace_rename(
    ctx: &dyn CommandPort,
    workspace_id: &str,
    name: &str,
) -> Result<CommandResult, String> {
    ctx.rename_workspace(workspace_id, name)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_workspace_close(
    ctx: &dyn CommandPort,
    workspace_id: &str,
    caller_pane_id: Option<&str>,
) -> Result<CommandResult, String> {
    ctx.close_workspace_guarded(caller_pane_id, workspace_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_workspace_list(ctx: &dyn CommandPort) -> Result<CommandResult, String> {
    let workspaces = ctx.list_workspaces();
    let json =
        serde_json::to_value(&workspaces).map_err(|e| format!("serialize workspaces: {}", e))?;
    Ok(CommandResult::Json(json))
}

fn cmd_workspace_recently_closed(
    ctx: &dyn CommandPort,
    workspace_id: &str,
) -> Result<CommandResult, String> {
    let closed = ctx.list_recently_closed(workspace_id);
    let json =
        serde_json::to_value(&closed).map_err(|e| format!("serialize recently_closed: {}", e))?;
    Ok(CommandResult::Json(json))
}

fn cmd_workspace_reopen_closed_pane(
    ctx: &dyn CommandPort,
    workspace_id: &str,
    index: Option<usize>,
) -> Result<CommandResult, String> {
    let (tab_id, pane_id) = ctx.reopen_closed_pane(workspace_id, index)?;
    Ok(CommandResult::Json(serde_json::json!({
        "tab_id": tab_id,
        "pane_id": pane_id,
    })))
}

fn cmd_workspace_reopen_closed_group(
    ctx: &dyn CommandPort,
    group_id: &str,
) -> Result<CommandResult, String> {
    let (workspace_id, tab_id, pane_id) = ctx.reopen_closed_group(group_id)?;
    Ok(CommandResult::Json(serde_json::json!({
        "workspace_id": workspace_id,
        "tab_id": tab_id,
        "pane_id": pane_id,
    })))
}

fn cmd_pty_resize(
    ctx: &dyn CommandPort,
    pane_id: &str,
    cols: u16,
    rows: u16,
) -> Result<CommandResult, String> {
    ctx.resize_pane(pane_id, cols, rows)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pty_kill(ctx: &dyn CommandPort, pane_id: &str) -> Result<CommandResult, String> {
    ctx.kill_pane_pty(pane_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_list_agent_providers(ctx: &dyn CommandPort) -> Result<CommandResult, String> {
    Ok(CommandResult::Json(serde_json::Value::Array(
        ctx.list_agent_providers(),
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
                assert_eq!(title, "My Tab");
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
                caller_pane_id: None,
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
                caller_pane_id: None,
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
    // Tests stay on the pure helper so both `Tab list` and `Tab find` keep full workspace context.

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
            .expect("default Tab present in list");
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
    fn pane_summary_is_workspace_scoped_and_excludes_full_payload_fields() {
        use crate::multiplexer::state::MultiplexerState;
        let (state, initial_pane) = MultiplexerState::new();
        let snapshot = state.snapshot();
        let workspace = snapshot.workspaces.first().expect("default workspace");
        let summaries = build_pane_summary_json(&snapshot, Some(&workspace.id), false, false);
        assert_eq!(summaries.len(), 1);
        let pane = summaries.first().expect("summary Tab");
        assert_eq!(pane["id"].as_str(), Some(initial_pane.as_str()));
        assert_eq!(pane["workspace_id"].as_str(), Some(workspace.id.as_str()));
        assert!(pane.get("cwd").is_none());
        assert!(pane.get("session_binding_nonce").is_none());
        assert!(pane.get("agent_state").is_none());
        assert!(pane.get("model_fallback").is_none());
    }

    #[test]
    fn pane_summary_command_deserializes_compact_scope_options() {
        let command: Command = serde_json::from_str(
            r#"{"cmd":"pane_summary","workspace_id":"ws_1","focused_workspace":false,"agents_only":true}"#,
        )
        .expect("Tab summary command");
        assert!(matches!(
            command,
            Command::PaneSummary {
                workspace_id: Some(workspace_id),
                focused_workspace: false,
                agents_only: true,
            } if workspace_id == "ws_1"
        ));
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
            "focused Tab must have focused=true"
        );
        assert!(
            !get_focused(&panes, &pane_b),
            "non-focused Tab must have focused=false"
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
                "Tab list must derive focused=true from the snapshot being projected"
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
