use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn failed_plan_read_defers_only_its_tab_and_keeps_other_restore_decisions() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let shell = mux.create_tab(&workspace).unwrap();
    let failed = mux.create_tab(&workspace).unwrap();
    let continuing = mux.create_tab(&workspace).unwrap();
    for id in [&failed, &continuing] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("opaque".into());
        tab.restored_from_snapshot = true;
    }
    mux.get_tab_mut(&shell).unwrap().protected = true;
    let fixture = ServiceFixture::new(mux);
    initialize_with_continuing(&fixture.service, 48, |tab| {
        assert_ne!(tab, shell);
        if tab == failed {
            Err("selected tab unavailable".into())
        } else {
            Ok(true)
        }
    });
    let mux = fixture.service.mux.read();
    assert!(mux.get_tab(&failed).unwrap().frozen);
    assert!(!mux.get_tab(&continuing).unwrap().frozen);
    assert!(!mux.get_tab(&shell).unwrap().frozen);
}
