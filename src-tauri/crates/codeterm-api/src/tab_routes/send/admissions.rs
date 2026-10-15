use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionQuery {
    client_tokens: Vec<String>,
}

pub async fn chat_send_admissions(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(query): Json<AdmissionQuery>,
) -> Result<
    Json<Vec<codeterm_service_core::service::pending_delivery_payloads::user_sends::UserAdmissionEvidence>>,
    SendRejection,
>{
    check_auth(&state, &headers).map_err(SendRejection::from)?;
    let tab_id = resolve_tab(&state, &id).map_err(SendRejection::from)?;
    let svc = state.svc.clone();
    let evidence = tokio::task::spawn_blocking(move || {
        let statuses = svc.reconcile_user_send_statuses(&tab_id)?;
        let tokens = statuses
            .into_iter()
            .filter_map(|status| status.correlation_id)
            .collect::<Vec<_>>();
        if svc.tab_chat_backing(&tab_id).is_some() {
            return Err("durable PTY admission proof unavailable for chat backend".to_string());
        }
        svc.pending_delivery_payloads.user_sends.admission_evidence(
            &tab_id,
            &query.client_tokens,
            &tokens,
        )
    })
    .await
    .map_err(|_| {
        SendRejection::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "admission proof task failed",
        ))
    })?
    .map_err(|error| SendRejection {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: serde_json::json!({ "ok": false, "error": { "code": "admission_proof_unavailable", "message": error } }),
    })?;
    Ok(Json(evidence))
}
