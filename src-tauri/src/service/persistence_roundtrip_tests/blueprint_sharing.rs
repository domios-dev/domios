use codeterm_blueprint_types::blueprint_recipe::{Recipe, SCHEMA_VERSION};
use serde_json::json;
use tauri::Manager;

use crate::sentinel::{amend::DefinitionSnapshot, store::Store};

#[test]
fn blueprint_import_persists_stopped_and_visible_in_scratch_team() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, original_tab) = crate::multiplexer::state::MultiplexerState::new();
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
    let instance = super::ServiceInstance::open_with_mux(root.path(), mux);
    let handle = crate::sentinel::scheduler::fixture_handle(
        Store::with_storage(&instance.storage, crate::cron::clock::system_clock()),
        instance.fixture._app.handle().clone(),
    );
    assert!(instance.fixture.service.app_handle.manage(handle));
    let service = &instance.fixture.service;
    let original_caller = service.sentinel_caller(&original_tab, None).unwrap();
    let scratch_caller = service.sentinel_caller(&scratch_tab, None).unwrap();
    let original = service
        .sentinel_create(
            &original_caller,
            serde_json::from_value(json!({
                "name":"Portable watcher", "probe":"printf ready", "question":"State?",
                "buckets":["done"], "matches":{"done":"ready"}, "timeout":"1h"
            }))
            .unwrap(),
        )
        .unwrap();
    let recipe = service
        .blueprint_export(&original_caller, &original.id)
        .unwrap();
    assert!(service
        .blueprint_import(&scratch_caller, recipe.clone(), &[], true)
        .unwrap_err()
        .contains("missing recipe placeholders"));
    let assignments = recipe
        .placeholders
        .iter()
        .map(|(key, description)| {
            let value = if key == "timeout" {
                "1h"
            } else if description.ends_with(".probe") {
                original.probe().as_str()
            } else if description.ends_with(".question") {
                original.question.as_str()
            } else if description.ends_with(".matches.done") {
                original.matches["done"].as_str()
            } else if description.ends_with(".handlers.done.0.id") {
                original.routes["done"][0].id.as_str()
            } else {
                panic!("unexpected placeholder {description}")
            };
            format!("{key}={value}")
        })
        .collect::<Vec<_>>();
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
        placeholders: Default::default(),
        definition: json!({"create":{"name":"Invalid watcher", "probe":"printf ready", "question":"State?", "buckets":[]}}),
    };
    assert!(service
        .blueprint_import(&scratch_caller, invalid, &[], true)
        .is_err());
    let pipeline = Recipe {
        schema_version: SCHEMA_VERSION,
        placeholders: Default::default(),
        definition: json!({"spec":{
            "name":"Portable pipeline", "owner":"team", "trigger":{"type":"schedule","every":"20s"},
            "steps":[{"name":"run","input":"text","kind":{"kind":"command","command":"printf ready"}}], "result":{"to":"owner"}
        }}),
    };
    let pipeline = service
        .blueprint_import(&scratch_caller, pipeline, &[], true)
        .unwrap();
    let pipeline_id = pipeline["id"].as_str().unwrap().to_string();
    assert!(pipeline["blueprint"]["machine"]["stopped"]
        .as_bool()
        .unwrap());
    let restarted = instance.restart(root.path());
    let store = Store::with_storage(&restarted.storage, crate::cron::clock::system_clock());
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
