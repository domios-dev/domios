use crate::service::ServiceState;

#[tauri::command]
pub(crate) async fn agent_runtime_retry(
    svc: tauri::State<'_, ServiceState>,
    tab_id: String,
) -> Result<crate::service::agent_runtime::model::RuntimeSnapshot, String> {
    crate::service::agent_runtime::retry(svc.inner(), &tab_id).await
}

#[tauri::command]
pub(crate) async fn agent_runtime_answer_question(
    svc: tauri::State<'_, ServiceState>,
    tab_id: String,
    item_id: String,
    answers: Vec<codeterm_plugin::agent_runtime::RuntimeQuestionAnswer>,
) -> Result<crate::service::chat_send_transaction::ChatSendStatus, String> {
    crate::service::agent_runtime::answer_question(svc.inner(), &tab_id, &item_id, answers).await
}
