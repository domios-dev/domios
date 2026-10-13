pub(crate) mod authority;
pub mod engine;
mod initialize;
#[cfg(unix)]
pub(crate) mod lease;
pub mod model;
#[cfg(any(unix, test))]
mod ownership;
mod process;
mod question_delivery;
pub mod questions;
mod rpc;
mod subscriber;
pub mod transport;
pub(crate) mod user_delivery;

pub fn requested_for_tab(svc: &crate::service::ServiceContext, tab: &str) -> bool {
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.agent_type.clone());
    provider.is_some_and(|provider| engine::requested(svc, &provider))
}

pub async fn retry(
    svc: &crate::service::ServiceContext,
    tab: &str,
) -> Result<model::RuntimeSnapshot, String> {
    if !requested_for_tab(svc, tab) {
        return Err("Tab has no agent runtime capability".into());
    }
    let attempt = engine::ensure(svc, tab).await;
    let session = attempt
        .ok()
        .or_else(|| engine::for_tab(tab))
        .ok_or_else(|| "Runtime binding unavailable".to_string())?;
    engine::validate_binding(svc, &session).map_err(|reason| reason.to_string())?;
    let snapshot = session.state.lock().snapshot();
    svc.emit_state_changed();
    Ok(snapshot)
}

pub async fn answer_question(
    svc: &crate::service::ServiceContext,
    tab: &str,
    item: &str,
    answers: Vec<codeterm_plugin::agent_runtime::RuntimeQuestionAnswer>,
) -> Result<crate::service::chat_send_transaction::ChatSendStatus, String> {
    let svc = svc.clone();
    let tab = tab.to_string();
    let item = item.to_string();
    tokio::task::spawn_blocking(move || question_delivery::answer(&svc, &tab, &item, answers))
        .await
        .map_err(|reason| reason.to_string())?
}
