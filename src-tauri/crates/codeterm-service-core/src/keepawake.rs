pub use codeterm_keepawake::*;
use tauri::{AppHandle, Runtime};

struct AppStatusEmitter<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Clone for AppStatusEmitter<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: Runtime> StatusEmitter for AppStatusEmitter<R> {
    fn emit_status(&self, status: &KeepAwakeStatus) {
        crate::events::emit_everywhere(&self.0, "keepawake-changed", status);
    }
}

pub fn start<R: Runtime>(
    state: &KeepAwakeState,
    app: &AppHandle<R>,
    duration_secs: Option<u64>,
    display: Option<bool>,
    gnome_policy: Option<bool>,
    system_block: Option<bool>,
) -> Result<KeepAwakeStatus, String> {
    codeterm_keepawake::start(
        state,
        &AppStatusEmitter(app.clone()),
        duration_secs,
        display,
        gnome_policy,
        system_block,
    )
}

pub fn stop<R: Runtime>(state: &KeepAwakeState, app: &AppHandle<R>) -> Result<(), String> {
    codeterm_keepawake::stop(state, &AppStatusEmitter(app.clone()))
}

pub fn resume_from_disk<R: Runtime>(state: &KeepAwakeState, app: &AppHandle<R>) {
    codeterm_keepawake::resume_from_disk(state, &AppStatusEmitter(app.clone()));
}

pub fn start_supervisor<R: Runtime>(state: KeepAwakeState, app: AppHandle<R>) {
    codeterm_keepawake::start_supervisor(state, AppStatusEmitter(app));
}
