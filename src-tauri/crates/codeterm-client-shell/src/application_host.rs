use std::{path::PathBuf, sync::Arc};

use codeterm_update::application_host::ApplicationHost;
use codeterm_update::apply::{ApplyOptions, ApplyResult, DebianPreflight, DebianUpdatePlan};
use codeterm_update::UpdateStateHandle;

use crate::app_runtime::AppHandle;

/// The native client stops its attached server before an installer replaces the pair.
#[async_trait::async_trait]
pub trait ServerStop: Send + Sync {
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
    fn debian_preflight(&self) -> DebianPreflight {
        crate::update::apply::debian_preflight_status()
    }

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

    async fn check_stage_deb_update(
        &self,
        options: &ApplyOptions,
    ) -> Result<DebianUpdatePlan, String> {
        crate::update::apply::check_stage_deb_update(&self.app, options, self.update_state.clone())
            .await
    }

    async fn install_staged_deb(
        &self,
        version: String,
        path: PathBuf,
        _initiating_tab: Option<String>,
    ) -> Result<(), String> {
        crate::update::apply::install_staged_deb_and_restart(
            &self.app,
            version,
            path,
            self.server_stop.clone(),
        )
        .await
    }
}
