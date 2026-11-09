use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TaskBinding;
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn personality_update_retitles_only_matching_live_task_tabs() {
    let (mut mux, worker) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let foreign = mux.create_tab(&workspace).unwrap();
    mux.rename_tab(&worker, "a").unwrap();
    mux.rename_tab(&foreign, "Foreign").unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Names".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::default(),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Persisted task".into(),
                personality: Some("a".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    {
        let mut mux = fixture.service.mux.write();
        for (id, plan_id) in [(&worker, plan.id.as_str()), (&foreign, "other-plan")] {
            mux.get_tab_mut(id).unwrap().task_binding = Some(TaskBinding {
                plan_id: Some(plan_id.into()),
                task_id: Some(task.clone()),
                ..Default::default()
            });
        }
    }
    let _storage = fixture.install_seeded_storage_desk();
    fixture
        .service
        .update_task_personality(&plan.id, &task, Some(Some(" Ledger ")), None, 3)
        .unwrap();
    assert_eq!(
        fixture.service.mux.read().get_tab(&worker).unwrap().title,
        "Ledger"
    );
    assert_eq!(
        fixture.service.mux.read().get_tab(&foreign).unwrap().title,
        "Foreign"
    );
    let saved = fixture.service.plan_store.get(&plan.id).unwrap();
    assert_eq!(saved.tasks[0].personality.as_deref(), Some("Ledger"));
    assert!(fixture.service.rename_tab(&worker, "Other").is_err());
    assert!(fixture
        .service
        .update_task_personality(&plan.id, &task, Some(Some("two words")), None, 4)
        .is_err());
    assert_eq!(
        fixture.service.mux.read().get_tab(&worker).unwrap().title,
        "Ledger"
    );
    let saved = fixture.service.plan_store.get(&plan.id).unwrap();
    assert_eq!(saved.tasks[0].personality.as_deref(), Some("Ledger"));
    crate::service::session_persist::flush_session_persistence(&fixture.service).unwrap();
    crate::service::session_persist::stop_worker(&fixture.service).unwrap();
}
