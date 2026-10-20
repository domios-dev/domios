use super::*;
use crate::{
    amend::DefinitionSnapshot,
    store::{Owner, Store},
};

fn owner(team: &str) -> Owner {
    Owner {
        creator_tab_id: "owner-private-id".into(),
        team_id: Some(team.into()),
        workspace_id: "workspace-private-id".into(),
        cwd: "/private/owner/directory".into(),
    }
}

fn bindings(recipe: &Recipe, definition: &Value) -> Vec<String> {
    recipe
        .placeholders
        .iter()
        .map(|(key, description)| {
            if key == "timeout" {
                return "timeout=1h".into();
            }
            let mut value = definition;
            for part in description
                .strip_prefix("Value for definition.")
                .unwrap()
                .split('.')
            {
                value = if value.is_array() {
                    &value[part.parse::<usize>().unwrap()]
                } else {
                    &value[part]
                };
            }
            format!("{key}={}", value.as_str().unwrap())
        })
        .collect()
}

#[test]
fn blueprint_export_redacts_local_references_secrets_and_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("jobs.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let job = store.prepare(serde_json::from_value(json!({
        "name":"Portable watcher", "probe":"cat /private/probe/file --token secret-token-value", "question":"State?", "buckets":["done"], "matches":{"done":"done"}, "timeout":"1h",
        "handlers":{"done":[
            {"id":"private-handler-id", "type":"send", "parameters":{"target":"private-tab-id"}},
            {"id":"next", "type":"blueprint", "parameters":{"target":"sentinel_private_child"}},
            {"id":"cron", "type":"cron", "parameters":{"target":"private-cron-name"}}
        ], "other":[{"id":"agent", "type":"escalate", "parameters":{"target":"agent:private-agent-id"}}]}, "escalation_timeout_ms": 1000
    })).unwrap(), owner("private-team-id"), false).unwrap();
    let recipe = export(&job).unwrap();
    let raw = serde_json::to_string(&recipe).unwrap();
    for forbidden in [
        "private-tab-id",
        "sentinel_private_child",
        "private-cron-name",
        "private-agent-id",
        "private-handler-id",
        "owner-private-id",
        "private-team-id",
        "workspace-private-id",
        "/private",
        "secret-token-value",
        "machine",
        "\"transitions\":",
        "expires_at",
        "last_reason",
        "definition_version",
        "causal_run",
    ] {
        assert!(!raw.contains(forbidden), "leaked {forbidden}");
    }
    assert!(!recipe.placeholders.is_empty());
}

#[test]
fn blueprint_recipe_round_trip_into_another_team_preserves_definition() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("jobs.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let job = store.prepare(serde_json::from_value(json!({
        "name":"Portable watcher", "probe":"cat /scratch/status", "question":"State?", "buckets":["done"], "matches":{"done":"^done$"}, "timeout":"1h"
    })).unwrap(), owner("original-team"), false).unwrap();
    let recipe = Recipe::parse(&serde_json::to_string(&export(&job).unwrap()).unwrap()).unwrap();
    assert_eq!(
        recipe.definition.get("create").is_some(),
        job.pipeline().is_none()
    );
    let bound = recipe.bind(&bindings(&recipe, &definition(&job))).unwrap();
    let imported = store
        .prepare(
            serde_json::from_value(bound["create"].clone()).unwrap(),
            owner("scratch-team"),
            false,
        )
        .unwrap();
    assert_eq!(
        DefinitionSnapshot::from_job(&job),
        DefinitionSnapshot::from_job(&imported)
    );
    assert_ne!(job.team_id, imported.team_id);
    assert_ne!(job.id, imported.id);
}

#[test]
fn blueprint_pipeline_export_round_trip_preserves_trigger_steps_and_result() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("jobs.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let spec = serde_json::from_value(json!({
        "name":"Portable pipeline", "owner":"team",
        "trigger":{"type":"input","kind":"text","source":{"text":"private-input-token"}},
        "steps":[{"name":"run","input":"text","kind":{"kind":"command","command":"cat /private/path --token private-token"}}],
        "result":{"to":"tab:private-destination"}
    })).unwrap();
    let job = store
        .prepare_pipeline(spec, owner("original-team"), false)
        .unwrap();
    let recipe = export(&job).unwrap();
    let raw = serde_json::to_string(&recipe).unwrap();
    for forbidden in [
        "private-input-token",
        "/private/path",
        "private-token",
        "private-destination",
    ] {
        assert!(!raw.contains(forbidden));
    }
    assert_eq!(
        recipe.definition.get("create").is_some(),
        job.pipeline().is_none()
    );
    let bound = recipe.bind(&bindings(&recipe, &definition(&job))).unwrap();
    assert_eq!(
        bound["spec"],
        serde_json::to_value(job.pipeline().cloned().unwrap()).unwrap()
    );
}

#[test]
fn blueprint_import_accepts_legacy_export_handler_keys() {
    let recipe = Recipe::parse(r#"{"schema_version":1,"placeholders":{},"definition":{"create":{"name":"Legacy export","probe":"status","question":"state?","buckets":["done"],"matches":{"done":"done"},"timeout":"1h","handlers":{"done":[{"id":"next","type":"sentinel","parameters":{"target":"sentinel_child"}}]}}}}"#).unwrap();
    let bound = crate::public_definition::stored_definition(recipe.bind(&[]).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("jobs.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let job = store
        .prepare(
            serde_json::from_value(bound["create"].clone()).unwrap(),
            owner("team"),
            false,
        )
        .unwrap();
    assert_eq!(
        job.routes["done"][0].action,
        Route::Sentinel("sentinel_child".into())
    );
    assert!(job.id.starts_with("blueprint_"));
    let public = definition(&job);
    assert_eq!(public["create"]["handlers"]["done"][0]["type"], "blueprint");
}
