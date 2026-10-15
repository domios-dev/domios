use std::ops::{Deref, DerefMut};
use std::path::Path;

use crate::test_fixture::ServiceFixture as CoreFixture;
use codeterm_service_core::multiplexer::state::MultiplexerState;

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
