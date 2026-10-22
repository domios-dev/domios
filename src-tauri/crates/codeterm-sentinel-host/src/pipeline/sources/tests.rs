use super::*;
#[test]
fn failing_observation_preserves_done_route_unless_explicitly_overridden() {
    let mut step: Step = serde_json::from_value(serde_json::json!({"name":"CI", "input":"text","kind":{"kind":"github","probe":{"repo":"owner/repo","resource":"run","id":42}}})).unwrap();
    let reading = || SourceReading {
        state: "failing".into(),
        summary: "failing · Linux".into(),
        output: serde_json::json!({"source_state":"failing", "failed_jobs":["Linux"]}),
    };
    let result = reading_result(&step, reading());
    assert_eq!(result.outcome, StepOutcome::Done);
    assert_eq!(result.stored_outcome, "failing");
    assert!(result.next_text.contains("Linux"));
    step.on.insert("failing".into(), "stop".into());
    assert_eq!(
        reading_result(&step, reading()).outcome,
        StepOutcome::Bucket("failing".into())
    );
}
#[test]
fn health_observation_routes_down_and_retains_status_output() {
    let step: Step = serde_json::from_value(serde_json::json!({"name":"health", "input":"text","kind":{"kind":"http","probe":{"url":"https://example.com"}}})).unwrap();
    let result = reading_result(
        &step,
        SourceReading {
            state: "down".into(),
            summary: "down · 503".into(),
            output: serde_json::json!({"state":"down","code":503}),
        },
    );
    assert_eq!(result.outcome, StepOutcome::Bucket("down".into()));
    assert_eq!(result.stored_outcome, "down");
    assert_eq!(result.state, "done");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.next_text).unwrap()["code"],
        503
    );
}

#[test]
fn blueprint_pipeline_run_receives_previous_source_json_on_stdin() {
    use crate::store::{Owner, Store};
    use codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec;
    use codeterm_service_core::cron::clock::MockClock;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    let directory = tempfile::tempdir().unwrap();
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let fixture = ServiceFixture::new(mux);
    let spec: BlueprintSpec = serde_json::from_value(serde_json::json!({
        "name":"stdin-pipeline", "trigger":{"type":"schedule","every":"20s"},
        "steps":[{"name":"read", "input":"text", "kind":{"kind":"command","command":"echo ignored"}}]
    })).unwrap();
    let step = spec.steps[0].clone();
    let store = Store::new(
        directory.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let job = store
        .prepare_pipeline(
            spec,
            Owner {
                creator_tab_id: creator,
                team_id: None,
                workspace_id: workspace,
                cwd: directory.path().to_string_lossy().into_owned(),
            },
            true,
        )
        .unwrap();
    let lease = Lease {
        job,
        cancelled: Default::default(),
        event: None,
    };
    let handle = crate::scheduler::fixture_handle_without_app(store);
    let source = reading_result(
        &step,
        SourceReading {
            state: "done".into(),
            summary: "opaque".into(),
            output: serde_json::json!({"count":7,"items":["opaque $(exit 17) `exit 19`"]}),
        },
    );
    #[cfg(windows)]
    let read = "[Console]::Out.Write([Console]::In.ReadToEnd())";
    #[cfg(unix)]
    let read = "cat";
    let step = Step {
        kind: StepKind::Command {
            command: read.into(),
        },
        ..step
    };
    let result = run(&handle, &fixture.service, &lease, &step, &source.next_text);
    assert_eq!(result.outcome, StepOutcome::Done);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.next_text).unwrap(),
        serde_json::from_str::<serde_json::Value>(&source.next_text).unwrap()
    );
}
