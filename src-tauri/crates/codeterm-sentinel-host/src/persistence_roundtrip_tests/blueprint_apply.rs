use super::ServiceInstance;
use crate::{store::Store, BlueprintApplyExt, SentinelExt};
use serde_json::{json, Value};

fn definition(pipeline: bool) -> Value {
    if pipeline {
        json!({"spec":{"name":"Apply pipeline","note":"Reports a result","owner":"team","trigger":{"type":"schedule","every":"20s"},"steps":[{"name":"emit","input":"text","kind":{"kind":"command","command":"echo ready"},"on":{"done":"stop","failed":"stop"}}],"result":{"to":"notify"}}})
    } else {
        json!({"create":{"name":"Apply watch","note":"Reports readiness","probe":"echo ready","question":"Ready?","buckets":["done"],"every":"20s","matches":{"done":"ready"},"routes":{"done":"notify"},"timeout":"1h"}})
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
        let key = if pipeline { "spec" } else { "create" };
        spec[key]["note"] = json!("Updated explanation");
        if !pipeline {
            spec[key]["probe"] = json!("echo changed");
        }
        let dry = service
            .blueprint_apply(&caller, spec.clone(), true)
            .unwrap();
        assert_eq!(dry["action"], "update");
        assert!(!dry["before"].is_null());
        assert_eq!(dry["after"], spec);
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
                changed["create"][field] = value;
                let error = service
                    .blueprint_apply(&caller, changed, false)
                    .unwrap_err();
                assert!(
                    error.contains(&format!("blueprint_creation_only:watch.{field}:")),
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
            schema_version: 1,
            placeholders: Default::default(),
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
        .contains("Team"));
    super::close(instance);
}
