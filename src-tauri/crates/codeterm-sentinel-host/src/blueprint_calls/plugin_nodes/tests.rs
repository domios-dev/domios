use super::*;
use codeterm_blueprint_types::blueprint_document::Document;
use codeterm_plugin::blueprint_step::*;
use std::sync::Arc;

struct Inert;

impl BlueprintStep for Inert {
    fn submit(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        Err("not invoked".into())
    }
    fn reconcile(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        Err("not invoked".into())
    }
    fn cancel(&self, _: &StepCall<'_>) -> Result<JobUpdate, String> {
        Err("not invoked".into())
    }
}

#[test]
fn viewer_plugin_nodes_carry_installed_descriptors_and_mark_missing_plugins_unavailable() {
    let registry = GraphNodeRegistry::default();
    let declared_steps = parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":"fixture.read","title":"Fixture: read","description":"Reads one item",
        "params":{"type":"object","properties":{}},"inputs":{"url":{"kind":"text","minItems":0,"maxItems":1}},
        "outputs":{"text":{"kind":"text","minItems":1,"maxItems":1}},"outcomes":["read","busy"],
        "effects":{"sideEffects":false},"lifecycle":{"reconcile":false,"cancel":false}}]))
    .unwrap();
    registry
        .register_steps("fixture", &declared_steps, Arc::new(Inert))
        .unwrap();
    let document = Document::from_definition(&json!({
        "schema_version":2,"kind":"pipeline","name":"viewer",
        "trigger":{"type":"schedule","every":{"lit":"20s"}},
        "steps":[{"id":"a","uses":"fixture.read"},{"id":"b","uses":"gone.step"},{"id":"c","run":{"lit":"echo"}}],
        "result":{"to":"notify"}
    }))
    .unwrap();
    let closure = FrozenClosure::from_document(document).unwrap();
    let nodes = descriptors(&registry, &closure);
    assert_eq!(
        nodes,
        json!({
            "fixture.read":{"title":"Fixture: read","plugin":"fixture","description":"Reads one item",
                "inputs":{"url":{"kind":"text","minItems":0,"maxItems":1}},
                "outputs":{"text":{"kind":"text","minItems":1,"maxItems":1}},
                "outcomes":["read","busy"],"available":true},
            "gone.step":{"available":false}
        })
    );
    registry.unregister("fixture");
    assert_eq!(
        descriptors(&registry, &closure)["fixture.read"],
        json!({"available":false})
    );
}
