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

pub use crate::update_state::{InstallMethod, UpdatePhase, UpdateState};
pub use channel::Channel;
pub use manifest::Manifest;

pub fn register_update_hooks() {
    crate::ports::update::register(crate::ports::update::UpdateHooks {
        bind_status_emitter,
        check: check_hook,
    });
}

fn check_hook<'a>(
    app: &'a crate::app_runtime::AppHandle,
    state: &'a crate::UpdateStateHandle,
    bundles: Option<std::sync::Arc<codeterm_frontend_bundle::FrontendBundles>>,
    force: bool,
) -> crate::ports::update::CheckFuture<'a> {
    Box::pin(discovery::check(app, state, bundles, force))
}

pub fn bind_status_emitter(app: crate::app_runtime::AppHandle) {
    let _ = crate::update_state::STATUS_EMITTER.set(Box::new(move |status| {
        codeterm_events::emit_everywhere(&app, "update-status-changed", status);
    }));
}
