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
        })
    }
}

impl ServiceContext {
    /// One-call agent spawn: create tab → wait PTY → set role → launch agent.
    ///
    /// For Claude Code: uses `-p` flag to send the task inline (no polling needed).
    /// Unsets CLAUDECODE env var so nested sessions work.
    /// Uses a single shell command to avoid sequential waits.
    pub async fn spawn_agent(&self, params: SpawnAgentParams) -> Result<SpawnResult, String> {
        let agent_cmd = params.agent.as_deref().unwrap_or("claude");
        let title = params
            .title
            .as_deref()
            .or(params.agent_name.as_deref())
            .unwrap_or(agent_cmd);

        // 1. Find workspace
        let ws_id = params
            .workspace_id
            .clone()
            .or_else(|| self.mux.lock().snapshot().active_workspace_id.clone())
            .ok_or("No workspace found")?;

        // 2. Create tab + pane, set cwd
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

        // 3. Wait for PTY
        let pty_id = self.wait_for_pty(&pane_id, 10).await?;

        // 4. Rename tab
        self.rename_pane(&pane_id, title)?;

        // 5. Set role in hierarchy
        if let Some(ref role_str) = params.role {
            self.set_pane_role(
                &pane_id,
                role_str,
                params.parent_pane_id.as_deref(),
                params.can_spawn,
            )?;
        }

        // 6. Build the launch command
        let launch_cmd = build_agent_command(agent_cmd, &params);

        // 7. Wait for shell prompt, then send everything as one command
        self.wait_for_shell_prompt(&pty_id, 10).await;
        self.pty
            .lock()
            .write(&pty_id, format!("{}\r", launch_cmd).as_bytes())
            .map_err(|e| e.to_string())?;

        Ok(SpawnResult {
            pane_id,
            tab_id,
            pty_id,
            agent: agent_cmd.to_string(),
            title: title.to_string(),
            role: params.role,
        })
    }
}

/// Build a single shell command that launches the agent.
///
/// For Claude Code:
///   `unset CLAUDECODE && claude --dangerously-skip-permissions [-p "task"]`
/// The `-p` flag passes the task as a prompt argument — no polling or
/// delayed send needed. CLAUDECODE is unset so nested sessions work.
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

        // Pass task inline via -p flag — instant start, no polling
        if let Some(ref task) = params.task {
            parts.push("-p".to_string());
            parts.push(shell_quote(task));
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

    #[test]
    fn test_build_claude_command_with_task() {
        let params = SpawnAgentParams {
            agent: Some("claude".to_string()),
            task: Some("count files".to_string()),
            agent_name: None,
            args: None,
            cwd: None,
            title: None,
            role: None,
            parent_pane_id: None,
            can_spawn: None,
            workspace_id: None,
        };
        let cmd = build_agent_command("claude", &params);
        assert!(cmd.starts_with("unset CLAUDECODE &&"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains("-p 'count files'"));
    }

    #[test]
    fn test_build_claude_command_no_task() {
        let params = SpawnAgentParams {
            agent: Some("claude".to_string()),
            task: None,
            agent_name: None,
            args: None,
            cwd: None,
            title: None,
            role: None,
            parent_pane_id: None,
            can_spawn: None,
            workspace_id: None,
        };
        let cmd = build_agent_command("claude", &params);
        assert_eq!(cmd, "unset CLAUDECODE && claude --dangerously-skip-permissions");
    }

    #[test]
    fn test_build_claude_command_with_agent_name() {
        let params = SpawnAgentParams {
            agent: Some("claude".to_string()),
            task: Some("review code".to_string()),
            agent_name: Some("architect".to_string()),
            args: None,
            cwd: None,
            title: None,
            role: None,
            parent_pane_id: None,
            can_spawn: None,
            workspace_id: None,
        };
        let cmd = build_agent_command("claude", &params);
        assert!(cmd.contains("--agent 'architect'"));
        assert!(cmd.contains("-p 'review code'"));
    }

    #[test]
    fn test_build_other_agent() {
        let params = SpawnAgentParams {
            agent: Some("aider".to_string()),
            task: None,
            agent_name: None,
            args: Some(vec!["--model".to_string(), "gpt-4".to_string()]),
            cwd: None,
            title: None,
            role: None,
            parent_pane_id: None,
            can_spawn: None,
            workspace_id: None,
        };
        let cmd = build_agent_command("aider", &params);
        assert_eq!(cmd, "aider --model gpt-4");
        // No CLAUDECODE unset for non-claude agents
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
