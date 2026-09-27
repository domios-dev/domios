use crate::{delayed_send, service::ServiceState};

#[tauri::command]
pub fn delayed_send_list(
    svc: tauri::State<'_, ServiceState>,
    tab_id: Option<String>,
) -> Result<Vec<delayed_send::DelayedMessageView>, String> {
    delayed_send::list_views(&svc, tab_id)
}

#[tauri::command]
pub fn delayed_send_create(
    svc: tauri::State<'_, ServiceState>,
    tab_id: String,
    text: String,
    scheduled_at_ms: Option<u64>,
    created_by_tab: Option<String>,
    recurrence: Option<delayed_send::recurrence::RecurrenceRule>,
) -> Result<delayed_send::DelayedMessageView, String> {
    delayed_send::create_message(
        &svc,
        tab_id,
        text,
        scheduled_at_ms,
        created_by_tab,
        recurrence,
    )
}

#[tauri::command]
pub fn delayed_send_update(
    svc: tauri::State<'_, ServiceState>,
    id: String,
    text: Option<String>,
    scheduled_at_ms: Option<u64>,
    stash: Option<bool>,
    recurrence: Option<delayed_send::recurrence::RecurrenceRule>,
) -> Result<delayed_send::DelayedMessageView, String> {
    delayed_send::update_message(&svc, id, text, scheduled_at_ms, stash, recurrence)
}

#[tauri::command]
pub fn delayed_send_delete(
    svc: tauri::State<'_, ServiceState>,
    id: String,
) -> Result<delayed_send::DelayedMessageView, String> {
    delayed_send::delete_message(&svc, id)
}

#[tauri::command]
pub async fn delayed_send_send_now(
    svc: tauri::State<'_, ServiceState>,
    id: String,
) -> Result<(), String> {
    delayed_send::send_message_now(svc.inner().clone(), id).await
}
