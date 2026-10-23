use super::*;
use codeterm_blueprint_types::blueprint_document;
use codeterm_service_core::cron::clock::MockClock;
use std::cell::Cell;

fn fixture() -> (tempfile::TempDir, Handle, Lease) {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::store::Store::new(
        directory.path().join("jobs.json"),
        MockClock::starting_at(1000),
    );
    let job = store
        .prepare_document(
            serde_json::json!({
                "schema_version":2,"kind":"pipeline","name":"budget-fixture","owner":"tab",
                "trigger":{"type":"schedule","every":{"lit":"20s"}},
                "steps":[{"id":"command","run":{"lit":"opaque"}}]
            }),
            crate::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "workspace".into(),
                cwd: directory.path().to_string_lossy().into_owned(),
            },
            true,
        )
        .unwrap();
    (
        directory,
        crate::scheduler::fixture_handle_without_app(store),
        Lease {
            job,
            cancelled: Default::default(),
            event: None,
        },
    )
}

#[test]
fn blueprint_v2_budget_spawn_return_excludes_authorization_and_deadline_is_typed() {
    let (_directory, handle, lease) = fixture();
    let step = &lease.job.pipeline().unwrap().steps[0];
    let result = run_with_probe(
        &handle,
        &lease,
        step,
        &ExecutionEnvelope::from_text("opaque"),
        Duration::from_secs(60),
        |_, limit, runtime, _| {
            assert_eq!(runtime, Duration::from_secs(10));
            let admitted = Instant::now();
            let clock = Cell::new(admitted);
            let mut deadlines = probe::process::Deadlines::at(limit, runtime, admitted).unwrap();
            let child = deadlines.spawn_with(
                || {
                    clock.set(admitted + Duration::from_secs(20));
                    "fake-child"
                },
                || clock.get(),
            );
            assert_eq!(child, "fake-child");
            assert_eq!(
                deadlines.remaining_operation_at(clock.get()),
                Duration::from_secs(10)
            );
            assert!(!deadlines.expired_operation_at(clock.get() + Duration::from_millis(9_999)));
            assert!(deadlines.expired_operation_at(clock.get() + Duration::from_secs(10)));
            Err(probe::process::ProbeFailure::new("probe_timeout"))
        },
    );
    assert_eq!(result.reason, Some(JournalReason::Deadline));
    assert_eq!(result.outcome, StepOutcome::Failed);
    assert!(!result.uncertain);
}

#[test]
fn blueprint_v2_budget_command_defaults_overrides_and_caller_clamp() {
    let (_directory, handle, lease) = fixture();
    for (timeout, expected) in [(None, 10), (Some("2s"), 2), (Some("60s"), 60)] {
        let mut definition = lease.job.document.clone().unwrap();
        definition["steps"][0]
            .as_object_mut()
            .unwrap()
            .remove("timeout");
        if let Some(timeout) = timeout {
            definition["steps"][0]["timeout"] = serde_json::json!({"lit":timeout});
        }
        let document = blueprint_document::Document::from_definition(&definition).unwrap();
        let spec: blueprint_pipeline::BlueprintSpec =
            serde_json::from_value(document.lower().unwrap()["spec"].clone()).unwrap();
        let result = run_with_probe(
            &handle,
            &lease,
            &spec.steps[0],
            &ExecutionEnvelope::from_text("input"),
            Duration::from_secs(90),
            |_, limit, runtime, _| {
                assert_eq!(runtime, Duration::from_secs(expected));
                assert_eq!(limit, Duration::from_secs(90));
                Ok(probe::fixture_output(0, b"complete", b""))
            },
        );
        assert_eq!(result.outcome, StepOutcome::Done);
    }
    let result = run_with_probe(
        &handle,
        &lease,
        &lease.job.pipeline().unwrap().steps[0],
        &ExecutionEnvelope::from_text("input"),
        Duration::from_secs(5),
        |_, limit, runtime, _| {
            let at = Instant::now();
            let mut deadlines = probe::process::Deadlines::at(limit, runtime, at).unwrap();
            deadlines.spawn_with(|| (), || at + Duration::from_secs(2));
            assert_eq!(deadlines.hard, at + Duration::from_secs(5));
            assert!(deadlines.expired_operation_at(at + Duration::from_millis(4_500)));
            Err(probe::process::ProbeFailure::new("probe_timeout"))
        },
    );
    assert_eq!(result.reason, Some(JournalReason::Deadline));
    let at = Instant::now();
    let mut deadlines =
        probe::process::Deadlines::at(Duration::from_secs(5), Duration::from_secs(10), at).unwrap();
    deadlines.spawn_with(|| (), || at + Duration::from_secs(2));
    assert_eq!(deadlines.hard, at + Duration::from_secs(5));
    assert_eq!(deadlines.operation, at + Duration::from_millis(4_500));
    assert_eq!(
        deadlines.remaining_operation_at(at + Duration::from_secs(2)),
        Duration::from_millis(2_500)
    );
}

#[test]
fn blueprint_v2_budget_checker_positions_timeout_bound_and_accepts_full_range() {
    for timeout in ["0s", "-1s", "61s"] {
        let source = format!("schema_version: 2\nkind: pipeline\nname: budget-check\ntrigger: {{type: schedule, every: {{lit: 20s}}}}\nsteps:\n  - id: command\n    run: {{lit: echo}}\n    timeout: {{lit: '{timeout}'}}\n");
        let check = blueprint_document::check("budget.yaml", &source, &[]);
        assert!(!check.ok);
        let diagnostic = check.diagnostics.iter().find(|d| d.rule == "B1").unwrap();
        assert_eq!(diagnostic.field, "steps[0].timeout");
        assert_eq!(diagnostic.file, "budget.yaml");
        assert_eq!(diagnostic.line, 8);
        assert!(diagnostic.column > 0);
    }
    for timeout in ["1s", "10s", "60s"] {
        let source = format!("schema_version: 2\nkind: pipeline\nname: budget-check\ntrigger: {{type: schedule, every: {{lit: 20s}}}}\nsteps:\n  - id: command\n    run: {{lit: echo}}\n    timeout: {{lit: '{timeout}'}}\n");
        let check = blueprint_document::check("budget.yaml", &source, &[]);
        assert!(check.ok, "{timeout}: {:?}", check.diagnostics);
    }
}

#[test]
fn blueprint_v2_budget_slow_override_ends_at_its_timeout() {
    let (_directory, handle, lease) = fixture();
    let mut step = lease.job.pipeline().unwrap().steps[0].clone();
    step.kind = StepKind::Command {
        command: "opaque".into(),
        timeout_ms: 2_000,
    };
    let result = run_with_probe(
        &handle,
        &lease,
        &step,
        &ExecutionEnvelope::from_text("input"),
        Duration::from_secs(60),
        |_, limit, runtime, _| {
            assert_eq!(runtime, Duration::from_secs(2));
            let at = Instant::now();
            let mut budget = probe::process::Deadlines::at(limit, runtime, at).unwrap();
            budget.spawn_with(|| (), || at);
            assert_eq!(budget.operation, at + Duration::from_secs(2));
            assert_eq!(budget.hard, at + Duration::from_millis(2_500));
            assert!(budget.expired_operation_at(at + Duration::from_secs(3)));
            Err(probe::process::ProbeFailure::new("probe_timeout"))
        },
    );
    assert_eq!(result.reason, Some(JournalReason::Deadline));
    assert_eq!(result.outcome, StepOutcome::Failed);
}
