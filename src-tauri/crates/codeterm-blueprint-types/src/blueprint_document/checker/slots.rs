use super::super::{Document, Node, Port, ResultTarget, Trigger, ValueForm};

pub(super) fn expected(kind: &str, slot: &str, node: Option<&Node>) -> Option<Port> {
    let kind = match (kind, slot) {
        (_, "run" | "every" | "timeout") => "string",
        (_, "in") => return node.and_then(|n| n.input.clone()),
        ("notify" | "input", "with.text")
        | ("command" | "probe", "with.command")
        | ("ask", "with.question")
        | ("send" | "cron" | "blueprint" | "escalate" | "respond_prompt", "with.target")
        | ("observe" | "terminal", "with.id")
        | ("cron", "with.name")
        | ("command" | "github", "with.label")
        | ("filter" | "terminal", "with.pattern")
        | ("github", "with.repo" | "with.org" | "with.resource")
        | ("http" | "input", "with.url")
        | ("agent", "with.provider" | "with.profile" | "with.task")
        | ("observe" | "terminal" | "input", "with.kind")
        | ("input", "with.file" | "with.convert_to")
        | ("convert", "with.from" | "with.to")
        | ("filter", "with.bucket")
        | ("http", "with.expected_status") => "string",
        ("github", "with.id")
        | ("command" | "http" | "github", "with.timeout_ms")
        | ("agent", "with.deadline_ms" | "with.max_tokens" | "with.max_turns")
        | ("terminal", "with.idle_ms")
        | ("input", "with.convert_timeout_ms") => "integer",
        ("ask", _) if slot.starts_with("matches.") => "string",
        _ => return None,
    };
    Some(Port {
        kind: kind.into(),
        ..Default::default()
    })
}

pub(super) fn forms(node: &Node) -> Vec<(String, &ValueForm)> {
    node.params
        .iter()
        .map(|(k, v)| (format!("with.{k}"), v))
        .chain(
            node.matches
                .iter()
                .map(|(k, v)| (format!("matches.{k}"), v)),
        )
        .chain(node.run.iter().map(|v| ("run".into(), v)))
        .chain(node.every.iter().map(|v| ("every".into(), v)))
        .chain(node.timeout.iter().map(|v| ("timeout".into(), v)))
        .chain(node.call.iter().flat_map(|c| {
            c.arguments
                .iter()
                .map(|(k, v)| (format!("call.with.{k}"), v))
        }))
        .collect()
}
pub(super) fn trigger_forms(trigger: &Trigger) -> Vec<(String, &ValueForm)> {
    match trigger {
        Trigger::Webhook { .. } => vec![],
        Trigger::Schedule { every } => vec![("every".into(), every)],
        Trigger::Event { kind, params } => std::iter::once(("kind".into(), kind))
            .chain(params.iter().map(|(k, v)| (format!("params.{k}"), v)))
            .collect(),
        Trigger::Input { kind, source } => vec![("kind".into(), kind), ("source".into(), source)],
    }
}
pub(super) struct InputUse {
    pub name: String,
    pub field: String,
    pub expected: Option<Port>,
}
pub(super) fn input_uses(doc: &Document) -> Vec<InputUse> {
    let mut uses = Vec::new();
    for (i, n) in doc.nodes().enumerate() {
        let path = node_path(doc, i);
        for (slot, form) in forms(n) {
            if let ValueForm::Input { input } = form {
                uses.push(InputUse {
                    name: input.clone(),
                    field: format!("{path}.{slot}"),
                    expected: expected(n.kind(), &slot, Some(n)),
                });
            }
        }
    }
    if let Some(trigger) = &doc.trigger {
        for (slot, form) in trigger_forms(trigger) {
            if let ValueForm::Input { input } = form {
                uses.push(InputUse {
                    name: input.clone(),
                    field: format!("trigger.{slot}"),
                    expected: if slot == "every" || slot == "kind" {
                        Some(Port {
                            kind: "string".into(),
                            ..Default::default()
                        })
                    } else {
                        None
                    },
                });
            }
        }
    }
    if let Some(name) = super::super::whole_input(&doc.name) {
        uses.push(InputUse {
            name: name.into(),
            field: "name".into(),
            expected: Some(Port {
                kind: "string".into(),
                ..Default::default()
            }),
        });
    }
    if let Some(ResultTarget::Input { input }) = &doc.result.to {
        uses.push(InputUse {
            name: input.clone(),
            field: "result.to".into(),
            expected: Some(Port {
                kind: "string".into(),
                ..Default::default()
            }),
        });
    }
    uses
}
pub(super) fn node_path(doc: &Document, index: usize) -> String {
    if index < doc.steps.len() {
        format!("steps[{index}]")
    } else {
        format!("other[{}]", index - doc.steps.len())
    }
}
pub(super) fn duration_ms(form: &ValueForm) -> Option<u64> {
    let raw = form.value()?.as_str()?;
    if !raw.is_empty()
        && raw.split_inclusive(['s', 'm', 'h', 'd']).all(|part| {
            part.len() > 1
                && part.ends_with(['s', 'm', 'h', 'd'])
                && part[..part.len() - 1].bytes().all(|b| b == b'0')
        })
    {
        return Some(0);
    }
    crate::blueprint_watch::duration_ms(raw).ok()
}
