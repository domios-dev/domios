use super::*;
use crate::async_runtime::block_on;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::agent::TabTodoWriteEntry;

struct Pair {
    fixture: ServiceFixture,
    orchestrator: String,
    worker: String,
}

fn pair() -> Pair {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    mux.set_tab_role_kind(&worker, AgentRole::Worker, None, Some(&orchestrator))
        .unwrap();
    let fixture = ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();
    Pair {
        fixture,
        orchestrator,
        worker,
    }
}

fn entry(content: &str, state: TabTodoState) -> TabTodoWriteEntry {
    TabTodoWriteEntry {
        content: content.into(),
        state,
        link: None,
        reason: None,
    }
}

impl Pair {
    fn write(
        &self,
        tab: &str,
        entries: &[TabTodoWriteEntry],
    ) -> Result<TabTodoWriteOutcome, String> {
        block_on(
            self.fixture
                .service
                .tab_todo_write(tab, tab, entries.to_vec()),
        )
    }

    fn has_checklist(&self, tab: &str) -> bool {
        self.fixture
            .service
            .mux
            .read()
            .get_tab(tab)
            .unwrap()
            .has_checklist
    }
}

fn titles_and_states(checklist: &TabTodoChecklist) -> Vec<(&str, TabTodoState, bool)> {
    checklist
        .items
        .iter()
        .map(|item| (item.title.as_str(), item.state, item.locked))
        .collect()
}

#[test]
fn an_orchestrator_creates_its_own_checklist_on_first_write_and_reconciles_it() {
    let pair = pair();
    let tab = pair.orchestrator.as_str();
    assert!(!pair.has_checklist(tab));

    let created = pair
        .write(
            tab,
            &[
                entry("Survey lanes", TabTodoState::InProgress),
                entry("Merge wave", TabTodoState::Open),
            ],
        )
        .unwrap()
        .checklist;
    assert_eq!(
        titles_and_states(&created),
        [
            ("Survey lanes", TabTodoState::InProgress, false),
            ("Merge wave", TabTodoState::Open, false),
        ]
    );
    assert!(
        pair.has_checklist(tab),
        "the tab row learns it holds a checklist"
    );

    let reconciled = pair
        .write(
            tab,
            &[
                entry("Survey lanes", TabTodoState::Done),
                entry("Release", TabTodoState::InProgress),
            ],
        )
        .unwrap()
        .checklist;
    assert_eq!(
        titles_and_states(&reconciled),
        [
            ("Survey lanes", TabTodoState::Done, false),
            ("Release", TabTodoState::InProgress, false),
        ]
    );
    assert_eq!(reconciled.generation, created.generation);
    assert_eq!(
        reconciled.items[0].id, created.items[0].id,
        "a matching title keeps its item"
    );
    assert_eq!(reconciled.items[0].attempt, 1);

    let cleared = pair.write(tab, &[]).unwrap().checklist;
    assert!(cleared.items.is_empty());
    assert!(!pair.has_checklist(tab));

    let again = pair
        .write(tab, &[entry("Second round", TabTodoState::Open)])
        .unwrap()
        .checklist;
    assert_eq!(
        titles_and_states(&again),
        [("Second round", TabTodoState::Open, false)]
    );
    assert!(again.generation > cleared.generation);
}

#[test]
fn seeded_locked_items_survive_agent_writes() {
    let pair = pair();
    let seeded = block_on(pair.fixture.service.tab_todo_seed_from_acceptance(
        &pair.worker,
        &pair.orchestrator,
        &["Criterion one".into(), "Criterion two".into()],
    ))
    .unwrap();

    let first = pair
        .write(
            &pair.worker,
            &[
                entry("Criterion one", TabTodoState::InProgress),
                entry("Own step", TabTodoState::Open),
            ],
        )
        .unwrap();
    assert_eq!(first.omitted_locked_items, 1);
    assert_eq!(
        titles_and_states(&first.checklist),
        [
            ("Criterion one", TabTodoState::InProgress, true),
            ("Criterion two", TabTodoState::Open, true),
            ("Own step", TabTodoState::Open, false),
        ]
    );
    assert_eq!(first.checklist.items[0].id, seeded.items[0].id);

    let second = pair
        .write(&pair.worker, &[entry("Criterion one", TabTodoState::Done)])
        .unwrap();
    assert_eq!(
        titles_and_states(&second.checklist),
        [
            ("Criterion one", TabTodoState::Done, true),
            ("Criterion two", TabTodoState::Open, true),
        ],
        "the omitted unlocked step goes, the omitted locked criterion stays"
    );

    let emptied = pair.write(&pair.worker, &[]).unwrap();
    assert_eq!(
        emptied.checklist.items.len(),
        2,
        "locked criteria are never removed"
    );
}

#[test]
fn a_second_in_progress_item_is_refused_by_name_and_foreign_writes_are_refused() {
    let pair = pair();
    pair.write(&pair.worker, &[entry("First", TabTodoState::InProgress)])
        .unwrap();
    let error = pair
        .write(
            &pair.worker,
            &[
                entry("First", TabTodoState::InProgress),
                entry("Second", TabTodoState::InProgress),
            ],
        )
        .unwrap_err();
    assert!(
        error.contains("First") && error.contains("Second"),
        "{error}"
    );

    let foreign = block_on(pair.fixture.service.tab_todo_write(
        &pair.worker,
        &pair.orchestrator,
        vec![entry("Hijack", TabTodoState::Open)],
    ));
    assert!(foreign.is_err());
}
