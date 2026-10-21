use super::*;
use crate::{store::Store, SentinelExt};
use codeterm_blueprint_types::blueprint_document::Document;
use codeterm_service_core::persistence_test_support::{close, ServiceInstance};

#[test]
fn worker_keep_is_refused_and_saved_recipe_applies_only_in_its_project() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let team = codeterm_types::team::Team::new(&workspace, 1);
    mux.upsert_team(team.clone());
    let worker = mux.get_tab_mut(&tab).unwrap();
    worker.team_id = Some(team.id);
    worker.role = Some(codeterm_types::agent::AgentRole::Worker);
    worker.cwd = Some(root.path().to_string_lossy().into_owned());
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
    assert!(service
        .blueprint_keep(&caller, "missing", "Watch CI failures")
        .unwrap_err()
        .contains("Orchestrator"));
    let document=Document::from_definition(&json!({"create":{"name":"Concrete watch","note":"Watch CI","probe":"echo private-instance","question":"Ready?","buckets":["done"],"every":"20s","matches":{"done":"ready"},"routes":{"done":"notify"},"timeout":"1h"}})).unwrap();
    let id = service
        .mem
        .save_blueprint(
            &area(&workspace),
            document,
            "Watch CI readiness",
            Evidence {
                kept: true,
                ..Evidence::default()
            },
        )
        .unwrap()
        .unwrap();
    assert!(service
        .blueprint_apply_recipe(&caller, &id, &[], None, false)
        .is_err());
    let result = service
        .blueprint_apply_recipe(
            &caller,
            &id,
            &["watch_probe=echo rebound".into()],
            None,
            false,
        )
        .unwrap();
    let job = service
        .sentinel_show(&caller, result["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(job.probe(), "echo rebound");
    assert_eq!(job.team_id, caller.team);
    assert_eq!(job.workspace_id, workspace);
    assert!(!service
        .mem
        .get(&id)
        .unwrap()
        .unwrap()
        .body
        .contains("private-instance"));
    close(instance);
}
