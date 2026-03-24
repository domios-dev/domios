//! Agent spawn & batch command orchestration.

use serde::{Deserialize, Serialize};
use super::{json_str, resolve_refs, ServiceContext};

// ── Batch ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchOp {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: Option<String>,
}

impl ServiceContext {
    /// Execute a batch of commands sequentially. Supports $ref resolution.
    pub async fn batch(&self, commands: Vec<BatchOp>) -> Vec<serde_json::Value> {
        let mut refs: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut results: Vec<serde_json::Value> = Vec::new();

        for cmd in &commands {
            let params = resolve_refs(&cmd.params, &refs);

            let result = match cmd.method.as_str() {
                "spawn" => self.batch_spawn(&params).await,
                "send" => self.batch_send(&params),
                "rename" => self.batch_rename(&params),
                "close" => self.batch_close(&params),
                "role" => self.batch_role(&params),
                "notify" => self.batch_notify(&params),
                "can-spawn" => self.batch_can_spawn(&params),
                "report" => self.batch_report(&params),
                other => Err(format!("Unknown method: {}", other)),
            };

            match result {
                Ok(val) => {
                    if let Some(ref id) = cmd.id {
                        if let Some(pid) = val.get("pane_id").and_then(|v| v.as_str()) {
                            refs.insert(id.clone(), pid.to_string());
                        }
                    }
                    results.push(val);
                }
                Err(e) => {
                    results.push(serde_json::json!({"error": e, "method": cmd.method}));
                }
            }
        }

        self.emit_state_changed();
        results
    }

    async fn batch_spawn(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let p = SpawnAgentParams::from_json(params)?;
        let result = self.spawn_agent(p).await?;
        Ok(serde_json::json!({
            "pane_id": result.pane_id,
            "tab_id": result.tab_id,
            "pty_id": result.pty_id,
            "agent": result.agent,
            "title": result.title,
            "role": result.role,
        }))
    }

    fn batch_send(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let text = json_str(params, "text").unwrap_or_default();
        let enter = params
            .get("enter")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        self.send_text(&pane_id, &text, enter)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_rename(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let title = json_str(params, "title").ok_or("title required")?;
        self.rename_pane(&pane_id, &title)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_close(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        self.close_pane(&pane_id)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_role(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let role = json_str(params, "role").ok_or("role required")?;
        let parent = json_str(params, "parent_pane_id");
        let can_spawn = params.get("can_spawn").and_then(|v| v.as_bool());
        self.set_pane_role(&pane_id, &role, parent.as_deref(), can_spawn)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_notify(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let title = json_str(params, "title").unwrap_or_else(|| "Notification".to_string());
        let body = json_str(params, "body");
        let level = json_str(params, "level");
        let id = self.create_notification(title, body, level.as_deref());
        Ok(serde_json::json!({"ok": true, "id": id}))
    }

    fn batch_can_spawn(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let can_spawn = params
            .get("can_spawn")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        self.set_can_spawn(&pane_id, can_spawn)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_report(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let pane_id = json_str(params, "pane_id").ok_or("pane_id required")?;
        let message = json_str(params, "message").ok_or("message required")?;
        let result = self.submit_report(&pane_id, &message)?;
        Ok(serde_json::json!({"ok": true, "report_id": result.report_id}))
    }
}

// ── Agent Spawn ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SpawnResult {
    pub pane_id: String,
    pub tab_id: String,
    pub pty_id: String,
    pub agent: String,
    pub title: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnAgentParams {
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub role: Option<String>,
    pub parent_pane_id: Option<String>,
    pub can_spawn: Option<bool>,
    pub task: Option<String>,
    pub workspace_id: Option<String>,
    /// Force creation as a new tab instead of split pane (even with parent).
    pub force_tab: Option<bool>,
}

impl SpawnAgentParams {
    pub fn from_json(params: &serde_json::Value) -> Result<Self, String> {
        Ok(Self {
            agent: json_str(params, "agent"),
            agent_name: json_str(params, "agent_name"),
            args: params
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                }),
            cwd: json_str(params, "cwd"),
            title: json_str(params, "title"),
            role: json_str(params, "role"),
            parent_pane_id: json_str(params, "parent_pane_id")
                .or_else(|| json_str(params, "parent")),
            can_spawn: params.get("can_spawn").and_then(|v| v.as_bool()),
            task: json_str(params, "task"),
            workspace_id: json_str(params, "workspace_id"),
            force_tab: params.get("force_tab").and_then(|v| v.as_bool()),
        })
    }
}

impl ServiceContext {
    /// One-call agent spawn: create tab → wait PTY → set role → launch agent.
    ///
    /// Two modes:
    /// - **Interactive** (default): launches agent, waits for readiness, sends task.
    ///   The agent stays alive for follow-up messages.
    /// - **One-shot** (`oneshot: true`): uses `-p` flag to pass task inline.
    ///   Agent exits after responding.
    /// Synchronous spawn — for socket API. No tokio runtime needed.
    pub fn spawn_agent_sync(&self, params: SpawnAgentParams) -> Result<SpawnResult, String> {
        let agent_cmd = params.agent.as_deref().unwrap_or("claude").to_string();
        let title = params.title.as_deref()
            .or(params.agent_name.as_deref())
            .unwrap_or(&agent_cmd).to_string();

        // 1. Find workspace
        let ws_id = if let Some(ref parent_id) = params.parent_pane_id {
            self.mux.lock().find_workspace_for_pane(parent_id)
                .or(params.workspace_id.clone())
                .or_else(|| {
                    let mux = self.mux.lock();
                    mux.snapshot().active_workspace_id.clone()
                        .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                })
                .ok_or("No workspace found for parent pane")?
        } else {
            params.workspace_id.clone()
                .or_else(|| {
                    let mux = self.mux.lock();
                    mux.snapshot().active_workspace_id.clone()
                        .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                })
                .ok_or("No workspace found")?
        };

        // 2. Create tab
        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            if let Some(ref dir) = params.cwd {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.cwd = Some(dir.clone());
                }
            }
            let _ = mux.set_active_tab(&ws_id, &result.0);
            result
        };
        self.emit_state_changed();

        // 3. Set role
        if let Some(ref role_str) = params.role {
            self.set_pane_role(&pane_id, role_str, params.parent_pane_id.as_deref(), params.can_spawn)?;
        }

        // 4. Rename
        self.rename_pane(&pane_id, &title)?;

        // 5. Background launch
        let launch_cmd = build_agent_command(&agent_cmd, &params);
        let is_claude = agent_cmd == "claude";
        let svc = self.clone();
        let bg_pane_id = pane_id.clone();
        let bg_task = params.task.clone();
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let pty_id = loop {
                if std::time::Instant::now() > deadline { return; }
                if let Some(pid) = svc.mux.try_lock()
                    .and_then(|m| m.get_pane(&bg_pane_id).and_then(|p| p.pty_id.clone())) { break pid; }
                std::thread::sleep(std::time::Duration::from_millis(300));
            };
            if is_claude {
                let dl = std::time::Instant::now() + std::time::Duration::from_secs(30);
                loop {
                    if std::time::Instant::now() > dl { break; }
                    let ready = svc.output_buf.try_lock()
                        .and_then(|b| b.get_text(&pty_id).map(|t| t.contains("$ ") || t.contains("% ") || t.contains("> ") || t.contains("PS ")))
                        .unwrap_or(false);
                    if ready { break; }
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                let _ = svc.pty.lock().write(&pty_id, format!("{}\r", launch_cmd).as_bytes());
                if let Some(ref task) = bg_task { svc.spawn_task_sender(pty_id, task.clone()); }
            } else {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let _ = svc.pty.lock().write(&pty_id, format!("{}\r", launch_cmd).as_bytes());
                if let Some(ref task) = bg_task {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = svc.pty.lock().write(&pty_id, format!("{}\r", task).as_bytes());
                }
            }
        });

        Ok(SpawnResult { pane_id, tab_id, pty_id: String::new(), agent: agent_cmd, title, role: params.role })
    }

    pub async fn spawn_agent(&self, params: SpawnAgentParams) -> Result<SpawnResult, String> {
        let agent_cmd = params.agent.as_deref().unwrap_or("claude");
        let title = params
            .title
            .as_deref()
            .or(params.agent_name.as_deref())
            .unwrap_or(agent_cmd);

        // 1. Find workspace — prefer parent's workspace, then explicit, then active, then first
        let ws_id = if let Some(ref parent_id) = params.parent_pane_id {
            self.mux.lock().find_workspace_for_pane(parent_id)
                .or_else(|| params.workspace_id.clone())
                .or_else(|| {
                    let mux = self.mux.lock();
                    mux.snapshot().active_workspace_id.clone()
                        .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                })
                .ok_or("No workspace found for parent pane")?
        } else {
            params.workspace_id.clone()
                .or_else(|| {
                    let mux = self.mux.lock();
                    mux.snapshot().active_workspace_id.clone()
                        .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                })
                .ok_or("No workspace found")?
        };

        // 2. Create new tab in the workspace (each agent gets its own tab)
        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            if let Some(ref dir) = params.cwd {
                if let Some(pane) = mux.get_pane_mut(&result.1) {
                    pane.cwd = Some(dir.clone());
                }
            }
            let _ = mux.set_active_tab(&ws_id, &result.0);
            result
        };
        self.emit_state_changed();

        // 3. Set role in hierarchy (before async wait — role is metadata)
        if let Some(ref role_str) = params.role {
            self.set_pane_role(
                &pane_id,
                role_str,
                params.parent_pane_id.as_deref(),
                params.can_spawn,
            )?;
        }

        // 4. Rename pane immediately (metadata only)
        self.rename_pane(&pane_id, title)?;

        // 5. Launch agent in background thread — don't block API runtime.
        //    Uses std::thread to avoid saturating the Tokio worker pool.
        let launch_cmd = build_agent_command(agent_cmd, &params);
        let is_claude = agent_cmd == "claude";
        let svc = self.clone();
        let bg_pane_id = pane_id.clone();
        let bg_task = params.task.clone();
        std::thread::spawn(move || {
            // Wait for PTY (frontend creates it after mux-state-changed)
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let pty_id = loop {
                if std::time::Instant::now() > deadline {
                    log::error!("spawn_agent: PTY wait timeout for {}", bg_pane_id);
                    return;
                }
                if let Some(pid) = svc.mux.try_lock()
                    .and_then(|m| m.get_pane(&bg_pane_id).and_then(|p| p.pty_id.clone()))
                {
                    break pid;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            };

            if is_claude {
                // Claude: wait for shell prompt, send launch command, then poll for readiness
                let prompt_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                loop {
                    if std::time::Instant::now() > prompt_deadline {
                        log::warn!("spawn_agent: shell prompt wait timeout for {}", bg_pane_id);
                        break;
                    }
                    let ready = svc.output_buf.try_lock()
                        .and_then(|buf| {
                            buf.get_text(&pty_id).map(|t| {
                                t.contains("PS ") || t.contains("> ")
                                    || t.contains("$ ") || t.contains("% ")
                            })
                        })
                        .unwrap_or(false);
                    if ready { break; }
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                if let Err(e) = svc.pty.lock().write(&pty_id, format!("{}\r", launch_cmd).as_bytes()) {
                    log::error!("spawn_agent: failed to write launch cmd: {}", e);
                    return;
                }
                if let Some(ref task_text) = bg_task {
                    svc.spawn_task_sender(pty_id, task_text.clone());
                }
            } else {
                // Non-claude: short delay for shell init, send command + task directly, then exit thread
                std::thread::sleep(std::time::Duration::from_secs(2));
                let _ = svc.pty.lock().write(&pty_id, format!("{}\r", launch_cmd).as_bytes());
                if let Some(ref task_text) = bg_task {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = svc.pty.lock().write(&pty_id, format!("{}\r", task_text).as_bytes());
                }
                log::info!("spawn_agent: non-claude agent launched in {}", bg_pane_id);
                // Thread exits immediately — no long-running polling
            }
        });

        Ok(SpawnResult {
            pane_id,
            tab_id,
            pty_id: String::new(), // PTY not yet assigned — happens async
            agent: agent_cmd.to_string(),
            title: title.to_string(),
            role: params.role,
        })
    }
}

/// Build a single shell command that launches the agent.
///
/// Build launch command for the agent. Always interactive — no `-p` flag.
///
/// For Claude Code:
///   `unset CLAUDECODE && claude --dangerously-skip-permissions [--agent NAME]`
/// Task is sent separately after the agent is ready (via spawn_task_sender).
///
/// For other agents:
///   `agent [args]`
fn build_agent_command(agent_cmd: &str, params: &SpawnAgentParams) -> String {
    let mut parts: Vec<String> = Vec::new();

    if agent_cmd == "claude" {
        // Unset the nested-session guard env var
        parts.push("unset CLAUDECODE &&".to_string());

        parts.push("claude".to_string());
        parts.push("--dangerously-skip-permissions".to_string());

        // Use --agent flag for named agent definitions
        if let Some(ref name) = params.agent_name {
            parts.push("--agent".to_string());
            parts.push(shell_quote(name));
        }
    } else {
        parts.push(agent_cmd.to_string());
    }

    // Extra args
    if let Some(ref extra) = params.args {
        parts.extend(extra.iter().cloned());
    }

    parts.join(" ")
}

/// Shell-quote a string: wrap in single quotes, escape internal single quotes.
fn shell_quote(s: &str) -> String {
    if s.contains('\'') {
        // Replace ' with '\'' (end quote, escaped quote, start quote)
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        format!("'{}'", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params(agent: &str, task: Option<&str>) -> SpawnAgentParams {
        SpawnAgentParams {
            agent: Some(agent.to_string()),
            task: task.map(|t| t.to_string()),
            agent_name: None,
            args: None,
            cwd: None,
            title: None,
            role: None,
            parent_pane_id: None,
            can_spawn: None,
            workspace_id: None,
            force_tab: None,
        }
    }

    #[test]
    fn test_build_claude_with_task_is_interactive() {
        // Task is NOT in the launch command — sent separately after readiness
        let params = make_params("claude", Some("count files"));
        let cmd = build_agent_command("claude", &params);
        assert!(cmd.starts_with("unset CLAUDECODE &&"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(!cmd.contains(" -p "), "must be interactive, no -p flag");
        assert!(!cmd.contains("count files"), "task must not be in launch command");
    }

    #[test]
    fn test_build_claude_no_task() {
        let params = make_params("claude", None);
        let cmd = build_agent_command("claude", &params);
        assert_eq!(cmd, "unset CLAUDECODE && claude --dangerously-skip-permissions");
    }

    #[test]
    fn test_build_claude_with_agent_name() {
        let mut params = make_params("claude", Some("review code"));
        params.agent_name = Some("architect".to_string());
        let cmd = build_agent_command("claude", &params);
        assert!(cmd.contains("--agent 'architect'"));
        assert!(!cmd.contains(" -p "), "must be interactive");
    }

    #[test]
    fn test_build_other_agent() {
        let mut params = make_params("aider", None);
        params.args = Some(vec!["--model".to_string(), "gpt-4".to_string()]);
        let cmd = build_agent_command("aider", &params);
        assert_eq!(cmd, "aider --model gpt-4");
        assert!(!cmd.contains("CLAUDECODE"));
    }

    #[test]
    fn test_shell_quote_simple() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn test_shell_quote_with_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_quote_with_spaces_and_special() {
        assert_eq!(shell_quote("count *.tsx files"), "'count *.tsx files'");
    }
}
