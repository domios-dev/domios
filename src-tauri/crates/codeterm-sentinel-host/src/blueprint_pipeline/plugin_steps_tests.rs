use super::*;
use codeterm_plugin::blueprint_step::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::Arc;

struct Unavailable;
impl BlueprintStep for Unavailable {
    fn estimate(&self, _: &StepCall<'_>) -> Result<Option<Estimate>, String> {
        Err("not invoked".into())
    }
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
fn plugin_steps_catalog_is_discoverable_and_removal_revokes_admission() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let registry = fixture
        .service
        .ext::<Arc<codeterm_plugin::graph_nodes::GraphNodeRegistry>>();
    let descriptors=parse_descriptors(&json!([{"contractVersion":1,"revision":1,"id":"fixture.generate","title":"Generate","description":"Fixture",
        "params":{"type":"object","properties":{}},"inputs":{},"outputs":{},"outcomes":["done","failed"],
        "effects":{"sideEffects":false,"potentiallyPaid":false},"lifecycle":{"reconcile":false,"cancel":false}}])).unwrap();
    registry
        .register_steps("fixture", &descriptors, Arc::new(Unavailable))
        .unwrap();
    let nodes = fixture.service.blueprint_pipeline_nodes();
    let node = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["kind"] == "fixture.generate")
        .unwrap();
    assert_eq!(node["descriptor"]["contractVersion"], 1);
    assert_eq!(node["available"], true);
    assert!(node.get("hold_reason").is_none());
    assert!(serde_json::from_value::<StepKind>(json!({"kind":"fixture.generate"})).is_err());
    registry.unregister("fixture");
    assert!(!fixture
        .service
        .blueprint_pipeline_nodes()
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["kind"] == "fixture.generate"));
}
