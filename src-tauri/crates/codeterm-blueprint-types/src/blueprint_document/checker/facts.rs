use super::super::{projection::PositionedIr, Document, Kind, Node, Port, ValueForm};
use super::engine::Database;
use super::{ports, slots, CheckContext, DuplicateStatus};
use serde_json::Value;

pub(super) fn database(ir: &PositionedIr, context: &CheckContext) -> Database {
    let doc = &ir.canonical.document;
    let parsed = super::super::Parsed {
        document: doc.clone(),
        locations: ir.locations.clone(),
    };
    let mut db = Database::default();
    let mut add = |predicate, args, field: &str| db.edb(predicate, args, field, parsed.at(field));
    for k in [
        "notify",
        "send",
        "cron",
        "blueprint",
        "respond_prompt",
        "escalate",
        "stop",
    ] {
        add("sink_kind", vec![k.into()], "");
    }
    for k in ["code", "convert"] {
        add("pure_kind", vec![k.into()], "");
    }
    for k in [
        "agent",
        "ask",
        "http",
        "github",
        "command",
        "probe",
        "observe",
        "terminal",
        "input",
        "text_input",
    ] {
        add("taint_kind", vec![k.into()], "");
    }
    for media in ["text", "file", "audio", "video", "image"] {
        add("media_kind", vec![media.into()], "");
    }
    for name in doc.inputs.keys() {
        add("input", vec![name.clone()], &format!("inputs.{name}"));
    }
    for cap in &context.grants {
        if cap != "domios-cli" {
            add("granted", vec![cap.clone()], "owner");
        }
    }
    if doc.kind == Kind::Watch {
        add("watch_graph", vec![], "kind");
    }
    if let Some(first) = doc.steps.first() {
        add("entry", vec![first.id.clone()], "steps[0].id");
    }
    let nodes = doc.nodes().collect::<Vec<_>>();
    let mut expanded_steps = nodes.len();
    if nodes.len() > 64 {
        add("step_count", vec![nodes.len().to_string()], "steps");
        return db;
    }
    for (i, n) in nodes.iter().enumerate() {
        let path = slots::node_path(doc, i);
        add(
            "node",
            vec![n.id.clone(), n.kind().into(), i.to_string()],
            &format!("{path}.id"),
        );
        if let Some(next) = nodes.get(i + 1) {
            add(
                "next",
                vec![n.id.clone(), next.id.clone()],
                &format!("{path}.id"),
            );
        }
        if doc.kind == Kind::Pipeline || i == 0 {
            add("falls_through", vec![n.id.clone()], &path);
        }
        for o in outcomes(doc, n, i) {
            add(
                "outcome",
                vec![n.id.clone(), o],
                &format!("{path}.outcomes"),
            );
        }
        for (outcome, target) in &n.routes {
            let target = match target.as_str() {
                "@next" | "next" => nodes.get(i + 1).map_or("@stop", |n| n.id.as_str()),
                "stop" => "@stop",
                value => value,
            };
            add(
                "route",
                vec![n.id.clone(), outcome.clone(), target.into()],
                &format!("{path}.routes.{outcome}"),
            );
        }
        for bucket in &n.buckets {
            add(
                "bucket",
                vec![n.id.clone(), bucket.clone()],
                &format!("{path}.buckets"),
            );
        }
        add(
            "bucket_count",
            vec![n.id.clone(), n.buckets.len().to_string()],
            &format!("{path}.buckets"),
        );
        for (direction, port) in [("in", &n.input), ("out", &n.output)] {
            let field = format!("{path}.{direction}");
            if let Some(port) = port {
                if let Ok(resolved) = ports::resolve(port, &doc.types) {
                    add(
                        "port",
                        vec![n.id.clone(), direction.into(), json(&resolved)],
                        &field,
                    );
                    if let Some(media) = &resolved.media {
                        add(
                            "port_media",
                            vec![n.id.clone(), direction.into(), media.clone()],
                            &field,
                        );
                    }
                    if direction == "out" && resolved.media.is_none() {
                        add("typed_out", vec![n.id.clone()], &field);
                    }
                }
            } else if direction == "out" {
                let implicit = output(n);
                add(
                    "port",
                    vec![n.id.clone(), direction.into(), json(&implicit)],
                    &path,
                );
            }
        }
        if n.kind() == "command" {
            if let Some(timeout) = &n.timeout {
                let field = format!("{path}.timeout");
                if let Some(value) = timeout.value() {
                    add(
                        "command_timeout",
                        vec![n.id.clone(), value.to_string()],
                        &field,
                    );
                    if slots::duration_ms(timeout).is_some_and(|ms| {
                        (1..=crate::blueprint_pipeline::MAX_COMMAND_STEP_RUNTIME_MS).contains(&ms)
                    }) {
                        add("valid_command_timeout", vec![n.id.clone()], &field);
                    }
                }
            }
        }
        for (slot, form) in slots::forms(n) {
            let field = format!("{path}.{slot}");
            let expected = slots::expected(n.kind(), &slot, Some(n));
            if let Some(wanted) = &expected {
                add(
                    "accepted_port",
                    vec![n.id.clone(), slot.clone(), json(wanted)],
                    &field,
                );
            }
            if let ValueForm::Bind { bind } = form {
                binding(&mut add, n, &slot, bind, &field, doc, expected.as_ref());
            }
            if matches!(slot.as_str(), "every" | "timeout")
                && !(n.kind() == "command" && slot == "timeout")
            {
                if let Some(ms) = slots::duration_ms(form) {
                    add(
                        "duration",
                        vec![n.id.clone(), slot.clone(), ms.to_string()],
                        &field,
                    );
                }
            }
            if executable(n, &slot) {
                if let ValueForm::Lit { lit } = form {
                    scan(&mut add, n, &field, lit);
                }
                if let ValueForm::Input { input } = form {
                    if let Some(value) = doc.inputs.get(input).and_then(|p| p.default.as_ref()) {
                        scan(&mut add, n, &field, value);
                    }
                }
            }
        }
        for (slot, bind) in &n.bind {
            let field = format!("{path}.bind.{slot}");
            let expected = if slot == "in" {
                n.input
                    .as_ref()
                    .and_then(|p| ports::resolve(p, &doc.types).ok())
            } else {
                n.input
                    .as_ref()
                    .and_then(|p| ports::resolve(p, &doc.types).ok())
                    .and_then(|p| p.properties.get(slot).cloned())
            };
            if let Some(wanted) = &expected {
                add(
                    "accepted_port",
                    vec![n.id.clone(), slot.clone(), json(wanted)],
                    &field,
                );
            }
            binding(&mut add, n, slot, bind, &field, doc, expected.as_ref());
            if matches!(n.kind(), "command" | "code") {
                add("data_slot", vec![n.id.clone(), slot.clone()], &field);
                if super::super::bindings::named(n) {
                    let call_media = super::super::bindings::parse(bind)
                        .ok()
                        .and_then(|binding| context.call_output_media.get(binding.node))
                        .is_some_and(|media| {
                            super::super::bindings::unsupported_media(Some(media))
                        });
                    let schema = if call_media {
                        Err(super::super::bindings::NAMED_MEDIA_UNSUPPORTED.into())
                    } else {
                        super::super::bindings::source_schema(doc, bind)
                    };
                    if let Err(why) = schema {
                        add(
                            if why == super::super::bindings::NAMED_MEDIA_UNSUPPORTED {
                                "named_bind_media"
                            } else {
                                "named_port_conflict"
                            },
                            vec![n.id.clone(), slot.clone(), why],
                            &field,
                        );
                    }
                }
            }
        }
        if super::super::bindings::named(n) {
            if let (Ok(actual), Some(wanted)) = (
                super::super::bindings::envelope_schema(doc, n),
                n.input
                    .as_ref()
                    .and_then(|p| ports::resolve(p, &doc.types).ok()),
            ) {
                for why in ports::subsumption_conflicts(&actual, &wanted) {
                    add(
                        "named_port_conflict",
                        vec![n.id.clone(), "in".into(), why],
                        &format!("{path}.in"),
                    );
                }
            }
        }
        if n.kind() == "notify" {
            add("data_slot", vec![n.id.clone(), "with.text".into()], &path);
        }
        if n.kind() == "send" {
            add(
                "data_slot",
                vec![n.id.clone(), "with.payload".into()],
                &path,
            );
        }
        if matches!(n.kind(), "command" | "code" | "ask" | "agent" | "call") {
            add("data_slot", vec![n.id.clone(), "in".into()], &path);
        }
        for cap in n.caps.iter().chain(n.code.iter().flat_map(|c| &c.caps)) {
            add(
                "cap",
                vec![n.id.clone(), cap.clone()],
                &format!("{path}.caps"),
            );
        }
        if let Some(code) = &n.code {
            if let Some(src) = &code.src {
                scan(
                    &mut add,
                    n,
                    &format!("{path}.code.src"),
                    &Value::String(src.clone()),
                );
            }
        }
        if let Some(call) = &n.call {
            add(
                "call",
                vec![n.id.clone(), call.reference.clone()],
                &format!("{path}.call"),
            );
            if let Some(verified) = context.calls.get(&n.id) {
                add("call_verified", vec![n.id.clone()], &format!("{path}.call"));
                expanded_steps = expanded_steps.saturating_add(verified.expanded_steps);
                if verified.result_tainted {
                    add(
                        "call_result_tainted",
                        vec![n.id.clone()],
                        &format!("{path}.call"),
                    );
                }
                for name in call.arguments.keys() {
                    let slot = format!("call.with.{name}");
                    add(
                        "data_slot",
                        vec![n.id.clone(), slot.clone()],
                        &format!("{path}.{slot}"),
                    );
                    if let Some(where_used) = verified.fixed_arguments.get(name) {
                        add(
                            "call_arg_fixed_use",
                            vec![n.id.clone(), slot, where_used.clone()],
                            &format!("{path}.call.with.{name}"),
                        );
                    }
                }
            } else if context
                .call_diagnostics
                .iter()
                .any(|d| d.severity == "error" && d.field.starts_with(&format!("{path}.call")))
            {
                add("call_failed", vec![n.id.clone()], &format!("{path}.call"));
            }
        }
    }
    for (index, receiver) in nodes.iter().enumerate() {
        if !receiver.bind.is_empty() {
            continue;
        }
        let Some(target) = receiver
            .input
            .as_ref()
            .and_then(|p| ports::resolve(p, &doc.types).ok())
        else {
            continue;
        };
        for source in nodes.iter().take(index) {
            let actual = source.output.clone().unwrap_or_else(|| output(source));
            if let Ok(mut actual) = ports::resolve(&actual, &doc.types) {
                let mut target = target.clone();
                actual.media = None;
                target.media = None;
                for why in ports::subsumption_conflicts(&actual, &target) {
                    add(
                        "flow_port_conflict",
                        vec![receiver.id.clone(), source.id.clone(), why],
                        &format!("{}.in", slots::node_path(doc, index)),
                    );
                }
            }
        }
    }
    for use_ in slots::input_uses(doc) {
        let node = nodes
            .iter()
            .enumerate()
            .find(|(i, _)| {
                use_.field
                    .starts_with(&format!("{}.", slots::node_path(doc, *i)))
            })
            .map(|(_, n)| n.id.clone())
            .unwrap_or("document".into());
        add(
            "input_use",
            vec![node, use_.field.clone(), use_.name],
            &use_.field,
        );
    }
    if let Some(trigger) = &doc.trigger {
        for (slot, form) in slots::trigger_forms(trigger) {
            if slot == "every" {
                if let Some(ms) = slots::duration_ms(form) {
                    add(
                        "duration",
                        vec!["trigger".into(), slot, ms.to_string()],
                        "trigger.every",
                    );
                }
            }
        }
    }
    if let Some(from) = &doc.result.from {
        let (source, field) = split_bind(from);
        add(
            "result_from",
            vec![source.into(), field.into()],
            "result.from",
        );
    }
    add("step_count", vec![expanded_steps.to_string()], "steps");
    for verdict in ["exact", "structural_twin"] {
        add("refusing_verdict", vec![verdict.into()], "");
    }
    for verdict in ["near_twin", "semantic_twin"] {
        add("warning_verdict", vec![verdict.into()], "");
    }
    if let DuplicateStatus::Available(twins)
    | DuplicateStatus::Partial {
        findings: twins, ..
    } = &context.duplicates
    {
        for twin in twins {
            add(
                "twin",
                vec![twin.verdict.clone(), twin.other.clone(), twin.why.clone()],
                "",
            );
        }
    }
    db
}
fn json(port: &Port) -> String {
    serde_json::to_string(port).unwrap_or_default()
}
fn binding(
    add: &mut impl FnMut(&'static str, Vec<String>, &str),
    node: &Node,
    slot: &str,
    bind: &str,
    field: &str,
    doc: &Document,
    wanted: Option<&Port>,
) {
    let (source, selection) = split_bind(bind);
    add(
        "bind",
        vec![
            node.id.clone(),
            slot.into(),
            source.into(),
            selection.into(),
        ],
        field,
    );
    let Some(wanted) = wanted else {
        return;
    };
    let Some(source_node) = doc.nodes().find(|n| n.id == source) else {
        return;
    };
    let actual = source_node
        .output
        .clone()
        .unwrap_or_else(|| output(source_node));
    let Ok(actual) = ports::resolve(&actual, &doc.types) else {
        return;
    };
    let conflicts = match ports::field(&actual, selection) {
        Some(actual) => ports::subsumption_conflicts(actual, wanted),
        None => vec![format!(
            "{selection} is not a required field of {source}.out"
        )],
    };
    for why in conflicts {
        add(
            "port_conflict",
            vec![
                node.id.clone(),
                slot.into(),
                source.into(),
                selection.into(),
                why,
            ],
            field,
        );
    }
}
pub(super) fn split_bind(bind: &str) -> (&str, &str) {
    super::super::bindings::parse(bind)
        .map(|binding| (binding.node, binding.selection))
        .unwrap_or((bind, ""))
}
fn output(node: &Node) -> Port {
    if matches!(node.kind(), "code" | "call") {
        Port::default()
    } else {
        Port {
            kind: "string".into(),
            ..Default::default()
        }
    }
}
pub(super) fn outcomes(doc: &Document, node: &Node, index: usize) -> Vec<String> {
    if !node.outcomes.is_empty() {
        return node.outcomes.clone();
    }
    if node.kind() == "ask" {
        return node
            .buckets
            .iter()
            .cloned()
            .chain(["other".into()])
            .collect();
    }
    if doc.kind == Kind::Watch && index == 0 {
        return Vec::new();
    }
    let base: &[&str] = match node.kind() {
        "http" => &["up", "down", "failed"],
        "github" => return github_outcomes(doc, node),
        "notify" | "send" | "cron" | "blueprint" | "respond_prompt" | "escalate" | "stop" => {
            if node.routes.is_empty() {
                &[]
            } else {
                &["done"]
            }
        }
        _ => &["done", "failed"],
    };
    let mut result = base.iter().map(|s| (*s).into()).collect::<Vec<_>>();
    if node.kind() == "filter" {
        if let Some(bucket) = node
            .params
            .get("bucket")
            .and_then(ValueForm::value)
            .and_then(Value::as_str)
        {
            result.push(bucket.into());
        }
    }
    result
}
fn executable(node: &Node, slot: &str) -> bool {
    slot == "run"
        || slot == "with.command"
        || (matches!(node.kind(), "agent" | "ask") && matches!(slot, "with.task" | "with.question"))
}
fn scan(
    add: &mut impl FnMut(&'static str, Vec<String>, &str),
    node: &Node,
    field: &str,
    value: &Value,
) {
    if let Some(text) = value.as_str() {
        if let Some(mention) = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-')))
            .find(|word| {
                matches!(*word, "codeterm" | "domios")
                    || word.starts_with("CODETERM_")
                    || word.starts_with("DOMIOS_")
            })
            .or_else(|| spliced_mention(field, text))
        {
            add(
                "mentions_domios",
                vec![node.id.clone(), mention.into()],
                field,
            );
        }
    }
}

// Joining adjacent literals catches "dom" + "ios" splits; this is lint, not a boundary.
fn spliced_mention(field: &str, text: &str) -> Option<&'static str> {
    if field.ends_with(".with.task") || field.ends_with(".with.question") {
        return None;
    }
    let joined: String = text
        .chars()
        .filter(|c| !(c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '+' | '\\' | ',')))
        .flat_map(char::to_lowercase)
        .collect();
    ["codeterm", "domios"]
        .into_iter()
        .find(|name| joined.contains(name))
}

fn github_outcomes(doc: &Document, node: &Node) -> Vec<String> {
    let resources = match node.params.get("resource") {
        Some(ValueForm::Lit { lit }) => vec![lit.clone()],
        Some(ValueForm::Input { input }) => doc
            .inputs
            .get(input)
            .and_then(|port| ports::resolve(port, &doc.types).ok())
            .and_then(|port| port.choices)
            .unwrap_or_else(all_github_resources),
        _ => all_github_resources(),
    };
    resources
        .into_iter()
        .filter_map(|value| {
            serde_json::from_value::<crate::blueprint_github::GithubResource>(value).ok()
        })
        .flat_map(|resource| {
            crate::blueprint_pipeline::StepKind::Github {
                probe: crate::blueprint_github::GithubProbe {
                    repo: String::new(),
                    org: None,
                    label: None,
                    timeout_ms: crate::blueprint_github::PROBE_RUNTIME_MS,
                    resource,
                    id: 0,
                },
            }
            .outcomes()
            .unwrap_or_default()
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn all_github_resources() -> Vec<Value> {
    use crate::blueprint_github::GithubResource;
    [
        GithubResource::Run,
        GithubResource::Jobs,
        GithubResource::PrChecks,
        GithubResource::Runners,
    ]
    .into_iter()
    .filter_map(|resource| serde_json::to_value(resource).ok())
    .collect()
}
