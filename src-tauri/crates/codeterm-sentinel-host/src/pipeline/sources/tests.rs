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
    use codeterm_service_core::cron::clock::MockClock;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    let directory = tempfile::tempdir().unwrap();
    let (mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let document = serde_json::json!({"schema_version":2,"kind":"pipeline","name":"stdin-pipeline","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"read","run":{"lit":"echo ignored"}}]});
    let store = Store::new(
        directory.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let job = store
        .prepare_document(
            document,
            Owner {
                creator_tab_id: creator,
                team_id: None,
                workspace_id: workspace,
                cwd: directory.path().to_string_lossy().into_owned(),
            },
            true,
        )
        .unwrap();
    let step = job.pipeline().unwrap().steps[0].clone();
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
            timeout_ms: codeterm_blueprint_types::blueprint_pipeline::COMMAND_STEP_RUNTIME_MS,
            command: read.into(),
        },
        ..step
    };
    let result = run(
        &handle,
        &lease,
        &step,
        &ExecutionEnvelope::from_text(&source.next_text),
        probe::PROBE_RUNTIME,
    );
    assert_eq!(result.outcome, StepOutcome::Done);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.next_text).unwrap()["value"],
        serde_json::from_str::<serde_json::Value>(&source.next_text).unwrap()
    );
}

#[test]
fn blueprint_run_environment_does_not_export_instance_authority() {
    let environment = crate::probe_env::command();
    #[cfg(windows)]
    let command = "[Environment]::GetEnvironmentVariables().Keys | Where-Object { $_ -match '^(CODETERM|DOMIOS)(_|$)' }";
    #[cfg(unix)]
    let command = "env | while IFS='=' read -r key rest; do case \"$key\" in CODETERM|DOMIOS|CODETERM_*|DOMIOS_*) printf '%s\\n' \"$key\";; esac; done";
    let directory = tempfile::tempdir().unwrap();
    let output = probe::run_with_budget_input(
        command,
        directory.path(),
        probe::PROBE_RUNTIME,
        probe::PROBE_RUNTIME,
        &std::sync::Arc::new(probe::owner::ProbeResourceOwner::new()),
        &environment,
        "opaque untrusted previous output",
    )
    .unwrap();
    assert!(output.success);
    assert!(output.stdout_text().trim().is_empty());
}
