use std::ops::{Deref, DerefMut};
use std::path::Path;

use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use tauri::Manager;

pub struct HostFixture {
    fixture: ServiceFixture,
    pub app: crate::app_runtime::App,
}

impl HostFixture {
    pub fn new(mux: MultiplexerState) -> Self {
        Self::from_core(ServiceFixture::new(mux))
    }

    pub fn new_at(mux: MultiplexerState, data_dir: &Path) -> Self {
        Self::from_core(ServiceFixture::new_at(mux, data_dir))
    }

    fn from_core(fixture: ServiceFixture) -> Self {
        let app = crate::app_runtime::builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("host fixture app");
        app.manage(
            fixture
                .service
                .ext::<codeterm_service_core::config::ConfigState>()
                .clone(),
        );
        app.manage(fixture.service.mux.clone());
        app.manage(fixture.service.events.clone());
        Self { fixture, app }
    }
}

impl Deref for HostFixture {
    type Target = ServiceFixture;

    fn deref(&self) -> &Self::Target {
        &self.fixture
    }
}

impl DerefMut for HostFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fixture
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_managed_config_shares_core_state_and_preserves_recording() {
        let (mux, _) = MultiplexerState::new();
        let fixture = HostFixture::new(mux);
        let config = fixture
            .app
            .state::<codeterm_service_core::config::ConfigState>();
        assert!(std::sync::Arc::ptr_eq(
            config.inner(),
            fixture
                .service
                .ext::<codeterm_service_core::config::ConfigState>(),
        ));
        fixture
            .service
            .ui
            .emit("fixture-event", serde_json::json!({ "value": 7 }));
        assert_eq!(
            fixture.ui.0.lock().as_slice(),
            &[("fixture-event".into(), serde_json::json!({ "value": 7 }))]
        );
    }
}
