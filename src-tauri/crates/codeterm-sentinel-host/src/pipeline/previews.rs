use codeterm_blueprint_types::blueprint_execution::{ExecutionContract, ExecutionEnvelope};
use codeterm_blueprint_types::blueprint_journal::MAX_JOURNAL_EVIDENCE_BYTES;
use serde_json::{json, Map, Value};

const MAX_VALUE_BYTES: usize = 4096;

pub(super) fn inputs(
    contract: &ExecutionContract,
    input: &ExecutionEnvelope,
) -> Map<String, Value> {
    if contract.input_bindings.is_empty() {
        return Map::from_iter([("in".into(), preview("in", &input.value))]);
    }
    contract
        .input_bindings
        .keys()
        .filter_map(|name| {
            input
                .value
                .get(name)
                .map(|value| (name.clone(), preview(name, value)))
        })
        .collect()
}

pub(super) fn evidence(
    inputs: Map<String, Value>,
    output: Option<&ExecutionEnvelope>,
) -> Map<String, Value> {
    let outputs: Map<String, Value> = output
        .map(|output| Map::from_iter([("out".into(), preview("out", &output.value))]))
        .unwrap_or_default();
    let mut evidence = json!({"ports":{"inputs":inputs,"outputs":outputs}});
    loop {
        let excess = evidence
            .to_string()
            .len()
            .saturating_sub(MAX_JOURNAL_EVIDENCE_BYTES);
        if excess == 0 {
            break;
        }
        let ports = evidence["ports"].as_object_mut().expect("ports object");
        let largest = ports
            .values_mut()
            .filter_map(Value::as_object_mut)
            .flat_map(|ports| ports.values_mut())
            .max_by_key(|port| port["preview"].as_str().map_or(0, str::len))
            .expect("evidence contains ports");
        let text = largest["preview"].as_str().unwrap_or_default();
        if !text.is_empty() {
            largest["preview"] = json!(prefix(text, text.len().saturating_sub(excess)));
            largest["truncated"] = json!(true);
        } else {
            let inputs = ports["inputs"].as_object_mut().expect("input ports object");
            if let Some(name) = inputs.keys().next_back().cloned() {
                inputs.remove(&name);
            }
            ports.insert("truncated".into(), json!(true));
        }
    }
    evidence.as_object().expect("evidence object").clone()
}

pub(crate) fn preview(name: &str, value: &Value) -> Value {
    let raw = value.to_string();
    let redacted = sensitive_name(name)
        || sensitive_keys(value)
        || codeterm_blueprint_types::blueprint_memory::contains_credentials(&raw)
        || codeterm_util::redact::text(&raw) != raw;
    let text = if redacted { "[redacted]" } else { &raw };
    let bounded = prefix(text, MAX_VALUE_BYTES);
    json!({
        "preview":bounded,
        "type":match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        },
        "bytes":raw.len(),
        "truncated":bounded.len() < text.len(),
        "redacted":redacted,
    })
}

fn sensitive_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("key")
        || codeterm_blueprint_types::blueprint_memory::contains_credentials(&format!(
            "{name}=withheld"
        ))
        || codeterm_util::redact::text(&format!("{name}=withheld")) != format!("{name}=withheld")
}

fn sensitive_keys(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| sensitive_name(key) || sensitive_keys(value)),
        Value::Array(values) => values.iter().any(sensitive_keys),
        _ => false,
    }
}

fn prefix(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests;
