use super::ServiceInstance;
use codeterm_blueprint_types::blueprint_recipe::{Recipe, SCHEMA_VERSION};
use serde_json::json;

use crate::{amend::DefinitionSnapshot, store::Store};
use crate::{AutomationGraphExt, BlueprintApplyExt, BlueprintPortableExt, SentinelExt};

#[test]
fn blueprint_import_persists_stopped_and_visible_in_scratch_team() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, original_tab) =
        codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&original_tab).unwrap();
    let scratch_tab = mux.create_tab(&workspace).unwrap();
    let original_team = codeterm_types::team::Team::new(&workspace, 1);
    let scratch_team = codeterm_types::team::Team::new(&workspace, 2);
    mux.upsert_team(original_team.clone());
    mux.upsert_team(scratch_team.clone());
    for (tab_id, team) in [
        (&original_tab, &original_team),
        (&scratch_tab, &scratch_team),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.team_id = Some(team.id.clone());
        tab.cwd = Some(root.path().to_string_lossy().into_owned());
        // A Team-owned pipeline delivers its result to the Team Orchestrator.
        tab.agent_type = Some("codex".into());
        tab.role = Some(codeterm_types::agent::AgentRole::Manager);
        tab.role_profile = Some("orchestrator".into());
    }
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(
            &instance.storage,
            codeterm_service_core::cron::clock::system_clock(),
        ),
        instance.fixture.service.ui.clone(),
    );
    crate::manage_fixture_ports(&instance.fixture.service, &handle);
    let service = &instance.fixture.service;
    let original_caller = service.sentinel_caller(&original_tab, None).unwrap();
    let scratch_caller = service.sentinel_caller(&scratch_tab, None).unwrap();
    let native = json!({"schema_version":2,"kind":"watch","name":"Portable watcher","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"printf ready"}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"State?"}},"buckets":["done"],"matches":{"done":{"lit":"ready"}}}]});
    let created = service
        .blueprint_apply(&original_caller, native, false)
        .unwrap();
    let original = service
        .sentinel_show(&original_caller, created["id"].as_str().unwrap())
        .unwrap();
    let recipe = service
        .blueprint_export(&original_caller, &original.id)
        .unwrap();
    assert!(service
        .blueprint_import(&scratch_caller, recipe.clone(), &[], true)
        .is_err());
    let assignments = vec!["command=printf ready".into()];
    let imported = service
        .blueprint_import(&scratch_caller, recipe, &assignments, true)
        .unwrap();
    let imported_id = imported["id"].as_str().unwrap().to_string();
    let imported = service
        .sentinel_show(&scratch_caller, &imported_id)
        .unwrap();
    assert!(imported.machine.stopped);
    assert_eq!(imported.team_id.as_deref(), Some(scratch_team.id.as_str()));
    assert_eq!(
        DefinitionSnapshot::from_job(&original),
        DefinitionSnapshot::from_job(&imported)
    );
    let snapshot = service
        .automation_graph_snapshot(&workspace, Some(&scratch_team.id))
        .unwrap();
    assert!(snapshot
        .scenarios
        .iter()
        .any(|scenario| scenario.root.id == imported_id));

    let invalid = Recipe {
        schema_version: SCHEMA_VERSION,
        definition: json!({"schema_version":2,"kind":"watch","name":"Invalid watcher","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"printf ready"}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"State?"}},"buckets":[]}]}),
    };
    assert!(service
        .blueprint_import(&scratch_caller, invalid, &[], true)
        .is_err());
    let pipeline = Recipe {
        schema_version: SCHEMA_VERSION,
        definition: json!({"schema_version":2,"kind":"pipeline","name":"Portable pipeline","owner":"team","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"run","run":{"lit":"printf ready"}}],"result":{"to":"owner"}}),
    };
    let pipeline = service
        .blueprint_import(&scratch_caller, pipeline, &[], true)
        .unwrap();
    let pipeline_id = pipeline["id"].as_str().unwrap().to_string();
    assert!(pipeline["blueprint"]["machine"]["stopped"]
        .as_bool()
        .unwrap());
    let restarted = instance.restart(root.path());
    let store = Store::with_storage(
        &restarted.storage,
        codeterm_service_core::cron::clock::system_clock(),
    );
    assert_eq!(store.list().len(), 3);
    assert!(store.get(&imported_id).unwrap().machine.stopped);
    assert!(store.get(&pipeline_id).unwrap().machine.stopped);
    assert_eq!(
        store.get(&imported_id).unwrap().team_id.as_deref(),
        Some(scratch_team.id.as_str())
    );
    drop(store);
    super::close(restarted);
}

#[test]
fn blueprint_github_create_show_json_retains_typed_source_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let team = codeterm_types::team::Team::new(&workspace, 1);
    mux.upsert_team(team.clone());
    mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id);
    mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(
            &instance.storage,
            codeterm_service_core::cron::clock::system_clock(),
        ),
        instance.fixture.service.ui.clone(),
    );
    crate::manage_fixture_ports(&instance.fixture.service, &handle);
    let service = &instance.fixture.service;
    let caller = service.sentinel_caller(&tab, None).unwrap();
    let probe = json!({"repo":"cli/cli", "resource":"run", "id":39652641013u64});
    let created=service.blueprint_apply_with_state(&caller,json!({"schema_version":2,"kind":"pipeline","name":"GitHub CI watch","trigger":{"type":"schedule","every":{"lit":"60s"}},"steps":[{"id":"github-check","uses":"github","with":{"repo":{"lit":"cli/cli"},"resource":{"lit":"run"},"id":{"lit":39652641013u64}}}],"result":{"to":"notify"}}),false,Some(true)).unwrap();
    let id = created["id"].as_str().unwrap();
    let shown = serde_json::to_value(service.sentinel_show(&caller, id).unwrap()).unwrap();
    assert_eq!(shown["pipeline"]["steps"][0]["kind"]["kind"], "github");
    assert_eq!(shown["pipeline"]["steps"][0]["kind"]["probe"], probe);
    let restarted = instance.restart(root.path());
    let store = Store::with_storage(
        &restarted.storage,
        codeterm_service_core::cron::clock::system_clock(),
    );
    let persisted = serde_json::to_value(store.get(id).unwrap()).unwrap();
    assert_eq!(persisted["pipeline"], shown["pipeline"]);
    drop(store);
    super::close(restarted);
}
