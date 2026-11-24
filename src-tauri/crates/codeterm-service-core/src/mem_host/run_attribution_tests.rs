use super::with_worker_write;
use crate::{
    mem::workers::make_info, multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};

#[test]
fn run_attribution_uses_lifecycle_owned_worker_and_restores_nested_origin() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let mut worker = make_info(
        "gw_owner".into(),
        tab.clone(),
        "dreaming:rem".into(),
        "test".into(),
        "blue".into(),
    );
    svc.workers.register(worker.clone());
    svc.mux.write().get_tab_mut(&tab).unwrap().worker_id = Some("gw_owner".into());
    with_worker_write(svc, Some(&tab), || {
        assert_eq!(
            crate::mem::store::current_write_worker().as_deref(),
            Some("gw_owner")
        );
        with_worker_write(svc, None, || {
            assert_eq!(crate::mem::store::current_write_worker(), None)
        });
        assert_eq!(
            crate::mem::store::current_write_worker().as_deref(),
            Some("gw_owner")
        );
    });
    assert_eq!(crate::mem::store::current_write_worker(), None);
    worker.tab_id = "another_tab".into();
    svc.workers.register(worker);
    with_worker_write(svc, Some(&tab), || {
        assert_eq!(crate::mem::store::current_write_worker(), None)
    });
}
