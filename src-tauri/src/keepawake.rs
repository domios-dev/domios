use codeterm_host_runtime::app_runtime::AppHandle;
pub use codeterm_service_core::keepawake::*;
use codeterm_service_core::ports::host::UiEvents;
use std::sync::Arc;
fn ui(app: &AppHandle) -> Arc<dyn UiEvents> {
    Arc::new(crate::host_ports::TauriUiEvents(app.clone()))
}
pub fn resume_from_disk(state: &KeepAwakeState, app: &AppHandle) {
    codeterm_service_core::keepawake::resume_from_disk(state, &ui(app));
}
pub fn start_supervisor(state: KeepAwakeState, app: AppHandle) {
    codeterm_service_core::keepawake::start_supervisor(state, ui(&app));
}
