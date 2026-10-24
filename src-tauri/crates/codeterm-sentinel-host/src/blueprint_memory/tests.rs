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
        .blueprint_keep(
            &caller,
            "missing",
            &["Watch CI failures".into()],
            &["ci".into()]
        )
        .unwrap_err()
        .contains("Orchestrator"));
    let document=Document::from_definition(&json!({"schema_version":2,"kind":"watch","name":"Concrete watch","note":"Watch CI","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"echo private-instance"}},"every":{"lit":"20s"},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["done"],"matches":{"done":{"lit":"ready"}},"until":"done","routes":{"done":"notice"}},{"id":"notice","uses":"notify"}]})).unwrap();
    let id = service
        .mem
        .save_blueprint(
            &area(&workspace),
            document,
            &["Watch CI readiness".into()],
            Evidence {
                kept: true,
                ..Evidence::default()
            },
        )
        .unwrap()
        .id;
    assert!(service
        .blueprint_apply_recipe(&caller, &id, &[], None, false)
        .is_err());
    let result = service
        .blueprint_apply_recipe(&caller, &id, &["command=echo rebound".into()], None, false)
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
    assert!(service
        .blueprint_save_solution_host(
            &workspace,
            caller.team.as_deref(),
            &job.id,
            "Watch readiness",
            &["ci".into(), "monitoring".into()],
        )
        .unwrap_err()
        .contains("successful"));
    let transition = crate::handlers::Transition {
        causal_run: None,
        decision: None,
        id: "proof".into(),
        bucket: "done".into(),
        accepted_at: 1,
        terminal: true,
        handlers: vec![],
    };
    assert!(crate::storage::commit(&handle.store, |store| store
        .stage_finish_with(&job, Some(transition)))
    .unwrap());
    let saved = service
        .blueprint_save_solution_host(
            &workspace,
            caller.team.as_deref(),
            &job.id,
            "Watch readiness",
            &["ci".into(), "monitoring".into()],
        )
        .unwrap();
    assert_eq!(saved["status"], "saved");
    assert_eq!(saved["tags"], json!(["ci", "monitoring"]));
    let saved_id = saved["id"].as_str().unwrap();
    assert!(service
        .mem
        .blueprint_recipe(&area(&workspace), saved_id)
        .is_ok());
    let listed = service.blueprint_solutions_host(&workspace).unwrap();
    assert!(listed
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"] == saved_id && entry["tags"] == json!(["ci", "monitoring"])));
    assert_eq!(
        service.mem.blueprint_tags(saved_id).unwrap(),
        vec!["ci", "monitoring"]
    );
    service
        .blueprint_archive_solution_host(&workspace, saved_id)
        .unwrap();
    assert!(service
        .mem
        .blueprint_recipe(&area(&workspace), saved_id)
        .is_err());
    close(instance);
}
