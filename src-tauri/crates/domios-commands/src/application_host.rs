use std::sync::Arc;

use codeterm_service_core::service::ServiceContext;
use codeterm_update::application_host::{ApplicationHost, NativeUnavailable};

#[derive(Clone)]
pub struct ApplicationHostState(pub Arc<dyn ApplicationHost>);

pub fn require(svc: &ServiceContext) -> Result<Arc<dyn ApplicationHost>, NativeUnavailable> {
    svc.host_extension::<ApplicationHostState>()
        .map(|state| state.0.clone())
        .ok_or(NativeUnavailable)
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
