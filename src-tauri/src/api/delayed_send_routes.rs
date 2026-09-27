use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_tab, AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDelayedMessageBody {
    text: String,
    scheduled_at_ms: Option<u64>,
    created_by_tab: Option<String>,
    recurrence: Option<crate::delayed_send::recurrence::RecurrenceRule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDelayedMessageBody {
    text: Option<String>,
    scheduled_at_ms: Option<u64>,
    stash: Option<bool>,
    recurrence: Option<crate::delayed_send::recurrence::RecurrenceRule>,
}

pub async fn list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<Vec<crate::delayed_send::DelayedMessageView>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    let messages = crate::delayed_send::list_views(&state.svc, Some(tab_id))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(messages))
}

pub async fn create(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<CreateDelayedMessageBody>,
) -> Result<Json<crate::delayed_send::DelayedMessageView>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    let msg = crate::delayed_send::create_message(
        &state.svc,
        tab_id,
        body.text,
        body.scheduled_at_ms,
        body.created_by_tab,
        body.recurrence,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(msg))
}

pub async fn update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateDelayedMessageBody>,
) -> Result<Json<crate::delayed_send::DelayedMessageView>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let msg = crate::delayed_send::update_message(
        &state.svc,
        id,
        body.text,
        body.scheduled_at_ms,
        body.stash,
        body.recurrence,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(msg))
}

pub async fn delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::delayed_send::DelayedMessageView>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let msg = crate::delayed_send::delete_message(&state.svc, id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(msg))
}

pub async fn send_now(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    crate::delayed_send::send_message_now(state.svc.clone(), id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::{CreateDelayedMessageBody, UpdateDelayedMessageBody};
    use crate::delayed_send::{
        recurrence::RecurrenceRule, DelayedMessage, DelayedMessageState, DelayedMessageView,
    };

    fn rule() -> RecurrenceRule {
        RecurrenceRule {
            every_ms: 600_000,
            until_ms: Some(1_200_000),
            count: None,
        }
    }

    #[test]
    fn delayed_create_edit_and_list_contracts_carry_recurrence() {
        let create: CreateDelayedMessageBody = serde_json::from_value(serde_json::json!({
            "text": "tick",
            "scheduledAtMs": 600_000,
            "createdByTab": "creator",
            "recurrence": { "everyMs": 600_000, "untilMs": 1_200_000 }
        }))
        .unwrap();
        assert_eq!(create.recurrence, Some(rule()));

        let edit: UpdateDelayedMessageBody = serde_json::from_value(serde_json::json!({
            "recurrence": { "everyMs": 600_000, "count": 3 }
        }))
        .unwrap();
        let edited = edit.recurrence.unwrap();
        assert_eq!(edited.every_ms, 600_000);
        assert_eq!(edited.count, Some(3));

        let listed = DelayedMessageView {
            message: DelayedMessage {
                id: "dmsg-1".into(),
                tab_id: "tab-1".into(),
                workspace_id: None,
                text: "tick".into(),
                scheduled_at_ms: 1_200_000,
                created_at_ms: 0,
                state: DelayedMessageState::Queued,
                error: None,
                session_id_at_create: None,
                created_by_tab: Some("creator".into()),
                created_by_label: None,
                created_by_model: None,
                recurrence: Some(rule()),
                series_start_at_ms: Some(600_000),
                tick_index: 1,
                skipped_ticks: 2,
                team_id: Some("team-1".into()),
                last_tick_unconfirmed: false,
            },
            current_session_id: None,
            session_changed: false,
        };
        let value = serde_json::to_value(listed).unwrap();
        assert_eq!(value["recurrence"]["everyMs"], 600_000);
        assert_eq!(value["scheduledAtMs"], 1_200_000);
        assert_eq!(value["skippedTicks"], 2);
    }
}
