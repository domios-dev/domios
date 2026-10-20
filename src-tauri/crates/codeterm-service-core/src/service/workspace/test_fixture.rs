use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

pub fn workspace_with_agent_tab() -> (ServiceFixture, String, String, String) {
    let (mut mux, caller_tab) = MultiplexerState::new();
    let workspace_id = mux.create_workspace("Agent workspace".into(), None).id;
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.title = "Release guard agent".into();
    (ServiceFixture::new(mux), workspace_id, tab_id, caller_tab)
}
