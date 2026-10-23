use super::{close, ServiceInstance};
use crate::{BlueprintApplyExt, SentinelExt};
use codeterm_cron::clock::MockClock;
use serde_json::json;

#[test]
fn stopped_blueprint_cleanup_survives_restart_and_update_cancels_it() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let team = codeterm_types::team::Team::new(&workspace, 1);
    mux.upsert_team(team.clone());
    mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id);
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let clock = MockClock::starting_at(1_000);
    let handle = crate::scheduler::fixture_handle_with_ui(
        crate::store::Store::with_storage(&instance.storage, clock.clone()),
        instance.fixture.service.ui.clone(),
    );
    crate::manage_fixture_ports(&instance.fixture.service, &handle);
    let caller = instance
        .fixture
        .service
        .sentinel_caller(&tab, None)
        .unwrap();
    let definition = json!({"schema_version":2,"kind":"watch","name":"cleanup watcher","note":"Watches releases and reports failures","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"status"}},"every":{"lit":"1m"},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Finished?"}},"buckets":["done"],"matches":{"done":{"lit":"done"}},"routes":{"done":"notice"}},{"id":"notice","uses":"notify"}]});
    let mut ids = Vec::new();
    for index in 0..2 {
        let mut definition = definition.clone();
        definition["name"] = json!(format!("cleanup watcher {index}"));
        let result = instance
            .fixture
            .service
            .blueprint_apply(&caller, definition.clone(), false)
            .unwrap();
        let created = instance
            .fixture
            .service
            .sentinel_show(&caller, result["id"].as_str().unwrap())
            .unwrap();
        let stopped = instance
            .fixture
            .service
            .sentinel_stop(&caller, &created.id)
            .unwrap();
        assert_eq!(stopped.cleanup.stopped_at, Some(1_000));
        assert_eq!(stopped.cleanup.deletes_at, Some(1_801_000));
        let shown = instance
            .fixture
            .service
            .sentinel_show(&caller, &created.id)
            .unwrap();
        let shown = serde_json::to_value(shown).unwrap();
        assert_eq!(shown["stopped_at"], 1_000);
        assert_eq!(shown["deletes_at"], 1_801_000);
        ids.push(created.id);
    }
    let restarted = instance.restart(root.path());
    let handle = crate::scheduler::fixture_handle_with_ui(
        crate::store::Store::with_storage(&restarted.storage, clock.clone()),
        restarted.fixture.service.ui.clone(),
    );
    crate::manage_fixture_ports(&restarted.fixture.service, &handle);
    let caller = restarted
        .fixture
        .service
        .sentinel_caller(&tab, None)
        .unwrap();
    assert_eq!(
        handle.store.lock().get(&ids[0]).unwrap().note.as_deref(),
        Some("Watches releases and reports failures")
    );
    let mut update = definition;
    update["name"] = json!("cleanup watcher 1");
    update["note"] = json!("Retargeted release watcher");
    restarted
        .fixture
        .service
        .blueprint_apply_document(
            &caller,
            update.clone(),
            false,
            None,
            Some((ids[1].clone(), 1, None)),
        )
        .unwrap();
    let updated = restarted
        .fixture
        .service
        .sentinel_show(&caller, &ids[1])
        .unwrap();
    assert!(!updated.machine.stopped);
    assert_eq!(updated.cleanup.deletes_at, None);
    assert_eq!(updated.definition_version, 2);
    assert_eq!(updated.note.as_deref(), Some("Retargeted release watcher"));
    assert_eq!(
        restarted
            .fixture
            .service
            .sentinel_history(&caller, &ids[1])
            .unwrap()
            .versions
            .len(),
        2
    );
    assert!(restarted
        .fixture
        .service
        .blueprint_apply_document(
            &caller,
            update,
            false,
            None,
            Some((ids[1].clone(), 1, None))
        )
        .is_err());
    clock.set_ms(1_800_999);
    crate::cleanup::remove_terminal(&handle.store).unwrap();
    assert!(handle.store.lock().get(&ids[0]).is_some());
    clock.set_ms(1_801_000);
    crate::cleanup::remove_terminal(&handle.store).unwrap();
    assert!(handle.store.lock().get(&ids[0]).is_none());
    assert!(handle.store.lock().get(&ids[1]).is_some());
    assert!(restarted
        .fixture
        .service
        .sentinel_show(&caller, &ids[0])
        .is_err());
    let listed = restarted.fixture.service.sentinel_list(&caller).unwrap();
    assert_eq!(listed.len(), 1);
    let listed = serde_json::to_value(&listed[0]).unwrap();
    assert_eq!(listed["note"], "Retargeted release watcher");
    assert!(listed["stopped_at"].is_null());
    assert!(listed["deletes_at"].is_null());
    let recipe =
        crate::portable::export(&handle.store.lock().get(&ids[1]).unwrap().clone()).unwrap();
    assert_eq!(recipe.definition["note"], "Retargeted release watcher");
    let persisted = restarted.restart(root.path());
    let store = crate::store::Store::with_storage(&persisted.storage, clock);
    assert!(store.get(&ids[0]).is_none());
    assert_eq!(
        store.get(&ids[1]).unwrap().note.as_deref(),
        Some("Retargeted release watcher")
    );
    close(persisted);
}

#[test]
fn stopped_pipeline_update_retargets_input_and_keeps_its_note() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let team = codeterm_types::team::Team::new(&workspace, 1);
    mux.upsert_team(team.clone());
    mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id);
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let clock = MockClock::starting_at(1_000);
    let handle = crate::scheduler::fixture_handle_with_ui(
        crate::store::Store::with_storage(&instance.storage, clock.clone()),
        instance.fixture.service.ui.clone(),
    );
    crate::manage_fixture_ports(&instance.fixture.service, &handle);
    let caller = instance
        .fixture
        .service
        .sentinel_caller(&tab, None)
        .unwrap();
    let spec = json!({"schema_version":2,"kind":"pipeline","name":"release pipeline","note":"Watches the current release","trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":"first-run"}}},"steps":[{"id":"inspect","uses":"filter","with":{"pattern":{"lit":"run"}}}],"result":{"to":"notify"}});
    let created = instance
        .fixture
        .service
        .blueprint_apply_with_state(&caller, spec.clone(), false, Some(true))
        .unwrap();
    let id = created["id"].as_str().unwrap();
    assert_eq!(
        instance
            .fixture
            .service
            .sentinel_show(&caller, id)
            .unwrap()
            .cleanup
            .stopped_at,
        Some(1_000)
    );
    let mut next = spec;
    next["note"] = json!("Watches the next release");
    next["trigger"]["source"] = json!({"lit":{"text":"next-run"}});
    clock.set_ms(2_000);
    instance
        .fixture
        .service
        .blueprint_apply_document(
            &caller,
            next.clone(),
            false,
            None,
            Some((id.into(), 1, None)),
        )
        .unwrap();
    let updated = instance.fixture.service.sentinel_show(&caller, id).unwrap();
    assert!(!updated.machine.stopped);
    assert_eq!(updated.cleanup.deletes_at, None);
    assert_eq!(updated.next_run, 2_000);
    assert_eq!(
        serde_json::to_value(updated.pipeline()).unwrap(),
        codeterm_blueprint_types::blueprint_document::Document::from_definition(&next)
            .unwrap()
            .lower()
            .unwrap()["spec"]
    );
    assert_eq!(updated.note.as_deref(), Some("Watches the next release"));
    let recipe = crate::portable::export(&updated).unwrap();
    assert_eq!(recipe.definition["note"], "Watches the next release");
    close(instance);
}
