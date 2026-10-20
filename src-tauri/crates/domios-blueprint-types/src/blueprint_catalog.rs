use serde_json::{json, Value};

pub struct NodeDescriptor {
    pub role: &'static str,
    pub kind: &'static str,
    pub name: &'static str,
    pub title: &'static str,
    pub metadata: &'static str,
}

impl NodeDescriptor {
    pub fn value(&self) -> Value {
        let mut value: Value = serde_json::from_str(self.metadata)
            .expect("static node descriptor metadata is valid JSON");
        let fields = value
            .as_object_mut()
            .expect("static node metadata is an object");
        fields.extend(
            json!({"role":self.role,"kind":self.kind,"type":self.name,"title":self.title})
                .as_object()
                .unwrap()
                .clone(),
        );
        value
    }
}

pub const TRIGGERS: &[NodeDescriptor] = &[
    NodeDescriptor {
        role: "trigger",
        kind: "trigger",
        name: "schedule",
        title: "Trigger: on a schedule",
        metadata: r#"{"params_schema":{"every":{"type":"duration","minimum":"10s"}}}"#,
    },
    NodeDescriptor {
        role: "trigger",
        kind: "trigger",
        name: "event",
        title: "Trigger: on an event",
        metadata: r#"{"params_schema":{"kind":"event source kind","params":"event source schema"}}"#,
    },
    NodeDescriptor {
        role: "trigger",
        kind: "trigger",
        name: "input",
        title: "Trigger: on input data",
        metadata: r#"{"params_schema":{"kind":"text|file|audio|video|image","source":"text|file|url"}}"#,
    },
];
pub const STEPS: &[NodeDescriptor] = &[
    NodeDescriptor {
        role: "step",
        kind: "step",
        name: "command",
        title: "Pipeline step: command",
        metadata: r#"{"params_schema":{"command":{"type":"string","max_length":16384}}}"#,
    },
    NodeDescriptor {
        role: "agent",
        kind: "step",
        name: "ask",
        title: "Pipeline step: ask an agent",
        metadata: r#"{"params_schema":{"question":"string","buckets":"declared string array"}}"#,
    },
    NodeDescriptor {
        role: "logic",
        kind: "step",
        name: "filter",
        title: "Pipeline step: filter",
        metadata: r#"{"params_schema":{"pattern":"regex","bucket":"optional bucket"}}"#,
    },
    NodeDescriptor {
        role: "step",
        kind: "step",
        name: "convert",
        title: "Pipeline step: convert",
        metadata: r#"{"params_schema":{"from":"media kind","to":"media kind"}}"#,
    },
    NodeDescriptor {
        role: "agent",
        kind: "step",
        name: "agent",
        title: "Pipeline step: bounded agent task",
        metadata: r#"{"params_schema":{"provider":"agent-task capable provider id","profile":"profile id","task":"bounded instruction","deadline_ms":"1..3600000","max_tokens":"1..1000000","max_turns":"1..100"}}"#,
    },
];
pub const ROUTES: &[NodeDescriptor] = &[
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "notify",
        title: "Action: notify the creator",
        metadata: r#"{"params_schema":{}}"#,
    },
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "send",
        title: "Action: send the payload to a terminal or agent",
        metadata: r#"{"params_schema":{"target":"same-Team tab"},"payload_schema":{"source":"stage output or graph text input"}}"#,
    },
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "cron",
        title: "Action: start a cron job",
        metadata: r#"{"params_schema":{"name":"cron name"}}"#,
    },
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "command",
        title: "Action: run a labelled command",
        metadata: r#"{"params_schema":{"label":"short name","command":"shell command","timeout_ms":"bound"}}"#,
    },
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "blueprint",
        title: "Action: activate the next stage",
        metadata: r#"{"params_schema":{"target":"blueprint:<exact-id>"}}"#,
    },
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "escalate",
        title: "Action: escalate to the creator or an agent",
        metadata: r#"{"params_schema":{"target":"creator|agent:<tab>"}}"#,
    },
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "respond_prompt",
        title: "Action: answer a provider-declared prompt",
        metadata: r#"{"params_schema":{"kind":"declared prompt option"}}"#,
    },
    NodeDescriptor {
        role: "action",
        kind: "handler",
        name: "stop",
        title: "Action: stop the Blueprint",
        metadata: r#"{"params_schema":{}}"#,
    },
];
pub const SOURCES_AND_MODES: &[NodeDescriptor] = &[
    NodeDescriptor {
        role: "trigger",
        kind: "trigger",
        name: "activated",
        title: "Trigger: after an earlier stage",
        metadata: r#"{"params_schema":{"route":"blueprint:<exact-id> on the earlier stage","paused":true}}"#,
    },
    NodeDescriptor {
        role: "source",
        kind: "source",
        name: "probe",
        title: "Source: run a bounded probe command",
        metadata: r#"{"params_schema":{"probe":"shell command printing one stable verdict"},"payload_schema":{"type":"text","max_bytes":32768}}"#,
    },
    NodeDescriptor {
        role: "source",
        kind: "source",
        name: "terminal",
        title: "Source: Team terminal output",
        metadata: r#"{"params_schema":{"observe":"tab:<same-Team tab>","terminal_pattern":"optional regex","output_settled":"optional idle milliseconds (10..60000)"},"payload_schema":{"type":"text","max_bytes":32768}}"#,
    },
    NodeDescriptor {
        role: "source",
        kind: "source",
        name: "observe",
        title: "Source: observed tab or cron run",
        metadata: r#"{"params_schema":{"observe":"tab:<id>|cron-run:<id>"}}"#,
    },
    NodeDescriptor {
        role: "source",
        kind: "source",
        name: "text_input",
        title: "Source: graph text input",
        metadata: r#"{"params_schema":{"command":"blueprint input <id> --from-pad <path>"},"payload_schema":{"type":"text","max_bytes":32768}}"#,
    },
    NodeDescriptor {
        role: "agent",
        kind: "decision",
        name: "agent",
        title: "Agent: chooses one bucket by instruction",
        metadata: r#"{"params_schema":{"question":"instruction naming every bucket and what is other","buckets":"declared string array"}}"#,
    },
    NodeDescriptor {
        role: "logic",
        kind: "decision",
        name: "pattern",
        title: "Logic: deterministic bucket patterns",
        metadata: r#"{"params_schema":{"match":"bucket=regex, checked before the agent"}}"#,
    },
    NodeDescriptor {
        role: "mode",
        kind: "mode",
        name: "session",
        title: "Mode: conversation",
        metadata: r#"{"params_schema":{"session":true,"safety_limit":16384,"rate_limit_per_minute":120}}"#,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint_pipeline::{StepKind, Trigger};
    use crate::sentinel_input::{DataRef, MediaKind};
    use std::collections::BTreeSet;

    #[test]
    fn blueprint_catalog_equals_enum_names_and_shared_golden_fixture() {
        let steps = [
            StepKind::Command {
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
                provider: "fixture".into(),
                profile: "reader".into(),
                task: "inspect".into(),
                deadline_ms: 1000,
                max_tokens: 1000,
                max_turns: 2,
            },
        ];
        assert_eq!(
            steps.iter().map(StepKind::name).collect::<BTreeSet<_>>(),
            STEPS.iter().map(|entry| entry.name).collect()
        );
        let triggers = [
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
            "../../../../tests/fixtures/blueprint-nodes.golden.json"
        ))
        .unwrap();
        assert_eq!(serde_json::json!({"nodes":nodes}), fixture);
        assert_eq!(
            steps[1].outcomes().unwrap(),
            BTreeSet::from(["ready".into(), "other".into()])
        );
    }
}
