use super::*;

/// Hook endpoint: agent stopped (task complete). Called by the Stop hook of
/// whichever provider declares the stop route in its roster.
pub async fn hook_stop(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    OptionalJson(body): OptionalJson<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_hook_tab(&state, &raw_tab_id, "stop")?;
    if let Some(response) = hook_authority::telemetry_response(&tab_id, "stop") {
        return Ok(response);
    }
    log::debug!("[hook_stop] fired for Tab {}", tab_id);
    let body = body.unwrap_or(serde_json::Value::Null);
    record_hook_post(&state, &tab_id, "stop", Some(&body));
    let report_nonce = body
        .get("binding_nonce")
        .and_then(|v| v.as_str())
        .or_else(|| header_str(&headers, "x-codeterm-binding-nonce"))
        .map(|s| s.to_string());
    let launch_nonce = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.session_binding_nonce.clone());
    let verdict = stop_report_verdict(launch_nonce.as_deref(), report_nonce.as_deref());
    if verdict == StopReportVerdict::Foreign {
        log::warn!(
            "[hook-refusal] tab={} event=stop reason=nonce_mismatch",
            tab_id
        );
        return Err((StatusCode::CONFLICT, "binding nonce mismatch".to_string()));
    }
    let title = state
        .svc
        .mux
        .write()
        .get_tab(&tab_id)
        .map(|p| p.title.clone());
    if let Some(title) = title {
        state.svc.notify_event(
            "agent.task_complete",
            serde_json::json!({
                "tab_id": &tab_id,
                "title": format!("{} finished", title),
                "body": "Task complete",
            }),
        );
    }

    // Stop = turn boundary, not exit — agent_type stays; PTY-close clears it.
    state.svc.prompt_detector.lock().clear(&tab_id);
    permission_decision::clear_tab(&tab_id);
    if let Some(identity) = state.svc.clear_pending_prompt(&tab_id) {
        state.svc.resolve_prompt_notifications(&tab_id, &identity);
    }
    state.svc.emit_state_changed();

    // A nonce-verified report is the completion-via-report path; a nonce-less legacy post keeps today's tab-keyed behavior.
    match (verdict, report_nonce.as_deref()) {
        (StopReportVerdict::Verified, Some(nonce)) => state
            .svc
            .run_watcher
            .complete_via_stop_report(&state.svc, &tab_id, nonce),
        _ => state
            .svc
            .run_watcher
            .complete_via_stop_hook(&state.svc, &tab_id),
    }
    state.svc.on_agent_turn_settled(&tab_id);
    state.svc.complete_worker_turn_without_report(
        &tab_id,
        crate::service::WorkerTurnSignal::ProviderStop,
    );
    state.svc.schedule_session_model_refresh_after_stop(&tab_id);
    if verdict == StopReportVerdict::Verified {
        state.svc.mark_agent_turn_complete(&tab_id);
        if let (Some(session_id), Some(nonce)) =
            (session_start_session_id(&body), report_nonce.as_deref())
        {
            state
                .svc
                .staged_model_turn_completed(&tab_id, &session_id, nonce);
        }
    }
    state.svc.reconcile_chat_send_hook(&tab_id).await;

    Ok(Json(serde_json::json!({"ok": true})))
}
