pub mod apply;
pub mod channel;
pub mod checker;
mod debian_verify;
pub mod discovery;
pub mod failure_report;
mod install_holders;
pub mod manifest;
pub mod orchestrate;
pub mod shutdown;
pub mod single_flight;
pub mod viability;

pub use channel::Channel;
pub use codeterm_service_core::update_state::{InstallMethod, UpdatePhase, UpdateState};
pub use manifest::Manifest;

pub fn register_update_hooks() {
    codeterm_service_core::ports::update::register(
        codeterm_service_core::ports::update::UpdateHooks {
            bind_status_emitter,
            check: check_hook,
        },
    );
}

fn check_hook<'a>(
    app: &'a codeterm_service_core::app_runtime::AppHandle,
    state: &'a codeterm_service_core::UpdateStateHandle,
    bundles: Option<std::sync::Arc<codeterm_frontend_bundle::FrontendBundles>>,
    force: bool,
) -> codeterm_service_core::ports::update::CheckFuture<'a> {
    Box::pin(discovery::check(app, state, bundles, force))
}

pub fn bind_status_emitter(app: codeterm_service_core::app_runtime::AppHandle) {
    let _ = codeterm_service_core::update_state::STATUS_EMITTER.set(Box::new(move |status| {
        codeterm_events::emit_everywhere(&app, "update-status-changed", status);
    }));
}
