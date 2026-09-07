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
    fn close_workspace_guarded(
        &self,
        caller: Option<&str>,
        ws_id: &str,
        force: bool,
    ) -> Result<(), String>;
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
    TabInspect {
        tab_ids: Vec<String>,
        #[serde(default)]
        lifecycle: bool,
    },

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
    /// coordinator closing the workspace it runs in; `force` acknowledges any agent tabs.
    WorkspaceClose {
        workspace_id: String,
        #[serde(default)]
        caller_tab_id: Option<String>,
        #[serde(default)]
        force: bool,
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
        Command::TabInspect { tab_ids, lifecycle } => cmd_tab_inspect(ctx, tab_ids, *lifecycle),
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
            force,
        } => cmd_workspace_close(ctx, workspace_id, caller_tab_id.as_deref(), *force),
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

fn cmd_tab_inspect(
    ctx: &dyn CommandPort,
    tab_ids: &[String],
    lifecycle: bool,
) -> Result<CommandResult, String> {
    if tab_ids.is_empty() {
        return Err("tab.inspect requires at least one Tab id".into());
    }
    if lifecycle && tab_ids.len() != 1 {
        return Err("tab.inspect lifecycle history requires exactly one Tab id".into());
    }
    let snapshot = ctx.mux_snapshot();
    let requested: std::collections::HashSet<&str> = tab_ids.iter().map(String::as_str).collect();
    let mut tabs = build_tab_list_json(&snapshot)
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
    if lifecycle {
        let tab = snapshot
            .tabs
            .get(&tab_ids[0])
            .ok_or_else(|| "requested Tab was not found".to_string())?;
        tabs[0]["lifecycle"] = crate::session::lifecycle::inspect_tab_lifecycle(tab);
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

/// Build compact tab summaries using the same scope rules as REST and socket callers.
/// The one liveness observation, over the facts a multiplexer snapshot carries.
/// A tab with no PTY, or a PTY the status tracker does not track, has not been
/// observed — that is `Unobserved`, never a guess in either direction.
fn snapshot_agent_liveness(
    tab: &crate::multiplexer::types::Tab,
    tracked: Option<&codeterm_types::pty::TabStatus>,
) -> codeterm_types::pty::AgentLiveness {
    use codeterm_types::pty::{AgentBacking, LivenessEvidence, TabStatus};
    let backing = match &tab.backing {
        crate::multiplexer::types::TabBacking::ChatBackend { .. } => AgentBacking::ChatBackend {
            session_open: tab.agent_session_id.is_some(),
        },
        crate::multiplexer::types::TabBacking::Pty => AgentBacking::Pty {
            pty_live: tab
                .pty_id
                .as_ref()
                .and_then(|_| tracked.map(|status| *status != TabStatus::Dead)),
        },
    };
    codeterm_types::pty::classify_agent_liveness(LivenessEvidence {
        declares_agent: tab.agent_type.is_some(),
        backing,
        boot_stage: tab.boot_stage,
        boot_in_flight: tab.agent_booting,
        provider_exit_observed: tab.exit_suspected_at.is_some(),
    })
}

pub fn build_tab_summary_json(
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
            let liveness = snapshot_agent_liveness(tab, snapshot.tab_statuses.get(tab.id.as_str()));
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
                "reported_session_binding_nonce": tab.reported_session_binding_nonce,
                "archived_context_epochs": tab.context_epoch_archives,
                "pty_id": tab.pty_id,
                "tab_type": tab.tab_type,
                "cwd": tab.cwd,
                "boot_stage": tab.boot_stage,
                "status": tab.agent_state.as_ref().map(|state| state.state),
                "task_binding": tab.task_binding,
                "roleless_tracked": tab.roleless_tracked,
                "role": tab.role,
                "role_profile": tab.role_profile,
                "ownership": tab.ownership,
                "chat_view": tab.chat_view,
                "backing": tab.backing,
                "backing_kind": backing_kind,
                "pending_delivery": tab.pending_delivery,
                "agent_liveness": liveness,
                // A session model is observed at runtime and only some providers
                // report one; the requested model is still the tab's identity.
                "agent_label": codeterm_types::pty::agent_bracket_label(
                    tab.agent_type.as_deref(),
                    tab.agent_session_model
                        .as_deref()
                        .or(tab.agent_provider_resolved_model.as_deref()),
                    liveness,
                ),
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
    force: bool,
) -> Result<CommandResult, String> {
    ctx.close_workspace_guarded(caller_tab_id, workspace_id, force)?;
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

#[cfg(test)]
mod tests;
