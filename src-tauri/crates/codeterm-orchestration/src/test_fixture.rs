use std::ops::{Deref, DerefMut};
use std::path::Path;

use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture as CoreFixture;

/// The core fixture, built after this crate registered its port and extension keys.
pub(crate) struct ServiceFixture(CoreFixture);

impl ServiceFixture {
    pub(crate) fn new(mux: MultiplexerState) -> Self {
        crate::register_orchestration();
        Self(CoreFixture::new(mux))
    }
}

impl Deref for ServiceFixture {
    type Target = CoreFixture;

    fn deref(&self) -> &CoreFixture {
        &self.0
    }
}

impl DerefMut for ServiceFixture {
    fn deref_mut(&mut self) -> &mut CoreFixture {
        &mut self.0
    }
}

pub(crate) fn open_instance_with_mux(
    root: &Path,
    mux: MultiplexerState,
) -> codeterm_service_core::persistence_test_support::ServiceInstance {
    crate::register_orchestration();
    codeterm_service_core::persistence_test_support::ServiceInstance::open_with_mux(root, mux)
}

/// Team close tears down crons and sentinels through the ports production always registers.
pub(crate) fn install_team_close_ports(
    instance: &codeterm_service_core::persistence_test_support::ServiceInstance,
    root: &Path,
) {
    use codeterm_service_core::cron::clock::MockClock;
    use codeterm_service_core::cron::scheduler::{CronState, CronWake};
    use codeterm_service_core::cron::store::{CronStorageOptions, CronStore};
    use std::sync::Arc;

    let cron: CronState = Arc::new(parking_lot::Mutex::new(
        CronStore::open_storage(
            &instance.storage,
            root.join("crons"),
            MockClock::starting_at(100),
            CronStorageOptions::default(),
            codeterm_service_core::config::schema::AppConfig::default()
                .general_agent
                .agent_provider,
        )
        .unwrap(),
    ));
    let wake: CronWake = Arc::new(std::sync::Condvar::new());
    assert!(instance.fixture.service.set_ext(cron).is_ok());
    assert!(instance.fixture.service.set_ext(wake).is_ok());
    let sentinels = Arc::new(parking_lot::Mutex::new(
        codeterm_sentinel::store::Store::with_storage(
            &instance.storage,
            MockClock::starting_at(100),
        ),
    ));
    crate::team::tests::sentinel_fixture::manage(&instance.fixture.service, &sentinels);
}
