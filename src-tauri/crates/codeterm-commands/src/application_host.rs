use std::{path::PathBuf, sync::Arc};

use codeterm_service_core::service::ServiceContext;
#[cfg(feature = "desktop")]
use codeterm_service_core::UpdateStateHandle;

use crate::update::apply::{ApplyOptions, ApplyResult, DebianPreflight, DebianUpdatePlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeUnavailable;

impl std::fmt::Display for NativeUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native_capability_unavailable")
    }
}

impl std::error::Error for NativeUnavailable {}

#[async_trait::async_trait]
pub trait ApplicationHost: Send + Sync {
    fn restart(&self) -> Result<(), String>;
    fn exit(&self, code: i32) -> Result<(), String>;
    async fn copy_image_to_clipboard(&self, path: String) -> Result<(), String>;
    fn debian_preflight(&self) -> DebianPreflight;
    async fn check_and_apply(&self, options: ApplyOptions) -> Result<ApplyResult, String>;
    async fn check_stage_deb_update(
        &self,
        options: &ApplyOptions,
    ) -> Result<DebianUpdatePlan, String>;
    async fn install_staged_deb(
        &self,
        version: String,
        path: PathBuf,
        initiating_tab: Option<String>,
    ) -> Result<(), String>;
}

#[derive(Clone)]
pub struct ApplicationHostState(pub Arc<dyn ApplicationHost>);

pub fn require(svc: &ServiceContext) -> Result<Arc<dyn ApplicationHost>, NativeUnavailable> {
    svc.host_extension::<ApplicationHostState>()
        .map(|state| state.0.clone())
        .ok_or(NativeUnavailable)
}

#[cfg(feature = "desktop")]
pub struct DesktopApplicationHost {
    pub app: codeterm_host_runtime::app_runtime::AppHandle,
    pub update_state: UpdateStateHandle,
}

#[cfg(feature = "desktop")]
#[async_trait::async_trait]
impl ApplicationHost for DesktopApplicationHost {
    fn debian_preflight(&self) -> DebianPreflight {
        crate::update::apply::debian_preflight_status()
    }

    async fn copy_image_to_clipboard(&self, path: String) -> Result<(), String> {
        crate::commands::copy_image_to_clipboard(path).await
    }

    fn restart(&self) -> Result<(), String> {
        self.app.restart()
    }

    fn exit(&self, code: i32) -> Result<(), String> {
        self.app.exit(code);
        Ok(())
    }

    async fn check_and_apply(&self, options: ApplyOptions) -> Result<ApplyResult, String> {
        crate::update::apply::check_and_apply(&self.app, options, self.update_state.clone()).await
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
        initiating_tab: Option<String>,
    ) -> Result<(), String> {
        crate::update::apply::install_staged_deb_and_restart(
            &self.app,
            version,
            path,
            initiating_tab.as_deref(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn process_split_contract_native_absence_is_typed() {
        let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
        assert!(matches!(
            super::require(&fixture.service),
            Err(super::NativeUnavailable)
        ));
        assert!(fixture.service.mux.read().snapshot().tabs.len() > 0);
    }

    #[test]
    fn process_split_contract_process_paths_survive_native_absence() {
        use codeterm_service_core::ports::host::{
            NoShell, ProcessPaths, ProcessShell, Shell, ShellError,
        };
        let root = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let shell = ProcessShell {
            paths: ProcessPaths {
                executable: executable.clone(),
                config: root.path().join("config"),
                resources: root.path().join("resources"),
            },
            native: std::sync::Arc::new(NoShell),
        };
        assert_eq!(shell.current_exe().unwrap(), executable);
        assert_eq!(shell.config_dir().unwrap(), root.path().join("config"));
        assert_eq!(shell.resource_dir().unwrap(), root.path().join("resources"));
        assert_eq!(shell.focus_window("main"), Err(ShellError::Unavailable));
    }
}
