use super::*;
use codeterm_plugin::blueprint_step::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::Arc;

struct Unavailable;

#[test]
fn unavailable_plugin_errors_preserve_typed_step_locations() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let document = codeterm_blueprint_types::blueprint_document::parse(
        "fixture.yaml",
        include_str!("../../../../../tests/fixtures/blueprint-plugin-nodes.yaml"),
    )
    .unwrap()
    .document;
    let lowered = document.lower().unwrap();
    let spec: BlueprintSpec = serde_json::from_value(lowered["spec"].clone()).unwrap();
    let errors = plugin_validation::validate(&fixture.service, &spec);
    assert_eq!(errors.len(), 2);
    for (error, index) in errors.iter().zip([0, 1]) {
        assert_eq!(error.reason, "plugin_step_unavailable");
        assert_eq!(error.field, format!("steps[{index}].plugin"));
        assert_eq!(
            error.path,
            ValidationPath::Step {
                index,
                field: StepField::Common("plugin".into()),
            }
        );
        assert_eq!(
            error.path.document_field(),
            format!("pipeline.steps[{index}].plugin")
        );
    }
}

impl BlueprintStep for Unavailable {
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
        "effects":{"sideEffects":false},"lifecycle":{"reconcile":false,"cancel":false}}])).unwrap();
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
    let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(&json!({
        "schema_version":2,"kind":"pipeline","name":"bad-port",
        "trigger":{"type":"schedule","every":{"lit":"20s"}},
        "steps":[{"id":"a","uses":"fixture.generate"},{"id":"sink","run":{"lit":"echo"},"bind":{"in":"a.out.missing"}}],
        "result":{"to":"notify"}
    })).unwrap();
    let spec = serde_json::from_value(document.lower().unwrap()["spec"].clone()).unwrap();
    let errors = plugin_validation::validate(&fixture.service, &spec);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].reason, "undeclared plugin output port");
    assert_eq!(errors[0].field, "steps[1].bind");
    registry.unregister("fixture");
    assert!(!fixture
        .service
        .blueprint_pipeline_nodes()
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["kind"] == "fixture.generate"));
}
