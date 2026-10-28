use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::json;

#[test]
fn workspace_import_is_runnable_without_a_team_and_refuses_foreign_scope() {
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    mux.get_tab_mut(&tab).unwrap().cwd = Some(std::env::temp_dir().to_string_lossy().into());
    let fixture = ServiceFixture::new(mux);
    let handle = crate::scheduler::fixture_handle_with_ui(
        crate::store::Store::new(
            fixture.temp.path().join("blueprints.json"),
            codeterm_cron::clock::MockClock::starting_at(1000),
        ),
        fixture.service.ui.clone(),
    );
    crate::manage_fixture_ports(&fixture.service, &handle);
    let caller = fixture.service.sentinel_caller(&tab, None).unwrap();
    fixture
        .service
        .blueprint_import_destination(&caller, Some(&format!("workspace:{workspace}")))
        .unwrap();
    for scope in ["workspace:foreign", "team:foreign", "invalid", "workspace:"] {
        assert!(fixture
            .service
            .blueprint_import_destination(&caller, Some(scope))
            .is_err());
    }
    let definition = json!({"schema_version":2,"kind":"pipeline","name":"Workspace import","owner":"tab",
        "trigger":{"type":"schedule","every":{"lit":"20s"}},
        "steps":[{"id":"emit","run":{"lit":"echo ready"}}],"result":{"to":"owner"}});
    let result = fixture
        .service
        .blueprint_import(
            &caller,
            Recipe {
                schema_version: 2,
                definition: definition.clone(),
            },
            &[],
            true,
        )
        .unwrap();
    let id = result["id"].as_str().unwrap();
    let job = handle.store.lock().get(id).unwrap().clone();
    assert_eq!(job.workspace_id, workspace);
    assert_eq!(job.team_id, None);
    assert_eq!(job.creator_tab_id, tab);
    assert!(crate::routes::owner_live(&fixture.service, &job));
    crate::blueprint_pipeline::validate_result_scope(
        &*fixture.service.mux.read(),
        &job,
        &codeterm_blueprint_types::blueprint_pipeline::ResultTarget::Owner,
    )
    .unwrap();
    let mut callable = definition;
    callable["name"] = json!("Unscoped call");
    callable["steps"] = json!([{"id":"nested","call":{"ref":"mem:untrusted"}}]);
    assert!(fixture
        .service
        .blueprint_import(
            &caller,
            Recipe {
                schema_version: 2,
                definition: callable
            },
            &[],
            true
        )
        .unwrap_err()
        .contains("Team-scoped authority"));
}

#[test]
fn general_destination_uses_the_real_general_workspace() {
    let (mut mux, _) = MultiplexerState::new();
    codeterm_service_core::service::general_agent::ensure_general_workspace(&mut mux, false);
    let tab = mux.resolve_tab_id("general").unwrap();
    let fixture = ServiceFixture::new(mux);
    let caller = fixture.service.sentinel_caller(&tab, None).unwrap();
    fixture
        .service
        .blueprint_import_destination(&caller, Some("general"))
        .unwrap();
    assert!(caller.team.is_none());
}
