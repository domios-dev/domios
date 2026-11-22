use super::apply_session_auto_title;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{TaskBinding, TitleLevel};
use crate::service::test_fixture::ServiceFixture;

#[test]
fn session_auto_title_renames_unpinned_tab_without_overwriting_manual_rename() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let agent_tab = mux.create_tab(&workspace_id).unwrap();
    let task_tab = mux.create_tab(&workspace_id).unwrap();
    mux.rename_tab_at_level(&task_tab, "Beacon", TitleLevel::Agent)
        .unwrap();
    mux.set_task_binding_once(
        &task_tab,
        TaskBinding {
            plan_id: Some("pl_test".into()),
            task_id: Some("task_test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let fixture = ServiceFixture::new(mux);

    apply_session_auto_title(&fixture.service, &agent_tab, "  Fix\tauth\u{0007} bug  ");
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&agent_tab).unwrap();
        assert_eq!(tab.title, "Fix auth bug");
        assert_eq!(tab.title_level, TitleLevel::Agent);
    }

    // Manual user rename always wins over subsequent agent auto-titles.
    fixture
        .service
        .mux
        .write()
        .rename_tab(&agent_tab, "My Custom Name")
        .unwrap();
    apply_session_auto_title(&fixture.service, &agent_tab, "Later auto title");
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&agent_tab)
            .unwrap()
            .title,
        "My Custom Name"
    );

    // Task-bound tabs and protected tabs are never overwritten by session_info_update.title.
    apply_session_auto_title(&fixture.service, &task_tab, "Overwritten worker");
    assert_eq!(
        fixture.service.mux.read().get_tab(&task_tab).unwrap().title,
        "Beacon"
    );
    let _ = tab_id;
}
