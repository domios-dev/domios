use std::sync::Arc;

use codeterm_update::application_host::ApplicationHost;
use codeterm_update::apply::{ApplyOptions, ApplyResult};
use codeterm_update::UpdateStateHandle;

use crate::app_runtime::AppHandle;

/// The native client stops its attached server before an installer replaces the pair.
#[async_trait::async_trait]
pub trait ServerStop: Send + Sync {
    #[cfg(target_os = "linux")]
    fn debian_quit_target(&self) -> Result<codeterm_update::debian::handoff::QuitTarget, String> {
        Err("the authenticated update quit target is unavailable".into())
    }
    async fn prepare_update(&self, _version: &str) -> Result<(), String> {
        Ok(())
    }
    async fn prepare_detached_update(&self, version: &str) -> Result<(), String> {
        self.prepare_update(version).await
    }
    fn install_identity(&self) -> Result<AttachedServerIdentity, String> {
        Err("the exact attached server identity is unavailable".into())
    }

    /// Resolves only after the server process exit was observed.
    async fn stop_server(&self) -> Result<(), ServerStopError>;
}

#[derive(Clone)]
pub struct AttachedServerIdentity {
    pub pid: u32,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStopError {
    /// The stop request reached the server, so it may already be gone.
    pub dispatched: bool,
    pub reason: String,
}

pub struct ServerStopState(pub Arc<dyn ServerStop>);

pub struct DesktopApplicationHost {
    pub app: AppHandle,
    pub update_state: UpdateStateHandle,
    pub server_stop: Arc<dyn ServerStop>,
}

#[async_trait::async_trait]
impl ApplicationHost for DesktopApplicationHost {
    async fn copy_image_to_clipboard(&self, path: String) -> Result<(), String> {
        crate::commands::copy_image_to_clipboard(path).await
    }

    fn restart(&self) -> Result<(), String> {
        crate::update::restart::restart(&self.app)
    }

    fn exit(&self, code: i32) -> Result<(), String> {
        self.app.exit(code);
        Ok(())
    }

    async fn check_and_apply(&self, options: ApplyOptions) -> Result<ApplyResult, String> {
        crate::update::apply::check_and_apply(
            &self.app,
            options,
            self.update_state.clone(),
            self.server_stop.clone(),
        )
        .await
    }
}
