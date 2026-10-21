use super::*;
use crate::async_runtime::block_on;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

struct Team {
    fixture: ServiceFixture,
    orchestrator: String,
    worker: String,
    qa: String,
    unrelated: String,
}

fn team() -> Team {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let worker = mux.create_tab(&workspace_id).unwrap();
    let qa = mux.create_tab(&workspace_id).unwrap();
    let unrelated = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    mux.set_tab_role_kind(&worker, AgentRole::Worker, None, Some(&orchestrator))
        .unwrap();
    mux.set_tab_role_kind(&qa, AgentRole::Worker, Some("qa"), Some(&orchestrator))
        .unwrap();
    let fixture = ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();
    Team {
        fixture,
        orchestrator,
        worker,
        qa,
        unrelated,
    }
}

impl Team {
    fn seed(&self, criteria: &[&str]) -> TabTodoChecklist {
        let criteria: Vec<String> = criteria.iter().map(|value| value.to_string()).collect();
        block_on(self.fixture.service.tab_todo_seed_from_acceptance(
            &self.worker,
            &self.orchestrator,
            &criteria,
        ))
        .unwrap()
    }

    fn set(
        &self,
        list: &TabTodoChecklist,
        index: usize,
        state: TabTodoState,
        reason: Option<&str>,
    ) -> Result<TabTodoChecklist, String> {
        let item = &list.items[index];
        block_on(self.fixture.service.tab_todo_set_state(
            &self.worker,
            Some(&self.worker),
            list.generation,
            item.id.clone(),
            item.revision,
            state,
            reason.map(str::to_owned),
        ))
    }
}

#[test]
fn every_checklist_mutation_emits_tab_todo_changed_with_its_revision() {
    let team = team();
    let seeded = team.seed(&["Build", "Test"]);
    let started = team
        .set(&seeded, 0, TabTodoState::InProgress, None)
        .unwrap();
    let reopened = block_on(team.fixture.service.tab_todo_reopen(
        &team.worker,
        Some(&team.qa),
        started.items[0].id.clone(),
        "QA rejected the build".into(),
    ))
    .unwrap();
    let events: Vec<_> = team
        .fixture
        .ui
        .0
        .lock()
        .iter()
        .filter(|(name, _)| name == TAB_TODO_CHANGED_EVENT)
        .map(|(_, payload)| payload.clone())
        .collect();
    let expected: Vec<_> = [&seeded, &started, &reopened]
        .iter()
        .map(|list| {
            serde_json::json!({
                "tab_id": team.worker,
                "generation": list.generation,
                "revision": list.revision,
            })
        })
        .collect();
    assert_eq!(events, expected);
}

#[test]
fn a_completed_step_written_back_to_in_progress_is_a_restart_with_its_reason() {
    let team = team();
    let list = team.seed(&["Build"]);
    let list = team.set(&list, 0, TabTodoState::InProgress, None).unwrap();
    let list = team.set(&list, 0, TabTodoState::Done, None).unwrap();
    let id = list.items[0].id.clone();
    let list = team
        .set(
            &list,
            0,
            TabTodoState::InProgress,
            Some("canonical lane-check failed"),
        )
        .unwrap();
    let step = &list.items[0];
    assert_eq!(step.id, id);
    assert_eq!(step.attempt, 2);
    assert_eq!(step.reason.as_deref(), Some("canonical lane-check failed"));
    assert_eq!(step.counters.started, 2);
    assert_eq!(step.attempt_spans().len(), 2);
    assert!(step.locked);
}

#[test]
fn blocked_and_skipped_are_refused_without_a_reason_and_recite_blocked_first() {
    let team = team();
    let list = team.seed(&["First", "Second", "Third"]);
    assert!(team.set(&list, 2, TabTodoState::Blocked, None).is_err());
    assert!(team.set(&list, 1, TabTodoState::Skipped, None).is_err());
    let list = team.set(&list, 0, TabTodoState::InProgress, None).unwrap();
    let list = team
        .set(
            &list,
            2,
            TabTodoState::Blocked,
            Some("waiting on the parent"),
        )
        .unwrap();
    let list = team
        .set(&list, 1, TabTodoState::Skipped, Some("covered elsewhere"))
        .unwrap();
    let recitation = block_on(
        team.fixture
            .service
            .tab_todo_recite(&team.worker, Some(&team.worker)),
    )
    .unwrap()
    .unwrap();
    let states: Vec<_> = recitation.items.iter().map(|item| item.state).collect();
    assert_eq!(
        states,
        [
            TabTodoState::Blocked,
            TabTodoState::InProgress,
            TabTodoState::Skipped
        ]
    );
    assert_eq!(
        recitation.items[0].reason.as_deref(),
        Some("waiting on the parent")
    );
    let summary = block_on(
        team.fixture
            .service
            .tab_todo_parent_summary(&team.worker, &team.orchestrator),
    )
    .unwrap()
    .unwrap();
    assert_eq!(summary.blocked_count, 1);
    assert_eq!(summary.skipped_count, 1);
    assert_eq!(summary.unfinished_count, 2);
    let line = step_line(&recitation).unwrap();
    assert!(line.contains("current \"First\""));
    assert!(line.contains("next none"));
    assert!(line.contains("1 open, 1 blocked"));
    let _ = list;
}

#[test]
fn reviewer_reopen_is_limited_to_the_orchestrator_qa_and_user() {
    let team = team();
    let list = team.seed(&["Check"]);
    let list = team.set(&list, 0, TabTodoState::Done, None).unwrap();
    let id = list.items[0].id.clone();
    for refused in [&team.unrelated, &team.worker] {
        assert!(block_on(team.fixture.service.tab_todo_reopen(
            &team.worker,
            Some(refused),
            id.clone(),
            "not a reviewer".into(),
        ))
        .is_err());
    }
    assert!(block_on(team.fixture.service.tab_todo_reopen(
        &team.worker,
        Some(&team.orchestrator),
        id.clone(),
        " ".into(),
    ))
    .is_err());
    let reopened = block_on(team.fixture.service.tab_todo_reopen_for_review(
        &team.worker,
        Some(&team.orchestrator),
        id.clone(),
        "Bounce:\nthe focused selector still fails",
    ))
    .unwrap();
    assert_eq!(reopened.items[0].state, TabTodoState::Open);
    assert_eq!(
        reopened.items[0].reason.as_deref(),
        Some("Bounce: the focused selector still fails")
    );
    let line = block_on(
        team.fixture
            .service
            .tab_todo_recite(&team.worker, Some(&team.worker)),
    )
    .unwrap()
    .and_then(|recitation| step_line(&recitation))
    .unwrap();
    assert!(line.contains("current none; next \"Check\""));
}

#[test]
fn a_respawned_worker_inherits_the_predecessor_list_with_attempts_and_history() {
    let team = team();
    let list = team.seed(&["Port", "Verify"]);
    let list = team.set(&list, 0, TabTodoState::InProgress, None).unwrap();
    let list = team.set(&list, 0, TabTodoState::Done, None).unwrap();
    let replacement = {
        let mut mux = team.fixture.service.mux.write();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let replacement = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role_kind(
            &replacement,
            AgentRole::Worker,
            None,
            Some(&team.orchestrator),
        )
        .unwrap();
        replacement
    };
    let inherited = block_on(team.fixture.service.tab_todo_inherit_or_seed(
        &replacement,
        &team.orchestrator,
        &[team.worker.clone()],
        &["Fresh criterion".into()],
    ))
    .unwrap()
    .unwrap();
    assert_eq!(inherited.tab_id, replacement);
    assert_eq!(
        inherited
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        list.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(inherited.items[0].attempt, 1);
    assert_eq!(inherited.items[0].history.len(), 2);
    assert!(block_on(
        team.fixture
            .service
            .tab_todo_list(&team.worker, Some(&team.worker))
    )
    .unwrap()
    .is_none());
    assert!(block_on(team.fixture.service.tab_todo_inherit_or_seed(
        &team.unrelated,
        &team.orchestrator,
        &[replacement.clone()],
        &[],
    ))
    .is_err());
}

#[test]
fn a_list_beyond_twelve_items_is_refused_with_split_guidance() {
    let team = team();
    let criteria: Vec<String> = (0..=MAX_TAB_TODO_ITEMS)
        .map(|index| format!("Criterion {index}"))
        .collect();
    let error = block_on(team.fixture.service.tab_todo_seed_from_acceptance(
        &team.worker,
        &team.orchestrator,
        &criteria,
    ))
    .unwrap_err();
    assert!(error.contains("propose a task split"));
}

#[test]
fn review_warns_accept_closes_and_bounce_reopens_locked_criteria() {
    let team = team();
    let seeded = team.seed(&["Build", "Test"]);
    let warning = block_on(team.fixture.service.tab_todo_report_warning(&team.worker))
        .unwrap()
        .unwrap();
    assert!(warning.starts_with("WARNING:"));
    for item in &seeded.items {
        assert!(warning.contains(&item.id) && warning.contains(&item.title));
    }
    block_on(team.fixture.service.tab_todo_apply_review(
        &team.worker,
        Some(&team.orchestrator),
        true,
    ))
    .unwrap();
    let closed = block_on(
        team.fixture
            .service
            .tab_todo_list(&team.worker, Some(&team.worker)),
    )
    .unwrap()
    .unwrap();
    assert!(closed
        .items
        .iter()
        .all(|item| item.state == TabTodoState::Done
            && item.reason.as_deref() == Some("accepted by review")));
    assert!(
        block_on(team.fixture.service.tab_todo_report_warning(&team.worker))
            .unwrap()
            .is_none()
    );
    block_on(team.fixture.service.tab_todo_apply_review(
        &team.worker,
        Some(&team.orchestrator),
        false,
    ))
    .unwrap();
    let reopened = block_on(
        team.fixture
            .service
            .tab_todo_list(&team.worker, Some(&team.worker)),
    )
    .unwrap()
    .unwrap();
    assert!(reopened
        .items
        .iter()
        .all(|item| item.state == TabTodoState::Open));
}

#[test]
fn review_without_checklist_storage_does_not_interrupt_task_review() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    assert!(block_on(fixture.service.tab_todo_report_warning(&tab_id))
        .unwrap()
        .is_none());
    for accepted in [true, false] {
        block_on(
            fixture
                .service
                .tab_todo_apply_review(&tab_id, None, accepted),
        )
        .unwrap();
    }
}
