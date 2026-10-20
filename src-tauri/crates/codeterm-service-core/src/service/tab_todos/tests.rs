use super::recitation::step_line;
use super::*;
use codeterm_types::agent::{MAX_TAB_TODO_SUMMARY_BYTES, MAX_TAB_TODO_SUMMARY_TITLES};

#[test]
fn acceptance_seed_titles_count_unicode_and_record_truncation() {
    let source = "🦀".repeat(121);
    let (title, truncated) = bounded_title(&source);
    assert_eq!(title.chars().count(), 120);
    assert!(truncated);
    assert!(title.is_char_boundary(title.len()));
}

#[test]
fn recitation_payload_is_bounded_and_contains_no_more_than_three_titles() {
    let recitation = TabTodoRecitation {
        tab_id: "tab".into(),
        generation: 2,
        revision: 7,
        items: (0..MAX_TAB_TODO_ITEMS)
            .map(|index| TabTodoRecitedItem {
                id: format!("item-{index}"),
                title: "🦀".repeat(120),
                state: TabTodoState::Blocked,
                reason: Some("я".repeat(200)),
                attempt: 1,
            })
            .collect(),
    };
    let text = recitation_text(&recitation);
    assert!(text.len() <= MAX_TAB_TODO_SUMMARY_BYTES);
    assert!(text.matches("\n-").count() <= MAX_TAB_TODO_SUMMARY_TITLES);
}

#[test]
fn external_url_requires_http_or_https_host() {
    assert!(validate_external_url("https://example.test/task/1").is_ok());
    assert!(validate_external_url("file:///tmp/task").is_err());
}

#[test]
fn parent_list_returns_owner_ids_and_states_and_refuses_unrelated_tab() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use crate::service::test_fixture::ServiceFixture;

    let (mut mux, parent_id) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let worker_id = mux.create_tab(&workspace_id).unwrap();
    let unrelated_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&parent_id).unwrap();
    mux.set_tab_role_kind(&worker_id, AgentRole::Worker, None, Some(&parent_id))
        .unwrap();
    let fixture = ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();

    let seeded = crate::async_runtime::block_on(fixture.service.tab_todo_seed_from_acceptance(
        &worker_id,
        &parent_id,
        &["Review the result".into()],
    ))
    .unwrap();
    let item_id = seeded.items[0].id.clone();
    let amended = crate::async_runtime::block_on(fixture.service.tab_todo_amend(
        &worker_id,
        &parent_id,
        seeded.generation,
        item_id.clone(),
        seeded.items[0].revision,
        TabTodoDraft {
            title: "Review the result".into(),
            description: None,
            link: Some(codeterm_types::agent::TabTodoLinkDraft::Url {
                url: "https://example.test/review".into(),
                label: "Review notes".into(),
            }),
        },
    ))
    .unwrap();
    let owner_result = crate::async_runtime::block_on(fixture.service.tab_todo_set_state(
        &worker_id,
        Some(&worker_id),
        seeded.generation,
        item_id.clone(),
        amended.items[0].revision,
        TabTodoState::Done,
        None,
    ))
    .unwrap();

    let owner_list =
        crate::async_runtime::block_on(fixture.service.tab_todo_list(&worker_id, Some(&worker_id)))
            .unwrap()
            .unwrap();

    let parent_result = crate::async_runtime::block_on(
        fixture.service.tab_todo_parent_list(&worker_id, &parent_id),
    )
    .unwrap()
    .unwrap();
    assert_eq!(parent_result, owner_list);
    assert_eq!(parent_result.items[0].id, item_id);
    assert_eq!(parent_result.items[0].state, owner_result.items[0].state);
    assert_eq!(
        parent_result.items[0].revision,
        owner_result.items[0].revision
    );
    assert!(crate::async_runtime::block_on(
        fixture
            .service
            .tab_todo_parent_list(&worker_id, &unrelated_id)
    )
    .is_err());
}

mod lifecycle;
