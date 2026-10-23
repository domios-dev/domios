use super::*;

#[test]
fn blueprint_agent_step_is_bounded_and_routes_done_or_failed() {
    let agent = StepKind::Agent {
        provider: "fixture".into(),
        profile: "reader".into(),
        task: "opaque".into(),
        deadline_ms: 1000,
        max_tokens: 1000,
        max_turns: 2,
    };
    assert_eq!(agent.name(), "agent");
    assert_eq!(agent.output_type(), IoType::Text);
    assert_eq!(
        agent.outcomes().unwrap(),
        BTreeSet::from(["done".into(), "failed".into()])
    );
    assert!(serde_json::from_value::<StepKind>(serde_json::json!({"kind":"agent","provider":"fixture","profile":"reader","task":"opaque","deadline_ms":1000,"max_tokens":1000,"max_turns":2,"unknown":true})).is_err());
    let mut step = command("bounded", IoType::Text);
    step.kind = agent;
    assert!(validate(&spec(vec![step.clone()]), &[]).is_ok());
    if let StepKind::Agent { max_turns, .. } = &mut step.kind {
        *max_turns = 0;
    }
    assert!(validate(&spec(vec![step]), &[])
        .unwrap_err()
        .iter()
        .any(|e| e.field == "steps[0].agent"));
}
use crate::sentinel_input::DataRef;

fn command(name: &str, input: IoType) -> Step {
    Step {
        name: name.into(),
        input,
        kind: StepKind::Command {
            timeout_ms: crate::blueprint_pipeline::COMMAND_STEP_RUNTIME_MS,
            command: "printf ready".into(),
        },
        on: BTreeMap::new(),
        execution: None,
    }
}

fn spec(steps: Vec<Step>) -> BlueprintSpec {
    BlueprintSpec {
        note: None,
        name: "Echo chain".into(),
        owner: BlueprintOwner::Tab,
        trigger: Trigger::Schedule {
            every: "20s".into(),
        },
        steps,
        result: BlueprintResult {
            to: ResultTarget::Owner,
        },
    }
}

#[test]
fn blueprint_pipeline_spec_validation_checks_name_trigger_and_step_bounds() {
    let mut value = spec(vec![command("run", IoType::Text)]);
    assert_eq!(validate(&value, &[]), Ok(()));
    value.name.clear();
    value.trigger = Trigger::Schedule { every: "0s".into() };
    let errors = validate(&value, &[]).unwrap_err();
    assert!(errors.iter().any(|error| error.field == "name"));
    assert!(errors.iter().any(|error| error.field == "trigger.every"));
}

#[test]
fn blueprint_pipeline_spec_round_trips_one_authored_chain() {
    let source = serde_json::json!({
        "name": "Echo chain",
        "owner": "tab",
        "trigger": {"type": "schedule", "every": "20s"},
        "steps": [{
            "name": "echo",
            "input": "text",
            "kind": {"kind": "command", "command": "printf ready", "timeout_ms": 10000}
        }],
        "result": {"to": "owner"}
    });
    let parsed: BlueprintSpec = serde_json::from_value(source.clone()).unwrap();
    assert_eq!(validate(&parsed, &[]), Ok(()));
    assert_eq!(serde_json::to_value(parsed).unwrap(), source);
}

#[test]
fn blueprint_pipeline_forward_only_branches_reject_missing_and_backward_targets() {
    let mut first = command("first", IoType::Text);
    first.on.insert("done".into(), "first".into());
    assert!(validate(
        &spec(vec![first.clone(), command("second", IoType::Text)]),
        &[]
    )
    .unwrap_err()
    .iter()
    .any(|error| error.reason.contains("forward-only")));
    first.on.insert("done".into(), "missing".into());
    assert!(
        validate(&spec(vec![first, command("second", IoType::Text)]), &[])
            .unwrap_err()
            .iter()
            .any(|error| error.reason.contains("does not exist"))
    );
    let mut first = command("first", IoType::Text);
    first.on.insert("done".into(), "third".into());
    assert_eq!(
        validate(
            &spec(vec![
                first,
                command("second", IoType::Text),
                command("third", IoType::Text)
            ]),
            &[]
        ),
        Ok(())
    );
}

#[test]
fn blueprint_pipeline_step_io_types_match_previous_output() {
    let mut second = command("second", IoType::File);
    second.kind = StepKind::Filter {
        pattern: "ready".into(),
        bucket: None,
    };
    let errors = validate(&spec(vec![command("first", IoType::Text), second]), &[]).unwrap_err();
    assert!(errors.iter().any(|error| error.field == "steps[1].input"));
}

#[test]
fn blueprint_pipeline_converter_chain_is_capability_ordered_and_bounded() {
    let converters = [
        ConverterEdge {
            from: MediaKind::Audio,
            to: MediaKind::Video,
        },
        ConverterEdge {
            from: MediaKind::Video,
            to: MediaKind::Text,
        },
    ];
    assert_eq!(
        converter_path(MediaKind::Audio, MediaKind::Text, &converters),
        Some(vec![0, 1])
    );
    for kind in MediaKind::ALL {
        assert_eq!(converter_path(kind, kind, &converters), Some(vec![]));
        assert_eq!(converter_path(kind, kind, &[]), Some(vec![]));
    }
    let spec = BlueprintSpec {
        note: None,
        name: "Voice".into(),
        owner: BlueprintOwner::Tab,
        trigger: Trigger::Input {
            kind: MediaKind::Audio,
            source: DataRef::Url("https://example.com/voice.wav".into()),
        },
        steps: vec![
            Step {
                name: "convert".into(),
                input: IoType::Audio,
                kind: StepKind::Convert {
                    from: MediaKind::Audio,
                    to: MediaKind::Text,
                },
                on: BTreeMap::new(),
                execution: None,
            },
            command("inspect", IoType::Text),
        ],
        result: BlueprintResult {
            to: ResultTarget::Owner,
        },
    };
    assert_eq!(validate(&spec, &converters), Ok(()));
}

#[test]
fn blueprint_pipeline_runner_advance_table() {
    for (outcome, expected, status) in [
        (StepOutcome::Done, Some(1), RunStatus::Running),
        (StepOutcome::Failed, None, RunStatus::Failed),
    ] {
        let state = RunnerState::start(&spec(vec![
            command("one", IoType::Text),
            command("two", IoType::Text),
        ]))
        .unwrap();
        let advanced = advance(&state, outcome).unwrap();
        assert_eq!(advanced.current_step, expected);
        assert_eq!(advanced.status, status);
    }
    let mut ask = Step {
        name: "choose".into(),
        input: IoType::Text,
        kind: StepKind::Ask {
            question: "State?".into(),
            buckets: vec!["ready".into()],
        },
        on: BTreeMap::new(),
        execution: None,
    };
    ask.on.insert("ready".into(), "stop".into());
    let state = RunnerState::start(&spec(vec![ask])).unwrap();
    let advanced = advance(&state, StepOutcome::Bucket("ready".into())).unwrap();
    assert_eq!(advanced.status, RunStatus::Done);

    let mut first = command("one", IoType::Text);
    first.on.insert("done".into(), "three".into());
    let state = RunnerState::start(&spec(vec![
        first,
        command("two", IoType::Text),
        command("three", IoType::Text),
    ]))
    .unwrap();
    assert_eq!(
        advance(&state, StepOutcome::Done).unwrap().current_step,
        Some(2)
    );
}

#[test]
fn blueprint_pipeline_rejects_undeclared_buckets_and_branch_outcomes() {
    let mut ask = Step {
        name: "choose".into(),
        input: IoType::Text,
        kind: StepKind::Ask {
            question: "State?".into(),
            buckets: vec!["ready".into()],
        },
        on: BTreeMap::new(),
        execution: None,
    };
    ask.on.insert("failed".into(), "stop".into());
    let errors = validate(&spec(vec![ask]), &[]).unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.field == "steps[0].on.failed"));
    let invalid = Step {
        name: "choose".into(),
        input: IoType::Text,
        kind: StepKind::Ask {
            question: "State?".into(),
            buckets: vec!["other".into()],
        },
        on: BTreeMap::new(),
        execution: None,
    };
    assert!(validate(&spec(vec![invalid]), &[])
        .unwrap_err()
        .iter()
        .any(|error| error.field == "steps[0].buckets"));
}

#[test]
fn blueprint_pipeline_event_triggers_are_available_and_input_waits_for_runtime() {
    assert!(ensure_trigger_available(&Trigger::Schedule {
        every: "20s".into()
    })
    .is_ok());
    assert!(ensure_trigger_available(&Trigger::Event {
        kind: "fixture.event".into(),
        params: Value::Null,
    })
    .is_ok());
    assert!(ensure_trigger_available(&Trigger::Input {
        kind: MediaKind::Text,
        source: DataRef::Text("hello".into()),
    })
    .is_ok());
}

#[test]
fn blueprint_event_subscription_action_table_covers_owner_and_provider_states() {
    use SubscriptionAction::*;
    use SubscriptionOwnerState::*;
    use SubscriptionProviderState::*;
    for (subscribed, owner, provider, expected) in [
        (false, Active, Available, Subscribe),
        (true, Active, Available, Keep),
        (false, Active, Missing, Unavailable),
        (false, Active, Ungranted, Unavailable),
        (true, Active, Missing, Unsubscribe),
        (true, Active, Ungranted, Unsubscribe),
        (true, Stopped, Available, Unsubscribe),
        (true, Expired, Available, Unsubscribe),
        (true, OwnerClosed, Available, Unsubscribe),
        (true, TeamClosed, Available, Unsubscribe),
        (true, Amended, Available, Unsubscribe),
        (false, Stopped, Available, Keep),
    ] {
        assert_eq!(
            next_subscription_action(subscribed, owner, provider),
            expected
        );
    }
}

#[test]
fn blueprint_pipeline_journal_summary_is_bounded_and_control_free() {
    let entry = StepJournalEntry {
        step_index: 0,
        step_name: "run".into(),
        step_kind: "command".into(),
        state: JournalStepState::Done,
        outcome: Some("done".into()),
        started_at_ms: Some(1),
        ended_at_ms: Some(2),
        result_summary: Some(StepJournalEntry::bound_summary(&format!(
            "{}\n{}",
            "x".repeat(199),
            "tail"
        ))),
        reason: None,
    };
    entry.validate().unwrap();
    let expected = "x".repeat(199);
    assert_eq!(entry.result_summary.as_deref(), Some(expected.as_str()));
    assert_eq!(StepJournalEntry::bound_summary("a\u{1b}b"), "ab");
}

#[test]
fn github_step_round_trips_validates_scope_and_routes_query_failure() {
    let value = serde_json::json!({"kind":"github","probe":{"repo":"owner/project","resource":"jobs","id":456}});
    let github: StepKind = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&github).unwrap(), value);
    assert_eq!(github.name(), "github");
    assert_eq!(github.output_type(), IoType::Text);
    assert_eq!(
        github.outcomes().unwrap(),
        BTreeSet::from(["done".into(), "failed".into(), "failing".into()])
    );
    let mut step = command("GitHub jobs", IoType::Text);
    step.kind = github;
    assert!(validate(&spec(vec![step.clone()]), &[]).is_ok());
    if let StepKind::Github { probe } = &mut step.kind {
        probe.repo = "owner/repo; whoami".into();
    }
    assert!(validate(&spec(vec![step]), &[])
        .unwrap_err()
        .iter()
        .any(|e| e.field == "steps[0].github"));
    assert!(serde_json::from_value::<StepKind>(serde_json::json!({"kind":"github","probe":{"repo":"owner/repo","resource":"delete","id":456}})).is_err());
}

#[test]
fn http_step_validates_defaults_and_declares_health_routes() {
    let kind: StepKind = serde_json::from_value(
        serde_json::json!({"kind":"http","probe":{"url":"https://example.com/health"}}),
    )
    .unwrap();
    assert_eq!(kind.name(), "http");
    assert_eq!(kind.output_type(), IoType::Text);
    assert_eq!(
        kind.outcomes().unwrap(),
        BTreeSet::from(["up".into(), "down".into(), "failed".into()])
    );
    let mut step = command("health", IoType::Text);
    step.kind = kind;
    assert!(validate(&spec(vec![step.clone()]), &[]).is_ok());
    if let StepKind::Http { probe } = &mut step.kind {
        probe.timeout_ms = 10001;
    }
    assert!(validate(&spec(vec![step]), &[])
        .unwrap_err()
        .iter()
        .any(|error| error.field == "steps[0].http"));
}

#[test]
fn blueprint_route_defaults_and_runtime_agree() {
    let step = command("first", IoType::Text);
    assert_eq!(route_target(&step.on, "failed"), "stop");
    assert_eq!(route_target(&step.on, "done"), "next");
    let mut pipeline = spec(vec![step, command("second", IoType::Text)]);
    assert_eq!(
        advance(&RunnerState::start(&pipeline).unwrap(), StepOutcome::Failed)
            .unwrap()
            .status,
        RunStatus::Failed
    );
    pipeline.steps[0].on.insert("failed".into(), "next".into());
    assert_eq!(route_target(&pipeline.steps[0].on, "failed"), "next");
    assert_eq!(
        advance(&RunnerState::start(&pipeline).unwrap(), StepOutcome::Failed)
            .unwrap()
            .current_step,
        Some(1)
    );
}
