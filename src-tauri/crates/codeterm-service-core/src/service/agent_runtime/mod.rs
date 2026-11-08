use crate::service::MuxEmitCause;
mod abort_provenance;
pub mod acp_profiles;
mod auth_env;
pub(crate) mod authority;
pub mod background;
pub(crate) mod blocking;
mod bootstrap;
pub(crate) mod capacity_retry;
pub(crate) mod capture;
pub mod children;
mod cli_exception;
mod client_bridge;
mod completions;
pub(crate) mod controls;
pub mod declared_controls;
pub mod engine;
mod envelope_delivery;
mod environment;
mod events;
pub mod full_access;
pub mod goals;
mod host_mcp;
mod inbound;
mod initialize;
mod launch_options;
pub mod launch_profile;
pub mod launch_profile_install;
pub mod launch_profile_setup;
#[cfg(unix)]
pub(crate) mod lease;
pub mod login;
pub(crate) mod lost_session;
pub mod model;
mod owned_context;
#[cfg(any(unix, test))]
mod ownership;
pub(crate) mod plan_warning;
pub(crate) mod process;
mod process_boundary;
mod process_slot;
pub(crate) mod profile_options;
pub(crate) mod projections;
pub mod reset_credits;
pub mod thread_history;
mod transcript;
pub(crate) mod update;
pub use projections::for_tab as usage_for_tab;
pub use projections::latest as projections_latest;
mod permission_policy;
pub(crate) mod prompt_answers;
mod question_delivery;
pub mod questions;
pub(crate) mod restart;
pub(crate) mod restore;
pub use restore::all as restore_all;
mod restore_attempt;
mod rpc;
mod selections;
pub(crate) mod start_failure;
mod subscriber;
pub mod transport;
mod turn_requests;
pub(crate) mod user_delivery;
mod working_directory;

pub fn requested_for_tab(svc: &crate::service::ServiceContext, tab: &str) -> bool {
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .filter(|tab| tab.restorable_launch.execution_mode.is_none())
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
    engine::backfill::retry(svc, tab, &session).map_err(|reason| reason.to_string())?;
    let snapshot = engine::snapshot(&session);
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
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

pub use restore::tab as restore_tab;
