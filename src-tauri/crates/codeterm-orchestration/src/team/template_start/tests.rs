use std::collections::BTreeMap;

use codeterm_team_package::contracts::{TemplateRef, TemplateStartRequest, TEMPLATE_UNAVAILABLE};

use crate::{OrchestrationPairExt, OrchestrationPairParams};

#[test]
fn a_named_template_is_refused_as_unavailable_and_creates_nothing() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let fixture = crate::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("template-orchestrator"),
    ));
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("template-planner"),
    ));
    let tabs_before = fixture.service.mux.read().snapshot().tabs.len();
    let teams_before = fixture.service.mux.read().team_count();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "template-orchestrator".into(),
                    planner_agent: "template-planner".into(),
                    workspace_id: Some(workspace_id),
                    template: Some(TemplateStartRequest {
                        template: TemplateRef {
                            name: "dev.example/probe".into(),
                            version: None,
                        },
                        team_id: None,
                        inputs: BTreeMap::new(),
                        opt_out: Vec::new(),
                        decision_id: None,
                        manager_models: BTreeMap::new(),
                    }),
                    ..Default::default()
                }),
        )
        .expect_err("template start is not available yet");
    assert!(error.starts_with(TEMPLATE_UNAVAILABLE), "{error}");
    let mux = fixture.service.mux.read();
    assert_eq!(mux.snapshot().tabs.len(), tabs_before);
    assert_eq!(mux.team_count(), teams_before);
    assert!(fixture.service.plan_store.list_index().unwrap().is_empty());
}
