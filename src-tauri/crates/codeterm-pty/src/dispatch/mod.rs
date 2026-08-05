//! Unified command dispatch: one `Command` variant per operation, one `dispatch()`
//! entry point. REST, Socket, Tauri IPC, and CLI adapters deserialize into Command
//! and call dispatch — no duplicated glue per transport.

use serde::{Deserialize, Serialize};

/// Abstraction over the app-side context that dispatch commands execute against.
/// Defined group-side; implemented app-side (ServiceContext).
pub trait CommandPort: Send + Sync {
    fn mark_user_activity(&self);
    fn mux_snapshot(&self) -> crate::multiplexer::state::MultiplexerSnapshot;
    fn send_text(&self, tab_id: &str, text: &str) -> Result<(), String>;
    fn read_tab_output(
        &self,
        tab_id: &str,
        offset: Option<usize>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
        full: bool,
    ) -> Result<serde_json::Value, String>;
    fn read_tab_screen(&self, tab_id: &str, formatted: bool) -> Result<serde_json::Value, String>;
    fn close_tab_guarded(&self, caller: Option<&str>, tab_id: &str) -> Result<(), String>;
    fn focus_tab(&self, tab_id: &str) -> Result<(), String>;
    fn rename_tab(&self, tab_id: &str, title: &str) -> Result<(), String>;
    fn set_tab_description(&self, tab_id: &str, desc: &str) -> Result<(), String>;
    fn create_workspace(
        &self,
        name: String,
        cwd: Option<String>,
        activate: bool,
    ) -> crate::multiplexer::state::WorkspaceInfo;
    fn rename_workspace(&self, ws_id: &str, name: &str) -> Result<(), String>;
    fn close_workspace_guarded(&self, caller: Option<&str>, ws_id: &str) -> Result<(), String>;
    fn list_workspaces(&self) -> Vec<crate::multiplexer::state::WorkspaceInfo>;
    fn list_recently_closed(&self, ws_id: &str) -> Vec<crate::multiplexer::types::ClosedTab>;
    fn reopen_closed_tab(&self, ws_id: &str, index: Option<usize>) -> Result<String, String>;
    fn reopen_closed_group(&self, group_id: &str) -> Result<(String, String), String>;
    fn resize_tab(&self, tab_id: &str, cols: u16, rows: u16) -> Result<(), String>;
    fn kill_tab_pty(&self, tab_id: &str) -> Result<(), String>;
    fn list_agent_providers(&self) -> Vec<serde_json::Value>;
}

/// Unified command enum — every operation CodeTerm supports.
/// Start with 5 operations, migrate the rest incrementally.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Health check / system identify
    Health,

    /// List all tabs with metadata
    TabList,

    /// List compact tab summaries scoped to one workspace or the focused workspace.
    TabSummary {
        #[serde(default)]
        workspace_id: Option<String>,
        #[serde(default)]
        caller_tab_id: Option<String>,
        #[serde(default)]
        focused_workspace: bool,
        #[serde(default)]
        agents_only: bool,
    },

    /// Inspect selected tabs with the full tab payload.
    TabInspect { tab_ids: Vec<String> },

    /// Send text to a tab's PTY
    SendText { tab_id: String, text: String },

    /// Get tab output (terminal buffer)
    GetOutput {
        tab_id: String,
        #[serde(default)]
        offset: Option<usize>,
        #[serde(default)]
        max_bytes: Option<usize>,
        #[serde(default)]
        strip_ansi: bool,
        #[serde(default)]
        full: bool,
    },

    /// Get visible tab text (VT100-parsed terminal screen).
    /// When `formatted` is true, the string contains ANSI escape codes
    /// preserving colors, cursor position, and cursor visibility — used
    /// for full state restore on tab remount. Default returns plain text.
    GetText {
        tab_id: String,
        #[serde(default)]
        formatted: bool,
    },

    /// Close a tab and kill its PTY. `caller_tab_id` is the tab making the
    /// request (CODETERM_TAB_ID), used to refuse coordinator self-close.
    TabClose {
        tab_id: String,
        #[serde(default)]
        caller_tab_id: Option<String>,
    },

    /// Focus a tab (move keyboard focus).
    TabFocus { tab_id: String },

    /// Rename a tab's title (pinned until the user clears it).
    TabRename { tab_id: String, title: String },

    /// Set or clear a tab's short description.
    TabDescribe { tab_id: String, description: String },

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

    /// Close a workspace and kill all of its PTYs. `caller_tab_id` refuses a
    /// coordinator closing the workspace it runs in.
    WorkspaceClose {
        workspace_id: String,
        #[serde(default)]
        caller_tab_id: Option<String>,
    },

    /// List all workspaces with their metadata.
    WorkspaceList,

    /// List a workspace's recently-closed tabs (newest first).
    WorkspaceRecentlyClosed { workspace_id: String },

    /// Reopen a recently-closed tab (browser-style). `index` selects the ring
    /// entry (newest = 0, default).
    WorkspaceReopenClosedTab {
        workspace_id: String,
        #[serde(default)]
        index: Option<usize>,
    },

    /// Reopen a whole closed group (single / cascade subtree / workspace) by id.
    WorkspaceReopenClosedGroup { group_id: String },

    /// Resize the PTY backing a tab.
    PtyResize {
        tab_id: String,
        cols: u16,
        rows: u16,
    },

    /// Kill the PTY process for a tab without removing the tab from the mux.
    /// Compare to `TabClose`, which both stops the process and drops the tab.
    PtyKill { tab_id: String },
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
        Command::TabList => cmd_tab_list(ctx),
        Command::TabSummary {
            workspace_id,
            caller_tab_id,
            focused_workspace,
            agents_only,
        } => cmd_tab_summary(
            ctx,
            workspace_id.as_deref(),
            caller_tab_id.as_deref(),
            *focused_workspace,
            *agents_only,
        ),
        Command::TabInspect { tab_ids } => cmd_tab_inspect(ctx, tab_ids),
        Command::SendText { tab_id, text } => cmd_send_text(ctx, tab_id, text),
        Command::GetOutput {
            tab_id,
            offset,
            max_bytes,
            strip_ansi,
            full,
        } => cmd_get_output(ctx, tab_id, *offset, *max_bytes, *strip_ansi, *full),
        Command::GetText { tab_id, formatted } => cmd_get_text(ctx, tab_id, *formatted),
        Command::TabClose {
            tab_id,
            caller_tab_id,
        } => cmd_tab_close(ctx, tab_id, caller_tab_id.as_deref()),
        Command::TabFocus { tab_id } => cmd_tab_focus(ctx, tab_id),
        Command::TabRename { tab_id, title } => cmd_tab_rename(ctx, tab_id, title),
        Command::TabDescribe {
            tab_id,
            description,
        } => cmd_tab_describe(ctx, tab_id, description),
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
            caller_tab_id,
        } => cmd_workspace_close(ctx, workspace_id, caller_tab_id.as_deref()),
        Command::WorkspaceList => cmd_workspace_list(ctx),
        Command::WorkspaceRecentlyClosed { workspace_id } => {
            cmd_workspace_recently_closed(ctx, workspace_id)
        }
        Command::WorkspaceReopenClosedTab {
            workspace_id,
            index,
        } => cmd_workspace_reopen_closed_tab(ctx, workspace_id, *index),
        Command::WorkspaceReopenClosedGroup { group_id } => {
            cmd_workspace_reopen_closed_group(ctx, group_id)
        }
        Command::PtyResize { tab_id, cols, rows } => cmd_pty_resize(ctx, tab_id, *cols, *rows),
        Command::PtyKill { tab_id } => cmd_pty_kill(ctx, tab_id),
    }
}

// ── Command implementations ─────────────────────────────────────────

fn is_mutating_user_command(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::SendText { .. }
            | Command::TabClose { .. }
            | Command::TabFocus { .. }
            | Command::TabRename { .. }
            | Command::TabDescribe { .. }
            | Command::WorkspaceCreate { .. }
            | Command::WorkspaceRename { .. }
            | Command::WorkspaceClose { .. }
            | Command::WorkspaceReopenClosedTab { .. }
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

fn cmd_tab_list(ctx: &dyn CommandPort) -> Result<CommandResult, String> {
    let snapshot = ctx.mux_snapshot();
    let tabs = build_tab_list_json(&snapshot);
    Ok(CommandResult::Json(serde_json::json!(tabs)))
}

fn cmd_tab_summary(
    ctx: &dyn CommandPort,
    workspace_id: Option<&str>,
    caller_tab_id: Option<&str>,
    focused_workspace: bool,
    agents_only: bool,
) -> Result<CommandResult, String> {
    let snapshot = ctx.mux_snapshot();
    Ok(CommandResult::Json(serde_json::json!(
        build_tab_summary_json(
            &snapshot,
            workspace_id,
            caller_tab_id,
            focused_workspace,
            agents_only,
        )
    )))
}

fn cmd_tab_inspect(ctx: &dyn CommandPort, tab_ids: &[String]) -> Result<CommandResult, String> {
    if tab_ids.is_empty() {
        return Err("tab.inspect requires at least one Tab id".into());
    }
    let snapshot = ctx.mux_snapshot();
    let requested: std::collections::HashSet<&str> = tab_ids.iter().map(String::as_str).collect();
    let tabs = build_tab_list_json(&snapshot)
        .into_iter()
        .filter(|tab| {
            tab.get("id")
                .and_then(|value| value.as_str())
                .is_some_and(|id| requested.contains(id))
        })
        .collect::<Vec<_>>();
    if tabs.len() != requested.len() {
        return Err("one or more requested Tabs were not found".into());
    }
    Ok(CommandResult::Json(serde_json::json!(tabs)))
}

/// Enrich tab JSON with tab + workspace identifiers.
pub(crate) fn build_tab_list_json(
    snapshot: &crate::multiplexer::state::MultiplexerSnapshot,
) -> Vec<serde_json::Value> {
    let mut tab_index: std::collections::HashMap<String, (String, String, String, String)> =
        std::collections::HashMap::new();
    for ws in &snapshot.workspaces {
        for tab_id in &ws.tabs {
            let tab_title = snapshot
                .tabs
                .get(tab_id)
                .map(|tab| tab.title.clone())
                .unwrap_or_else(|| tab_id.chars().take(8).collect());
            tab_index.insert(
                tab_id.clone(),
                (tab_id.clone(), tab_title, ws.id.clone(), ws.name.clone()),
            );
        }
    }
    let focused_id = snapshot.focused_tab_id.as_deref();
    snapshot
        .tabs
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

pub(crate) fn build_tab_summary_json(
    snapshot: &crate::multiplexer::state::MultiplexerSnapshot,
    workspace_id: Option<&str>,
    caller_tab_id: Option<&str>,
    focused_workspace: bool,
    agents_only: bool,
) -> Vec<serde_json::Value> {
    let mut tab_index: std::collections::HashMap<&str, (&str, &str, &str)> =
        std::collections::HashMap::new();
    for workspace in &snapshot.workspaces {
        for tab_id in &workspace.tabs {
            tab_index.insert(
                tab_id.as_str(),
                (
                    tab_id.as_str(),
                    workspace.id.as_str(),
                    workspace.name.as_str(),
                ),
            );
        }
    }
    let focused_workspace_id = snapshot
        .focused_tab_id
        .as_deref()
        .and_then(|tab_id| tab_index.get(tab_id).map(|(_, id, _)| *id));
    let caller_workspace_id =
        caller_tab_id.and_then(|tab_id| tab_index.get(tab_id).map(|(_, id, _)| *id));
    let selected_workspace = workspace_id
        .or(caller_workspace_id)
        .or_else(|| focused_workspace.then_some(focused_workspace_id).flatten());
    let focused_id = snapshot.focused_tab_id.as_deref();
    snapshot
        .tabs
        .values()
        .filter(|tab| !tab.hidden)
        .filter(|tab| !agents_only || tab.agent_type.is_some())
        .filter(|tab| {
            selected_workspace.is_none_or(|selected| {
                tab_index
                    .get(tab.id.as_str())
                    .is_some_and(|(_, workspace, _)| *workspace == selected)
            })
        })
        .filter_map(|tab| {
            let (tab_id, workspace_id, workspace_name) = tab_index.get(tab.id.as_str())?;
            let backing_kind = match &tab.backing {
                crate::multiplexer::types::TabBacking::Pty => "pty",
                crate::multiplexer::types::TabBacking::ChatBackend { .. } => "chat_backend",
            };
            Some(serde_json::json!({
                "id": tab.id,
                "tab_id": tab_id,
                "title": tab.title,
                "description": tab.description,
                "workspace_id": workspace_id,
                "workspace_name": workspace_name,
                "team_id": tab.team_id,
                "parent_tab_id": tab.parent_tab_id,
                "spawned_by_tab_id": tab.spawned_by_tab_id,
                "agent_type": tab.agent_type,
                "agent_session_id": tab.agent_session_id,
                "agent_session_model": tab.agent_session_model,
                "reasoning_effort_identity": tab.reasoning_effort_identity,
                "context_epoch": tab.context_epoch_archives.len(),
                "provider_context_session_id": tab.agent_session_id,
                // Chat transcript authority compares all three: a summary that omits
                // the nonce makes every remote projection reject its own transcript.
                "session_binding_nonce": tab.session_binding_nonce,
                "archived_context_epochs": tab.context_epoch_archives,
                "pty_id": tab.pty_id,
                "tab_type": tab.tab_type,
                "cwd": tab.cwd,
                "boot_stage": tab.boot_stage,
                "status": tab.agent_state.as_ref().map(|state| state.state),
                "role": tab.role,
                "role_profile": tab.role_profile,
                "ownership": tab.ownership,
                "chat_view": tab.chat_view,
                "backing": tab.backing,
                "backing_kind": backing_kind,
                "pending_delivery": tab.pending_delivery,
                "focused": focused_id == Some(tab.id.as_str()),
            }))
        })
        .collect()
}

fn cmd_send_text(ctx: &dyn CommandPort, tab_id: &str, text: &str) -> Result<CommandResult, String> {
    ctx.send_text(tab_id, text)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_get_output(
    ctx: &dyn CommandPort,
    tab_id: &str,
    offset: Option<usize>,
    max_bytes: Option<usize>,
    strip_ansi: bool,
    full: bool,
) -> Result<CommandResult, String> {
    let json = ctx.read_tab_output(tab_id, offset, max_bytes, strip_ansi, full)?;
    Ok(CommandResult::Json(json))
}

fn cmd_get_text(
    ctx: &dyn CommandPort,
    tab_id: &str,
    formatted: bool,
) -> Result<CommandResult, String> {
    let json = ctx.read_tab_screen(tab_id, formatted)?;
    Ok(CommandResult::Json(json))
}

fn cmd_tab_close(
    ctx: &dyn CommandPort,
    tab_id: &str,
    caller_tab_id: Option<&str>,
) -> Result<CommandResult, String> {
    ctx.close_tab_guarded(caller_tab_id, tab_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_tab_focus(ctx: &dyn CommandPort, tab_id: &str) -> Result<CommandResult, String> {
    ctx.focus_tab(tab_id)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_tab_rename(
    ctx: &dyn CommandPort,
    tab_id: &str,
    title: &str,
) -> Result<CommandResult, String> {
    ctx.rename_tab(tab_id, title)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_tab_describe(
    ctx: &dyn CommandPort,
    tab_id: &str,
    description: &str,
) -> Result<CommandResult, String> {
    ctx.set_tab_description(tab_id, description)?;
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
    caller_tab_id: Option<&str>,
) -> Result<CommandResult, String> {
    ctx.close_workspace_guarded(caller_tab_id, workspace_id)?;
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

fn cmd_workspace_reopen_closed_tab(
    ctx: &dyn CommandPort,
    workspace_id: &str,
    index: Option<usize>,
) -> Result<CommandResult, String> {
    let tab_id = ctx.reopen_closed_tab(workspace_id, index)?;
    Ok(CommandResult::Json(serde_json::json!({
        "tab_id": tab_id,
    })))
}

fn cmd_workspace_reopen_closed_group(
    ctx: &dyn CommandPort,
    group_id: &str,
) -> Result<CommandResult, String> {
    let (workspace_id, tab_id) = ctx.reopen_closed_group(group_id)?;
    Ok(CommandResult::Json(serde_json::json!({
        "workspace_id": workspace_id,
        "tab_id": tab_id,
    })))
}

fn cmd_pty_resize(
    ctx: &dyn CommandPort,
    tab_id: &str,
    cols: u16,
    rows: u16,
) -> Result<CommandResult, String> {
    ctx.resize_tab(tab_id, cols, rows)?;
    Ok(CommandResult::Json(serde_json::json!({"ok": true})))
}

fn cmd_pty_kill(ctx: &dyn CommandPort, tab_id: &str) -> Result<CommandResult, String> {
    ctx.kill_tab_pty(tab_id)?;
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
        let json = r#"{"cmd": "send_text", "tab_id": "abc", "text": "hello"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::SendText { tab_id, text } => {
                assert_eq!(tab_id, "abc");
                assert_eq!(text, "hello");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_get_output() {
        let json = r#"{"cmd": "get_output", "tab_id": "abc", "strip_ansi": true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetOutput {
                tab_id,
                strip_ansi,
                full,
                offset,
                max_bytes,
            } => {
                assert_eq!(tab_id, "abc");
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
        let json = r#"{"cmd": "get_text", "tab_id": "abc", "formatted": true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetText { tab_id, formatted } => {
                assert_eq!(tab_id, "abc");
                assert!(formatted);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_command_deserialize_tab_list() {
        let json = r#"{"cmd": "tab_list"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::TabList));
    }

    #[test]
    fn test_command_result_into_json() {
        let result = CommandResult::Json(serde_json::json!({"test": true}));
        let json = result.into_json();
        assert_eq!(json["test"], true);
    }

    // ── Command enum round-trip tests ───────────────────────────────

    #[test]
    fn tab_close_deserialize() {
        let cmd: Command =
            serde_json::from_str(r#"{"cmd": "tab_close", "tab_id": "abc"}"#).unwrap();
        assert!(matches!(cmd, Command::TabClose { .. }));
    }

    #[test]
    fn tab_focus_deserialize() {
        let cmd: Command =
            serde_json::from_str(r#"{"cmd": "tab_focus", "tab_id": "abc"}"#).unwrap();
        assert!(matches!(cmd, Command::TabFocus { .. }));
    }

    #[test]
    fn tab_rename_deserialize() {
        let json = r#"{"cmd": "tab_rename", "tab_id": "abc", "title": "My Tab"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::TabRename { tab_id, title } => {
                assert_eq!(tab_id, "abc");
                assert_eq!(title, "My Tab");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn get_output_deserialize_full_params() {
        let json = r#"{"cmd":"get_output","tab_id":"p","offset":100,"max_bytes":4096,"strip_ansi":true,"full":true}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::GetOutput {
                tab_id,
                offset,
                max_bytes,
                strip_ansi,
                full,
            } => {
                assert_eq!(tab_id, "p");
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
            Command::TabList,
            Command::SendText {
                tab_id: "p".into(),
                text: "hi".into(),
            },
            Command::GetOutput {
                tab_id: "p".into(),
                offset: None,
                max_bytes: None,
                strip_ansi: false,
                full: false,
            },
            Command::GetText {
                tab_id: "p".into(),
                formatted: false,
            },
            Command::TabClose {
                tab_id: "p".into(),
                caller_tab_id: None,
            },
            Command::TabFocus { tab_id: "p".into() },
            Command::TabRename {
                tab_id: "p".into(),
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
                caller_tab_id: None,
            },
            Command::WorkspaceList,
            Command::PtyResize {
                tab_id: "p".into(),
                cols: 80,
                rows: 24,
            },
            Command::PtyKill { tab_id: "p".into() },
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
        let json = r#"{"cmd":"pty_resize","tab_id":"p","cols":120,"rows":40}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::PtyResize { tab_id, cols, rows } => {
                assert_eq!(tab_id, "p");
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn pty_kill_deserialize() {
        let cmd: Command = serde_json::from_str(r#"{"cmd":"pty_kill","tab_id":"p"}"#).unwrap();
        assert!(matches!(cmd, Command::PtyKill { .. }));
    }

    #[test]
    fn list_agent_providers_deserialize() {
        let cmd: Command = serde_json::from_str(r#"{"cmd": "list_agent_providers"}"#).unwrap();
        assert!(matches!(cmd, Command::ListAgentProviders));
    }

    // `cmd_tab_list` and `build_tab_list_json` enrich tabs with sidebar labels.
    // Tests stay on the pure helper so both `Tab list` and `Tab find` keep full workspace context.

    #[test]
    fn tab_list_attaches_tab_title_and_workspace_name() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, initial_tab) = MultiplexerState::new();
        let snapshot = state.snapshot();
        let ws = snapshot.workspaces.first().expect("default ws").clone();
        let tab_id = state.find_tab_for_tab(&initial_tab).expect("default tab");
        state.rename_tab(&tab_id, "Dalton").unwrap();
        let snapshot = state.snapshot();
        let tabs = build_tab_list_json(&snapshot);
        let tab = tabs
            .iter()
            .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(initial_tab.as_str()))
            .expect("default Tab present in list");
        assert_eq!(
            tab.get("tab_title").and_then(|v| v.as_str()),
            Some("Dalton"),
            "tab_title must be attached so CLI can show the sidebar label",
        );
        assert_eq!(
            tab.get("tab_id").and_then(|v| v.as_str()),
            Some(tab_id.as_str()),
        );
        assert_eq!(
            tab.get("workspace_id").and_then(|v| v.as_str()),
            Some(ws.id.as_str()),
        );
        assert!(
            tab.get("workspace_name").and_then(|v| v.as_str()).is_some(),
            "workspace_name must be attached so user can disambiguate same-titled tabs across workspaces",
        );
    }

    #[test]
    fn tab_summary_is_workspace_scoped_and_excludes_full_payload_fields() {
        use crate::multiplexer::state::MultiplexerState;
        let (state, initial_tab) = MultiplexerState::new();
        let snapshot = state.snapshot();
        let workspace = snapshot.workspaces.first().expect("default workspace");
        let summaries = build_tab_summary_json(&snapshot, Some(&workspace.id), None, false, false);
        assert_eq!(summaries.len(), 1);
        let tab = summaries.first().expect("summary Tab");
        assert_eq!(tab["id"].as_str(), Some(initial_tab.as_str()));
        assert_eq!(tab["workspace_id"].as_str(), Some(workspace.id.as_str()));
        assert!(tab.get("agent_state").is_none());
        assert!(tab.get("model_fallback").is_none());
        for identity in ["agent_session_id", "provider_context_session_id", "session_binding_nonce"] {
            assert!(
                tab.get(identity).is_some(),
                "{identity} must survive into the summary: remote projections are built from it \
                 and chat transcript authority compares the whole session identity",
            );
        }
    }

    #[test]
    fn tab_summary_keeps_team_terminal_provenance() {
        use crate::multiplexer::state::MultiplexerState;

        let (mut state, tab_id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).expect("default Tab");
        tab.team_id = Some("team_probe".into());
        tab.spawned_by_tab_id = Some("creator_probe".into());

        let snapshot = state.snapshot();
        let workspace = snapshot.workspaces.first().expect("default workspace");
        let summaries = build_tab_summary_json(&snapshot, Some(&workspace.id), None, false, false);
        let summary = summaries.first().expect("summary Tab");

        assert_eq!(summary["team_id"], "team_probe");
        assert_eq!(summary["spawned_by_tab_id"], "creator_probe");
    }

    #[test]
    fn tab_list_default_scope_keeps_requesting_tab_visible_during_focus_churn() {
        use crate::multiplexer::state::MultiplexerState;

        let (mut state, requesting_tab) = MultiplexerState::new();
        let other_workspace = state.create_workspace("other".into(), None);
        let focused_tab = state.create_tab(&other_workspace.id).unwrap();
        state.set_focused_tab(&focused_tab).unwrap();

        let summaries =
            build_tab_summary_json(&state.snapshot(), None, Some(&requesting_tab), true, false);

        assert!(
            summaries
                .iter()
                .any(|tab| tab["id"].as_str() == Some(requesting_tab.as_str())),
            "requesting live Tab {requesting_tab} was omitted after focus moved to {focused_tab}"
        );
    }

    #[test]
    fn pending_delivery_survives_persistence_and_compact_projection() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryRecord};
        use crate::session::store::SessionStore;

        let (mut state, tab_id) = MultiplexerState::new();
        let mut pending = PendingDeliveryRecord::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            PendingDeliveryChannel::InteractiveInput,
        );
        pending.mark_unconfirmed();
        state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(pending.clone());

        let snapshot = state.to_session_snapshot();
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(temp.path().to_path_buf());
        store.save(&snapshot).unwrap();
        let persisted = store.load_last().unwrap().unwrap();
        let restored = MultiplexerState::from_session_snapshot(&persisted);
        let restored_snapshot = restored.snapshot();
        let workspace_id = restored_snapshot.workspaces[0].id.as_str();
        let projection =
            build_tab_summary_json(&restored_snapshot, Some(workspace_id), None, false, false);
        let projected = projection
            .iter()
            .find(|tab| tab["id"].as_str() == Some(tab_id.as_str()))
            .unwrap();

        assert_eq!(
            restored.get_tab(&tab_id).unwrap().pending_delivery,
            Some(pending.clone())
        );
        assert_eq!(
            projected.get("pending_delivery"),
            Some(&serde_json::to_value(pending).unwrap())
        );
    }

    #[test]
    fn tab_summary_command_deserializes_compact_scope_options() {
        let command: Command = serde_json::from_str(
            r#"{"cmd":"tab_summary","workspace_id":"ws_1","caller_tab_id":"tab_1","focused_workspace":false,"agents_only":true}"#,
        )
        .expect("Tab summary command");
        assert!(matches!(
            command,
            Command::TabSummary {
                workspace_id: Some(workspace_id),
                caller_tab_id: Some(caller_tab_id),
                focused_workspace: false,
                agents_only: true,
            } if workspace_id == "ws_1" && caller_tab_id == "tab_1"
        ));
    }

    #[test]
    fn tab_list_focused_field_tracks_mux_focused_tab_id() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, tab_a) = MultiplexerState::new();
        // Create a second tab.
        let tab_b = state
            .create_tab(&state.list_workspaces()[0].id.clone())
            .unwrap();
        // Focus tab_a explicitly.
        state.set_focused_tab(&tab_a).unwrap();
        let tabs = build_tab_list_json(&state.snapshot());
        let get_focused = |tabs: &[serde_json::Value], id: &str| {
            tabs.iter()
                .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
                .and_then(|p| p.get("focused"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        assert!(
            get_focused(&tabs, &tab_a),
            "focused Tab must have focused=true"
        );
        assert!(
            !get_focused(&tabs, &tab_b),
            "non-focused Tab must have focused=false"
        );
        // Switch focus.
        state.set_focused_tab(&tab_b).unwrap();
        let tabs = build_tab_list_json(&state.snapshot());
        assert!(
            !get_focused(&tabs, &tab_a),
            "tab_a must be unfocused after switch"
        );
        assert!(
            get_focused(&tabs, &tab_b),
            "tab_b must be focused after switch"
        );
    }

    #[test]
    fn tab_list_marks_exactly_one_focused_tab_from_each_snapshot() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, tab_a) = MultiplexerState::new();
        let tab_b = state
            .create_tab(&state.list_workspaces()[0].id.clone())
            .unwrap();

        for focused in [&tab_a, &tab_b] {
            state.set_focused_tab(focused).unwrap();
            let tabs = build_tab_list_json(&state.snapshot());
            let focused_ids: Vec<&str> = tabs
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
    fn tab_list_keeps_tab_info_after_rename_to_fibonacci() {
        use crate::multiplexer::state::MultiplexerState;
        let (mut state, visible) = MultiplexerState::new();
        let tab_id = state.find_tab_for_tab(&visible).unwrap();
        state.rename_tab(&tab_id, "Fibonacci").unwrap();
        let snapshot = state.snapshot();
        let tabs = build_tab_list_json(&snapshot);
        assert_eq!(tabs.len(), 1);
        assert_eq!(
            tabs[0].get("tab_title").and_then(|v| v.as_str()),
            Some("Fibonacci"),
        );
    }
}
