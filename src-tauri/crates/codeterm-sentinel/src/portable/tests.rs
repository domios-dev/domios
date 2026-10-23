use super::*;
use crate::store::{Owner, Store};
use codeterm_blueprint_types::blueprint_document::{Document, ValueForm};
use serde_json::json;
fn owner(team: &str) -> Owner {
    Owner {
        creator_tab_id: "private-owner".into(),
        team_id: Some(team.into()),
        workspace_id: "private-workspace".into(),
        cwd: "/private/directory".into(),
    }
}
fn native(watch: bool) -> Document {
    Document::from_definition(&if watch {json!({"schema_version":2,"kind":"watch","name":"Portable watcher","note":"Watch a release","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"cat /private/file --token secret-value"}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"State?"}},"buckets":["done"],"matches":{"done":{"lit":"done"}},"routes":{"done":"send-result"}},{"id":"send-result","uses":"send","with":{"target":{"lit":"private-tab"}}}]})}else{json!({"schema_version":2,"kind":"pipeline","name":"Portable pipeline","note":"Report release state","trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":"private-input"}},"steps":[{"id":"run","run":{"lit":"echo private-command"}}],"result":{"to":"owner"}})}).unwrap()
}
fn job(doc: &Document, team: &str) -> Sentinel {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(
        root.path().join("jobs.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let bound = doc.bind(&[]).unwrap();
    let doc = Document::from_definition(&bound).unwrap();
    let lowered = doc.lower().unwrap();
    let mut job = if let Some(spec) = lowered.get("spec") {
        store
            .prepare_pipeline(
                serde_json::from_value(spec.clone()).unwrap(),
                owner(team),
                false,
            )
            .unwrap()
    } else {
        store
            .prepare(
                serde_json::from_value(lowered["create"].clone()).unwrap(),
                owner(team),
                false,
            )
            .unwrap()
    };
    job.document = Some(bound);
    job
}
fn bindings(recipe: &Recipe, definition: &Value) -> Vec<String> {
    fn collect(a: &Value, b: &Value, out: &mut Vec<String>) {
        if let Some(key) = a.get("input").and_then(Value::as_str) {
            let value = b.get("lit").unwrap_or(b);
            out.push(format!(
                "{key}={}",
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            ));
            return;
        }
        match (a, b) {
            (Value::Object(a), Value::Object(b)) => {
                for (k, v) in a {
                    if k != "inputs" {
                        if let Some(other) = b.get(k) {
                            collect(v, other, out);
                        }
                    }
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                for (a, b) in a.iter().zip(b) {
                    collect(a, b, out)
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    collect(&recipe.definition, definition, &mut out);
    out
}
#[test]
fn blueprint_export_redacts_local_references_secrets_and_runtime() {
    let job = job(&native(true), "private-team");
    let recipe = export(&job).unwrap();
    let raw = serde_json::to_string(&recipe).unwrap();
    for forbidden in [
        "private-tab",
        "private-owner",
        "private-workspace",
        "private-team",
        "/private",
        "secret-value",
        "machine",
        "expires_at",
        "definition_version",
    ] {
        assert!(!raw.contains(forbidden), "leaked {forbidden}");
    }
    assert!(!Document::from_recipe(&recipe).unwrap().inputs.is_empty());
}
#[test]
fn blueprint_recipe_round_trip_into_another_team_preserves_definition() {
    let original = job(&native(true), "first");
    let recipe = export(&original).unwrap();
    let bound = recipe
        .bind(&bindings(&recipe, &definition(&original)))
        .unwrap();
    assert_eq!(bound, definition(&original));
    let imported = job(&Document::from_definition(&bound).unwrap(), "second");
    assert_eq!(original.document, imported.document);
    assert_ne!(original.team_id, imported.team_id);
    assert_ne!(original.id, imported.id);
}
#[test]
fn blueprint_pipeline_export_round_trip_preserves_trigger_steps_and_result() {
    let original = job(&native(false), "first");
    let recipe = export(&original).unwrap();
    let bound = recipe
        .bind(&bindings(&recipe, &definition(&original)))
        .unwrap();
    assert_eq!(bound, definition(&original));
    assert_eq!(
        Document::from_definition(&bound).unwrap().lower().unwrap()["spec"],
        serde_json::to_value(original.pipeline()).unwrap()
    );
}
#[test]
fn blueprint_import_refuses_v1_export_handler_keys() {
    assert!(Recipe::parse(
        r#"{"schema_version":1,"placeholders":{},"definition":{"create":{"name":"old"}}}"#
    )
    .is_err());
    assert!(
        Recipe::parse(r#"{"schema_version":2,"definition":{"create":{"name":"old"}}}"#).is_err()
    );
}
#[test]
fn exported_source_recipes_keep_kind_when_inputs_and_routes_are_edited() {
    let doc=Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"Source watch","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"check","uses":"http","with":{"url":{"lit":"https://example.com/health"}},"routes":{"down":"@stop"}}]})).unwrap();
    let original = job(&doc, "first");
    let recipe = export(&original).unwrap();
    let mut edited = Document::from_recipe(&recipe).unwrap();
    edited.name = "Customized source".into();
    edited.steps[0].routes.insert("up".into(), "@stop".into());
    let bound = edited
        .bind(&bindings(&recipe, &definition(&original)))
        .unwrap();
    let typed = Document::from_definition(&bound).unwrap();
    assert_eq!(typed.steps[0].kind(), "http");
    assert_eq!(
        typed.steps[0].params["url"],
        ValueForm::literal("https://example.com/health")
    );
    assert_eq!(typed.steps[0].routes["up"], "@stop");
}

#[test]
fn blueprint_v2_check_export_redacts_decision_and_pattern_literals() {
    let mut doc = native(true);
    doc.steps[1].params.insert(
        "question".into(),
        ValueForm::literal("private-question-marker"),
    );
    doc.steps[1]
        .matches
        .insert("done".into(), ValueForm::literal("private-match-marker"));
    let recipe = export(&job(&doc, "team")).unwrap();
    let raw = serde_json::to_string(&recipe).unwrap();
    for literal in ["private-question-marker", "private-match-marker"] {
        assert!(!raw.contains(literal), "leaked {literal}");
    }
    assert_eq!(
        recipe
            .bind(&bindings(&recipe, &serde_json::to_value(&doc).unwrap()))
            .unwrap(),
        serde_json::to_value(&doc).unwrap()
    );
    doc.steps[0].uses = Some(codeterm_blueprint_types::blueprint_document::NodeKind::Terminal);
    doc.steps[0].params = std::collections::BTreeMap::from([
        ("kind".into(), ValueForm::literal("tab")),
        ("id".into(), ValueForm::literal("opaque-tab")),
        (
            "pattern".into(),
            ValueForm::literal("private-terminal-marker"),
        ),
    ]);
    let recipe = export(&job(&doc, "team")).unwrap();
    assert!(!serde_json::to_string(&recipe)
        .unwrap()
        .contains("private-terminal-marker"));
}

#[test]
fn blueprint_v2_check_persistence_refuses_stale_native_document() {
    for watch in [false, true] {
        let original = job(&native(watch), "team");
        assert!(crate::storage::codec::import(&original).is_ok());
        let mut changed = original.clone();
        changed.question = "different-runtime-question".into();
        assert!(crate::storage::codec::import(&changed)
            .unwrap_err()
            .contains("differs from its runtime"));
        let mut changed = original;
        changed.interval_ms += 10_000;
        assert!(crate::storage::codec::import(&changed)
            .unwrap_err()
            .contains("differs from its runtime"));
    }
}
