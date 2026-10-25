use super::ServiceInstance;
use crate::{store::Store, BlueprintApplyExt, SentinelExt};
use serde_json::{json, Value};

fn definition(pipeline: bool) -> Value {
    if pipeline {
        json!({"schema_version":2,"kind":"pipeline","name":"Apply pipeline","note":"Reports a result","owner":"team","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"echo ready"},"routes":{"done":"@stop","failed":"@stop"}}],"result":{"to":"notify"}})
    } else {
        json!({"schema_version":2,"kind":"watch","name":"Apply watch","note":"Reports readiness","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"echo ready"}},"every":{"lit":"20s"},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["done"],"matches":{"done":{"lit":"ready"}},"routes":{"done":"notify"}}],"other":[{"id":"notify","uses":"notify"}]})
    }
}

#[test]
fn blueprint_apply_is_idempotent_scoped_and_persisted_for_watch_and_pipeline() {
    for pipeline in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&tab).unwrap();
        let team = codeterm_types::team::Team::new(&workspace, 1);
        mux.upsert_team(team.clone());
        mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id.clone());
        mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
        let instance = ServiceInstance::open_with_mux(root.path(), mux);
        let service = &instance.fixture.service;
        let handle = crate::scheduler::fixture_handle_with_ui(
            Store::with_storage(
                &instance.storage,
                codeterm_service_core::cron::clock::system_clock(),
            ),
            service.ui.clone(),
        );
        crate::manage_fixture_ports(service, &handle);
        let caller = service.sentinel_caller(&tab, None).unwrap();
        let mut spec = definition(pipeline);
        assert_eq!(
            service
                .blueprint_apply(&caller, spec.clone(), true)
                .unwrap()["action"],
            "create"
        );
        assert!(handle.store.lock().jobs.is_empty());
        let created = service
            .blueprint_apply(&caller, spec.clone(), false)
            .unwrap();
        let id = created["id"].as_str().unwrap();
        assert_eq!(created["action"], "create");
        assert_eq!(
            service
                .blueprint_apply(&caller, spec.clone(), false)
                .unwrap()["action"],
            "unchanged"
        );
        assert_eq!(
            service
                .sentinel_show(&caller, id)
                .unwrap()
                .definition_version,
            1
        );
        spec["note"] = json!("Updated explanation");
        if !pipeline {
            spec["steps"][0]["with"]["command"]["lit"] = json!("echo changed");
        }
        let dry = service
            .blueprint_apply(&caller, spec.clone(), true)
            .unwrap();
        assert_eq!(dry["action"], "update");
        assert!(!dry["before"].is_null());
        let expected =
            codeterm_blueprint_types::blueprint_document::Document::from_definition(&spec).unwrap();
        let actual =
            codeterm_blueprint_types::blueprint_document::Document::from_definition(&dry["after"])
                .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            service
                .sentinel_show(&caller, id)
                .unwrap()
                .definition_version,
            1
        );
        let updated = service
            .blueprint_apply(&caller, spec.clone(), false)
            .unwrap();
        assert_eq!(updated["id"], id);
        assert_eq!(
            service
                .sentinel_show(&caller, id)
                .unwrap()
                .definition_version,
            2
        );
        assert_eq!(handle.store.lock().jobs.len(), 1);
        if !pipeline {
            for (field, value) in [("timeout", json!("4h")), ("paused", json!(true))] {
                let mut changed = spec.clone();
                changed["steps"][0][field] = if field == "timeout" {
                    json!({"lit":value})
                } else {
                    value
                };
                let error = service
                    .blueprint_apply(&caller, changed, false)
                    .unwrap_err();
                assert!(
                    matches!(&error, codeterm_blueprint_types::blueprint_contract::ApplyError::CreationOnly { field: actual, .. } if actual == &format!("steps[0].{field}")),
                    "{error}"
                );
            }
        }
        service.sentinel_stop(&caller, id).unwrap();
        let reactivated = service
            .blueprint_apply(&caller, spec.clone(), false)
            .unwrap();
        assert_eq!(reactivated["action"], "reactivate");
        assert_eq!(reactivated["state"], "running");
        assert_eq!(reactivated["id"], id);
        let recipe = codeterm_blueprint_types::blueprint_recipe::Recipe {
            schema_version: 2,
            definition: spec.clone(),
        };
        use crate::BlueprintPortableExt;
        for _ in 0..2 {
            let imported = service
                .blueprint_import(&caller, recipe.clone(), &[], true)
                .unwrap();
            assert_eq!(imported["id"], id);
        }
        assert_eq!(handle.store.lock().jobs.len(), 1);
        service
            .blueprint_apply(&caller, spec.clone(), false)
            .unwrap();
        let restarted = instance.restart(root.path());
        let service = &restarted.fixture.service;
        let handle = crate::scheduler::fixture_handle_with_ui(
            Store::with_storage(
                &restarted.storage,
                codeterm_service_core::cron::clock::system_clock(),
            ),
            service.ui.clone(),
        );
        crate::manage_fixture_ports(service, &handle);
        let caller = service.sentinel_caller(&tab, None).unwrap();
        assert_eq!(
            service.blueprint_apply(&caller, spec, false).unwrap()["action"],
            "unchanged"
        );
        assert_eq!(handle.store.lock().jobs.len(), 1);
        super::close(restarted);
    }
}

#[test]
fn blueprint_apply_without_team_refuses_before_writing() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let caller = instance
        .fixture
        .service
        .sentinel_caller(&tab, None)
        .unwrap();
    assert!(instance
        .fixture
        .service
        .blueprint_apply(&caller, definition(true), false)
        .unwrap_err()
        .to_string()
        .contains("Team"));
    super::close(instance);
}

#[test]
fn blueprint_apply_invalid_command_bind_is_typed_validation() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    mux.resolve_or_mint_team_for_parent(&tab).unwrap();
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let service = &instance.fixture.service;
    let caller = service.sentinel_caller(&tab, None).unwrap();
    let mut spec = definition(true);
    spec["steps"][0]["bind"] = json!({"run":"unknown.out"});
    let error = service.blueprint_apply(&caller, spec, true).unwrap_err();
    let codeterm_blueprint_types::blueprint_contract::ApplyError::Validation { checks } = error
    else {
        panic!("expected typed validation, got {error}");
    };
    assert!(checks
        .iter()
        .any(|check| check.field == "steps[0].bind.run" && !check.passed));
    super::close(instance);
}

#[test]
fn blueprint_concurrent_apply_commits_one_identity() {
    for pipeline in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&tab).unwrap();
        let team = codeterm_types::team::Team::new(&workspace, 1);
        mux.upsert_team(team.clone());
        mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id.clone());
        mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
        let instance = ServiceInstance::open_with_mux(root.path(), mux);
        let service = &instance.fixture.service;
        let handle = crate::scheduler::fixture_handle_with_ui(
            Store::with_storage(
                &instance.storage,
                codeterm_service_core::cron::clock::system_clock(),
            ),
            service.ui.clone(),
        );
        crate::manage_fixture_ports(service, &handle);
        let caller = service.sentinel_caller(&tab, None).unwrap();
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let left = scope.spawn(|| {
                barrier.wait();
                service
                    .blueprint_apply(&caller, definition(pipeline), false)
                    .unwrap()
            });
            let right = scope.spawn(|| {
                barrier.wait();
                service
                    .blueprint_apply(&caller, definition(pipeline), false)
                    .unwrap()
            });
            [left.join().unwrap(), right.join().unwrap()]
        });
        assert_eq!(results[0]["id"], results[1]["id"]);
        assert_eq!(
            results
                .iter()
                .filter(|result| result["action"] == "create")
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result["action"] == "unchanged")
                .count(),
            1
        );
        let inventory = handle.store.lock().jobs.clone();
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].definition_version, 1);
        super::close(instance);
    }
}
