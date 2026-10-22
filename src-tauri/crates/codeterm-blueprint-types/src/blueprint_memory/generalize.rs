use crate::blueprint_document::{Document, Input};
use serde_json::{json, Value};
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
    if let Some(watch) = &mut document.watch {
        abstract_values(watch, "watch", &mut document.inputs);
    }
    if let Some(pipeline) = &mut document.pipeline {
        abstract_values(pipeline, "pipeline", &mut document.inputs);
    }
    document
}

fn abstract_values(value: &mut Value, path: &str, inputs: &mut BTreeMap<String, Input>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(key.as_str(), "on" | "routes" | "matches" | "note" | "name") {
                    continue;
                }
                let path = format!("{path}_{key}");
                let parameter = path.contains("_params_")
                    || (path.contains("_with_")
                        && !generic(key)
                        && !matches!(
                            key.as_str(),
                            "resource" | "from" | "to" | "expected_status" | "pattern" | "bucket"
                        ));
                let concrete = parameter
                    || matches!(
                        key.as_str(),
                        "repo"
                            | "org"
                            | "id"
                            | "url"
                            | "file"
                            | "probe"
                            | "run"
                            | "command"
                            | "task"
                            | "text"
                            | "target"
                            | "path"
                            | "label"
                    )
                    || (key == "to" && value.as_str().is_some_and(|text| text.starts_with("tab:")))
                    || value.as_str().is_some_and(unsafe_text);
                if concrete
                    && (value.is_string() || value.is_number())
                    && !value.as_str().is_some_and(|text| text.contains("{{"))
                {
                    let role = match key.as_str() {
                        "run" | "probe" => "command",
                        "text" => "message",
                        other => other,
                    };
                    let mut name = role.to_owned();
                    let mut suffix = 2;
                    while inputs.contains_key(&name) {
                        name = format!("{role}_{suffix}");
                        suffix += 1;
                    }
                    inputs.insert(
                        name.clone(),
                        Input {
                            kind: if value.is_number() {
                                "integer"
                            } else {
                                "string"
                            }
                            .into(),
                            default: None,
                            description: Some(format!("Provide {}", role.replace('_', " "))),
                            choices: None,
                        },
                    );
                    *value = json!(format!("{{{{{name}}}}}"));
                } else {
                    abstract_values(value, &path, inputs);
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                abstract_values(value, &format!("{path}_{index}"), inputs);
            }
        }
        _ => {}
    }
}

pub(super) fn merge_variants(keeper: &mut Document, lesser: &Document) {
    fn merge(a: &mut Value, b: &Value, path: &str, inputs: &mut BTreeMap<String, Input>) {
        match (a, b) {
            (Value::Object(a), Value::Object(b)) => {
                for (key, value) in a {
                    let Some(other) = b.get(key) else { continue };
                    let path = format!("{path}_{key}");
                    let variant = generic(key)
                        || matches!(
                            key.as_str(),
                            "resource" | "expected_status" | "pattern" | "question"
                        );
                    if variant
                        && value != other
                        && (value.is_string() || value.is_number())
                        && (other.is_string() || other.is_number())
                        && !value.as_str().is_some_and(|text| text.contains("{{"))
                        && !other.as_str().is_some_and(|text| text.contains("{{"))
                    {
                        let role = match key.as_str() {
                            "run" | "probe" => "command",
                            "text" => "message",
                            other => other,
                        };
                        let mut name = role.to_owned();
                        let mut suffix = 2;
                        while inputs.contains_key(&name) {
                            name = format!("{role}_{suffix}");
                            suffix += 1;
                        }
                        inputs.insert(
                            name.clone(),
                            Input {
                                kind: if value.is_number() {
                                    "integer"
                                } else {
                                    "string"
                                }
                                .into(),
                                default: generic(key).then(|| value.clone()),
                                description: Some(format!("Choose {}", key.replace('_', " "))),
                                choices: Some(vec![value.clone(), other.clone()]),
                            },
                        );
                        *value = json!(format!("{{{{{name}}}}}"));
                    } else {
                        merge(value, other, &path, inputs);
                    }
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                for (index, (a, b)) in a.iter_mut().zip(b).enumerate() {
                    merge(a, b, &format!("{path}_{index}"), inputs);
                }
            }
            _ => {}
        }
    }
    if let (Some(a), Some(b)) = (&mut keeper.watch, &lesser.watch) {
        merge(a, b, "watch", &mut keeper.inputs);
    }
    if let (Some(a), Some(b)) = (&mut keeper.pipeline, &lesser.pipeline) {
        merge(a, b, "pipeline", &mut keeper.inputs);
    }
}
