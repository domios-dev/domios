use super::*;
use codeterm_types::agent::{TabTodoProposalState, TabTodoWriteEntry};

fn entry(id: Option<&str>, content: &str, state: TabTodoState) -> TabTodoWriteEntry {
    TabTodoWriteEntry {
        id: id.map(str::to_owned),
        content: content.into(),
        state,
        link: None,
        reason: None,
    }
}

#[test]
fn proposal_sharing_does_not_gate_live_work_and_only_notifies_once() {
    let team = team();
    let team_id = "checklist-team";
    {
        let mut mux = team.fixture.service.mux.write();
        for tab in [&team.worker, &team.orchestrator] {
            mux.get_tab_mut(tab).unwrap().team_id = Some(team_id.into());
        }
    }
    let plan = team
        .fixture
        .service
        .plan_store
        .create_plan_for_team(
            "Audit".into(),
            team.fixture.temp.path().to_string_lossy().into_owned(),
            crate::plan::OrchestratorRef::from_tab(&team.orchestrator),
            team_id.into(),
            vec![],
            1,
        )
        .unwrap();
    let seeded = team.seed(&["Ship"]);
    let working = team
        .set(&seeded, 0, TabTodoState::InProgress, None)
        .unwrap();
    let shared = block_on(
        team.fixture
            .service
            .tab_todo_submit(&team.worker, &team.worker),
    )
    .unwrap();
    assert_eq!(shared.items, working.items);
    assert_eq!(
        shared.proposal.as_ref().unwrap().state,
        TabTodoProposalState::Proposed
    );
    let again = block_on(
        team.fixture
            .service
            .tab_todo_submit(&team.worker, &team.worker),
    )
    .unwrap();
    assert_eq!(again.revision, shared.revision);
    assert_eq!(
        team.fixture
            .ui
            .0
            .lock()
            .iter()
            .filter(|(name, _)| name == "tab-todo-proposed")
            .count(),
        1
    );
    let completed = team.set(&shared, 0, TabTodoState::Done, None).unwrap();
    assert_eq!(completed.proposal, shared.proposal);
    let reviewed = block_on(team.fixture.service.tab_todo_review_proposal(
        &team.worker,
        Some(&team.orchestrator),
        None,
        None,
        None,
    ))
    .unwrap();
    assert_eq!(
        reviewed.proposal.unwrap().state,
        TabTodoProposalState::Approved
    );
    assert_eq!(reviewed.items[0].state, TabTodoState::Done);
    let stored = team.fixture.service.plan_store.get(&plan.id).unwrap();
    let events = codeterm_plan::service_plan::state::mission_control::build_plan_event_log(&stored);
    let kinds: Vec<_> = events
        .entries
        .iter()
        .filter(|event| event.kind.starts_with("tab_todo_"))
        .map(|event| event.kind.as_str())
        .collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&"tab_todo_shared"));
    assert!(kinds.contains(&"tab_todo_approved"));
}

#[test]
fn proposal_permissions_refuse_self_siblings_and_unrelated_reviewers() {
    let team = team();
    team.seed(&["Ship"]);
    for actor in [&team.worker, &team.qa, &team.unrelated] {
        let error = block_on(team.fixture.service.tab_todo_review_proposal(
            &team.worker,
            Some(actor),
            None,
            None,
            None,
        ))
        .unwrap_err();
        assert!(error.contains("only the Worker's parent or Plan owner"));
    }
    assert!(block_on(
        team.fixture
            .service
            .tab_todo_submit(&team.worker, &team.orchestrator)
    )
    .is_err());
    let approved = block_on(team.fixture.service.tab_todo_review_proposal(
        &team.worker,
        Some(&team.orchestrator),
        None,
        None,
        None,
    ))
    .unwrap();
    assert_eq!(
        approved.proposal.unwrap().state,
        TabTodoProposalState::Approved
    );
}

#[test]
fn proposal_amendment_preserves_criteria_and_attempts_and_refuses_stale_cards() {
    let team = team();
    let seeded = team.seed(&["Ship"]);
    let written = block_on(team.fixture.service.tab_todo_write(
        &team.worker,
        &team.worker,
        vec![
            entry(None, "Inspect", TabTodoState::InProgress),
            entry(None, "Redundant", TabTodoState::Open),
        ],
    ))
    .unwrap()
    .checklist;
    let own = written
        .items
        .iter()
        .find(|item| item.title == "Inspect")
        .unwrap();
    let amended = block_on(team.fixture.service.tab_todo_review_proposal(
        &team.worker,
        Some(&team.orchestrator),
        Some(vec![
            entry(Some(&own.id), "Inspect errors", TabTodoState::InProgress),
            entry(None, "Verify", TabTodoState::Open),
        ]),
        Some("Cover error handling".into()),
        Some((written.generation, written.revision)),
    ))
    .unwrap();
    assert_eq!(amended.items[0], seeded.items[0]);
    let kept = amended.items.iter().find(|item| item.id == own.id).unwrap();
    assert_eq!(kept.title, "Inspect errors");
    assert_eq!(kept.history, own.history);
    assert_eq!(kept.attempt, own.attempt);
    assert!(!amended.items.iter().any(|item| item.title == "Redundant"));
    assert_eq!(
        amended.proposal.as_ref().unwrap().state,
        TabTodoProposalState::Amended
    );
    assert!(block_on(team.fixture.service.tab_todo_review_proposal(
        &team.worker,
        Some(&team.orchestrator),
        None,
        None,
        Some((written.generation, written.revision))
    ))
    .is_err());
    let error = block_on(team.fixture.service.tab_todo_review_proposal(
        &team.worker,
        Some(&team.orchestrator),
        Some(vec![entry(
            Some(&seeded.items[0].id),
            "Changed criterion",
            TabTodoState::Open,
        )]),
        None,
        None,
    ))
    .unwrap_err();
    assert!(error.contains("seeded criteria stay locked"));
    let saved = block_on(
        team.fixture
            .service
            .tab_todo_list(&team.worker, Some(&team.worker)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(saved, amended);
}
