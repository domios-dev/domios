use super::*;
use crate::blueprint_pipeline::{StepKind, Trigger};
use crate::sentinel_input::{DataRef, MediaKind};
use std::collections::BTreeSet;

#[test]
fn blueprint_catalog_equals_enum_names_and_shared_golden_fixture() {
    let steps = [
        StepKind::Github {
            probe: crate::blueprint_github::GithubProbe {
                repo: "owner/repo".into(),
                resource: crate::blueprint_github::GithubResource::Run,
                id: 42,
                org: None,
                label: None,
                timeout_ms: 60000,
            },
        },
        StepKind::Command {
            timeout_ms: crate::blueprint_pipeline::COMMAND_STEP_RUNTIME_MS,
            command: "status".into(),
        },
        StepKind::Ask {
            question: "state?".into(),
            buckets: vec!["ready".into()],
        },
        StepKind::Filter {
            pattern: "ready".into(),
            bucket: None,
        },
        StepKind::Convert {
            from: MediaKind::Audio,
            to: MediaKind::Text,
        },
        StepKind::Agent {
            review: false,
            provider: "fixture".into(),
            profile: "reader".into(),
            task: "inspect".into(),
            deadline_ms: 1000,
            max_tokens: 1000,
            max_turns: 2,
        },
        StepKind::Http {
            probe: serde_json::from_value(serde_json::json!({"url":"https://example.com/health"}))
                .unwrap(),
        },
    ];
    assert_eq!(
        steps.iter().map(StepKind::name).collect::<BTreeSet<_>>(),
        STEPS.iter().map(|entry| entry.name).collect()
    );
    let triggers = [
        Trigger::Webhook { schema: None },
        Trigger::Schedule {
            every: "20s".into(),
        },
        Trigger::Event {
            kind: "event".into(),
            params: serde_json::json!({}),
        },
        Trigger::Input {
            kind: MediaKind::Text,
            source: DataRef::Text("body".into()),
        },
    ];
    assert_eq!(
        triggers.iter().map(Trigger::name).collect::<BTreeSet<_>>(),
        TRIGGERS.iter().map(|entry| entry.name).collect()
    );
    let nodes = TRIGGERS
        .iter()
        .chain(STEPS)
        .chain(ROUTES)
        .chain(SOURCES_AND_MODES)
        .map(NodeDescriptor::value)
        .collect::<Vec<_>>();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/blueprint-nodes.golden.json"
    ))
    .unwrap();
    assert_eq!(serde_json::json!({"nodes":nodes}), fixture);
    assert_eq!(
        steps[2].outcomes().unwrap(),
        BTreeSet::from(["ready".into(), "other".into()])
    );
}
