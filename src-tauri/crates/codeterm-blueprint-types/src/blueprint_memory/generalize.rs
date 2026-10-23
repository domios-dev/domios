use crate::blueprint_document::{Document, Input};
use serde_json::Value;
use std::collections::BTreeMap;

fn generic(key: &str) -> bool {
    matches!(
        key,
        "interval"
            | "every"
            | "timeout"
            | "timeout_ms"
            | "max_tokens"
            | "max_turns"
            | "deadline_ms"
            | "max_transitions"
            | "max_actions"
            | "escalation_timeout_ms"
    )
}

fn unsafe_text(text: &str) -> bool {
    static PATH: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let secret = super::contains_credentials(text);
    let path = text.contains("/Users/")
        || text.contains("/home/")
        || PATH
            .get_or_init(|| regex::Regex::new(r"(?i)([a-z]:[\\/]|\\\\)").ok())
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(text))
        || text.split_whitespace().any(|word| word.starts_with('/'));
    secret || path
}

pub fn safe_template(document: &Document, when_to_use: &str) -> bool {
    fn safe(value: &Value) -> bool {
        match value {
            Value::String(text) => !unsafe_text(text),
            Value::Array(values) => values.iter().all(safe),
            Value::Object(fields) => fields
                .iter()
                .all(|(key, value)| !unsafe_text(key) && safe(value)),
            _ => true,
        }
    }
    !unsafe_text(when_to_use) && serde_json::to_value(document).is_ok_and(|value| safe(&value))
}

pub fn abstract_document(mut document: Document) -> Document {
    for (key, input) in &mut document.inputs {
        if !generic(key) {
            input.default = None;
            input.choices = None;
        }
        input.description = Some(format!("Provide {}", key.replace('_', " ")));
    }
    let mut inputs = std::mem::take(&mut document.inputs);
    for node in document.steps.iter_mut().chain(&mut document.other) {
        for (key, value) in &mut node.params {
            abstract_value(key, value, &mut inputs);
        }
        if let Some(run) = &mut node.run {
            abstract_value("command", run, &mut inputs);
        }
        if let Some(call) = &mut node.call {
            for (key, value) in &mut call.arguments {
                abstract_value(key, value, &mut inputs);
            }
        }
    }
    if let Some(crate::blueprint_document::Trigger::Input { source, .. }) = &mut document.trigger {
        abstract_value("source", source, &mut inputs);
    }
    if document
        .result
        .to
        .as_ref()
        .and_then(crate::blueprint_document::ResultTarget::fixed)
        .is_some_and(|to| to.starts_with("tab:"))
    {
        let key = "result_target".to_owned();
        inputs.insert(
            key.clone(),
            Input {
                kind: "string".into(),
                description: Some("Provide result destination".into()),
                ..Default::default()
            },
        );
        document.result.to = Some(crate::blueprint_document::ResultTarget::Input { input: key });
    }
    document.inputs = inputs;
    document
}
pub fn portable_document(document: Document) -> Document {
    let mut document = abstract_document(document);
    for node in document.steps.iter_mut().chain(&mut document.other) {
        for key in ["question", "pattern"] {
            if let Some(value) = node.params.get_mut(key) {
                abstract_parameter(key, value, &mut document.inputs);
            }
        }
        for value in node.matches.values_mut() {
            abstract_parameter("pattern", value, &mut document.inputs);
        }
    }
    document
}
fn abstract_value(
    key: &str,
    value: &mut crate::blueprint_document::ValueForm,
    inputs: &mut BTreeMap<String, Input>,
) {
    use crate::blueprint_document::ValueForm;
    let ValueForm::Lit { lit } = value else {
        return;
    };
    let concrete = matches!(
        key,
        "repo"
            | "org"
            | "id"
            | "url"
            | "file"
            | "command"
            | "task"
            | "text"
            | "payload"
            | "target"
            | "path"
            | "label"
            | "source"
    ) || lit.as_str().is_some_and(unsafe_text);
    if !concrete {
        return;
    }
    abstract_parameter(key, value, inputs);
}
fn abstract_parameter(
    key: &str,
    value: &mut crate::blueprint_document::ValueForm,
    inputs: &mut BTreeMap<String, Input>,
) {
    let Some(lit) = value.value() else {
        return;
    };
    let kind = if lit.is_u64() || lit.is_i64() {
        "integer"
    } else if lit.is_number() {
        "number"
    } else if lit.is_object() {
        "object"
    } else if lit.is_array() {
        "array"
    } else if lit.is_boolean() {
        "boolean"
    } else {
        "string"
    };
    let mut name = key.to_owned();
    let mut suffix = 2;
    while inputs.contains_key(&name) {
        name = format!("{key}_{suffix}");
        suffix += 1;
    }
    inputs.insert(
        name.clone(),
        Input {
            kind: kind.into(),
            description: Some(format!("Provide {}", key.replace('_', " "))),
            ..Default::default()
        },
    );
    *value = crate::blueprint_document::ValueForm::input(name);
}
pub(super) fn merge_variants(keeper: &mut Document, lesser: &Document) {
    use crate::blueprint_document::ValueForm;
    for (a, b) in keeper
        .steps
        .iter_mut()
        .chain(&mut keeper.other)
        .zip(lesser.nodes())
    {
        for (key, av) in &mut a.params {
            let Some(ValueForm::Lit { lit: bv }) = b.params.get(key) else {
                continue;
            };
            let ValueForm::Lit { lit: avalue } = av else {
                continue;
            };
            if avalue == bv
                || !(generic(key)
                    || matches!(
                        key.as_str(),
                        "resource" | "expected_status" | "pattern" | "question"
                    ))
            {
                continue;
            }
            let mut name = key.clone();
            let mut suffix = 2;
            while keeper.inputs.contains_key(&name) {
                name = format!("{key}_{suffix}");
                suffix += 1;
            }
            keeper.inputs.insert(
                name.clone(),
                Input {
                    kind: if avalue.is_number() {
                        "number"
                    } else {
                        "string"
                    }
                    .into(),
                    default: generic(key).then(|| avalue.clone()),
                    choices: Some(vec![avalue.clone(), bv.clone()]),
                    description: Some(format!("Choose {}", key.replace('_', " "))),
                    ..Default::default()
                },
            );
            *av = ValueForm::input(name);
        }
    }
}
