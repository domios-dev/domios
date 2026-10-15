use codeterm_service_core::service::{json_str, resolve_refs, ServiceContext};
use codeterm_service_core::service::{BatchOp, SpawnAgentParams};

/// Execute a batch of commands sequentially. Supports $ref resolution.
pub async fn batch(svc: &ServiceContext, commands: Vec<BatchOp>) -> Vec<serde_json::Value> {
    let mut refs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut results: Vec<serde_json::Value> = Vec::new();

    for cmd in &commands {
        let params = resolve_refs(&cmd.params, &refs);

        let result = match cmd.method.as_str() {
            "spawn" => batch_spawn(svc, &params).await,
            "send" => batch_send(svc, &params),
            "rename" => batch_rename(svc, &params),
            "close" => batch_close(svc, &params).await,
            "role" => batch_role(svc, &params),
            "notify" => batch_notify(svc, &params),
            "can-spawn" => batch_can_spawn(svc, &params),
            "report" => batch_report(svc, &params),
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

    svc.emit_state_changed();
    results
}

async fn batch_spawn(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p = SpawnAgentParams::from_json(params)?;
    let result = svc.spawn_agent(p, None).await?;
    Ok(serde_json::json!({
        "tab_id": result.tab_id,
        "pty_id": result.pty_id,
        "agent": result.agent,
        "title": result.title,
        "role": result.role,
    }))
}

fn batch_send(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
    let text = json_str(params, "text").unwrap_or_default();
    let enter = params
        .get("enter")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if enter {
        svc.send_text_detached(&tab_id, &text);
    } else {
        svc.insert_text_detached(&tab_id, &text);
    }
    Ok(serde_json::json!({"ok": true}))
}

fn batch_rename(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
    let title = json_str(params, "title").ok_or("title required")?;
    svc.rename_tab(&tab_id, &title)?;
    Ok(serde_json::json!({"ok": true}))
}

async fn batch_close(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
    svc.close_tab_async(&tab_id).await?;
    Ok(serde_json::json!({"ok": true}))
}

fn batch_role(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
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
    let (parsed_role, parsed_profile) =
        ServiceContext::parse_role_kind(&role, role_probe.role_profile.as_deref())?;
    let becomes_anchor = codeterm_types::agent::role_kind::anchors_team(
        Some(parsed_role),
        parsed_profile.as_deref(),
    );
    let was_orchestrator = {
        let mux = svc.mux.read();
        mux.get_tab(&tab_id)
            .map(|p| p.anchors_team())
            .unwrap_or(false)
    };
    svc.set_tab_role_with_profile(
        &tab_id,
        &role,
        parsed_profile.as_deref(),
        parent.as_deref(),
        can_spawn,
    )?;
    if becomes_anchor {
        crate::maybe_deliver_orchestrator_promotion_onboarding(svc, &tab_id, was_orchestrator);
    }
    Ok(serde_json::json!({"ok": true}))
}

fn batch_notify(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let title = json_str(params, "title").unwrap_or_else(|| "Notification".to_string());
    let body = json_str(params, "body");
    let severity = json_str(params, "severity");
    let id = svc.log_system_notice(title, body, severity.as_deref());
    Ok(serde_json::json!({"ok": true, "id": id}))
}

fn batch_can_spawn(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
    let can_spawn = params
        .get("can_spawn")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    svc.set_can_spawn(&tab_id, can_spawn)?;
    Ok(serde_json::json!({"ok": true}))
}

fn batch_report(
    svc: &ServiceContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let tab_id = json_str(params, "tab_id").ok_or("tab_id required")?;
    let message = json_str(params, "message").ok_or("message required")?;
    let details = serde_json::from_value(params.clone()).unwrap_or_default();
    let result = svc
        .submit_report(&tab_id, &message, details, None, None, None)
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({"ok": true, "report_id": result.report_id}))
}
