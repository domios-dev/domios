use super::types::{BatchOp, SpawnAgentParams};
use crate::service::{json_str, resolve_refs, ServiceContext};

impl ServiceContext {
    /// Execute a batch of commands sequentially. Supports $ref resolution.
    pub async fn batch(&self, commands: Vec<BatchOp>) -> Vec<serde_json::Value> {
        let mut refs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
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
                        if let Some(pid) = val.get("tab_id").and_then(|v| v.as_str()) {
                            refs.insert(id.clone(), pid.to_string());
                        }
                    }
                    results.push(val);
                }
                Err(e) => {
                    log::warn!("[batch] {} failed: {}", cmd.method, e);
                    results.push(serde_json::json!({"error": e, "method": cmd.method}));
                }
            }
        }

        self.emit_state_changed();
        results
    }

    async fn batch_spawn(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let p = SpawnAgentParams::from_json(params)?;
        let result = self.spawn_agent(p, None).await?;
        Ok(serde_json::json!({
            "tab_id": result.tab_id,
            "pty_id": result.pty_id,
            "agent": result.agent,
            "title": result.title,
            "role": result.role,
        }))
    }

    fn batch_send(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
        let text = json_str(params, "text").unwrap_or_default();
        let enter = params
            .get("enter")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        self.send_text_input_detached(&tab_id, &text, enter);
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_rename(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
        let title = json_str(params, "title").ok_or("title required")?;
        self.rename_tab(&tab_id, &title)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_close(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
        self.close_tab(&tab_id)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_role(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
        let role = json_str(params, "role").ok_or("role required")?;
        let mut role_probe = SpawnAgentParams {
            role: Some(role),
            role_profile: json_str(params, "role_profile"),
            ..Default::default()
        };
        role_probe.normalize_role_profile()?;
        let role = role_probe.role.unwrap_or_else(|| "Worker".into());
        let parent = json_str(params, "parent_tab_id");
        let can_spawn = params.get("can_spawn").and_then(|v| v.as_bool());
        let parsed_role = Self::parse_role(&role)?;
        let was_orchestrator = {
            let mux = self.mux.read();
            mux.get_tab(&tab_id)
                .map(|p| p.role == Some(crate::multiplexer::types::AgentRole::Orchestrator))
                .unwrap_or(false)
        };
        self.set_tab_role(&tab_id, &role, parent.as_deref(), can_spawn)?;
        if let Some(profile) = role_probe.role_profile {
            if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
                tab.role_profile = Some(profile);
            }
            self.emit_state_changed();
        }
        if parsed_role == crate::multiplexer::types::AgentRole::Orchestrator {
            self.maybe_deliver_orchestrator_promotion_onboarding(&tab_id, was_orchestrator);
        }
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
        let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
        let can_spawn = params
            .get("can_spawn")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        self.set_can_spawn(&tab_id, can_spawn)?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn batch_report(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
        let message = json_str(params, "message").ok_or("message required")?;
        let details = serde_json::from_value(params.clone()).unwrap_or_default();
        let result = self.submit_report(&tab_id, &message, details, None, None, None)?;
        Ok(serde_json::json!({"ok": true, "report_id": result.report_id}))
    }
}
