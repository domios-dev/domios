//! The fixture shared by the plan-store consumer load tests here and in the lifecycle crate.

use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

pub fn fixture() -> (ServiceFixture, String) {
    let (mux, orchestrator) = MultiplexerState::new();
    (ServiceFixture::new(mux), orchestrator)
}

pub fn project(fx: &ServiceFixture) -> std::path::PathBuf {
    fx.temp.path().to_path_buf()
}
