pub use codeterm_service_core::test_support::*;
pub fn fixture(
    mux: codeterm_service_core::multiplexer::state::MultiplexerState,
) -> codeterm_service_core::service::test_fixture::ServiceFixture {
    crate::register_neighbors();
    codeterm_service_core::service::test_fixture::ServiceFixture::new(mux)
}
pub fn fixture_at(
    mux: codeterm_service_core::multiplexer::state::MultiplexerState,
    root: &std::path::Path,
) -> codeterm_service_core::service::test_fixture::ServiceFixture {
    crate::register_neighbors();
    codeterm_service_core::service::test_fixture::ServiceFixture::new_at(mux, root)
}
