use super::super::{Document, Port, ValueForm};
use std::collections::BTreeMap;

pub fn expand(port: &Port, types: &BTreeMap<String, Port>) -> Option<Port> {
    fn walk(port: &Port, types: &BTreeMap<String, Port>, depth: usize) -> Option<Port> {
        if depth > 32 {
            return None;
        }
        if let Some(reference) = &port.reference {
            return walk(types.get(reference)?, types, depth + 1);
        }
        let mut port = port.clone();
        for child in port.properties.values_mut() {
            *child = walk(child, types, depth + 1)?;
        }
        if let Some(child) = &mut port.items {
            **child = walk(child, types, depth + 1)?;
        }
        Some(port)
    }
    walk(port, types, 0)
}
pub fn compatible(actual: &Port, expected: &Port) -> bool {
    if actual.reference.is_some() || expected.reference.is_some() {
        return false;
    }
    if actual.media != expected.media {
        return false;
    }
    if actual.kind != expected.kind && !(actual.kind == "integer" && expected.kind == "number") {
        return false;
    }
    if expected.choices.as_ref().is_some_and(|choices| {
        actual
            .choices
            .as_ref()
            .is_none_or(|xs| xs.iter().any(|v| !choices.contains(v)))
    }) {
        return false;
    }
    if expected
        .max_length
        .is_some_and(|max| actual.max_length.is_none_or(|n| n > max))
    {
        return false;
    }
    if expected.kind == "object" {
        if expected
            .required
            .iter()
            .any(|key| !actual.required.contains(key))
        {
            return false;
        }
        if expected.additional_properties == Some(false)
            && (actual.additional_properties != Some(false)
                || actual
                    .properties
                    .keys()
                    .any(|key| !expected.properties.contains_key(key)))
        {
            return false;
        }
        for (key, port) in &expected.properties {
            match actual.properties.get(key) {
                Some(actual) if !compatible(actual, port) => return false,
                None if expected.required.contains(key) => return false,
                _ => {}
            }
        }
    }
    if expected.kind == "array"
        && expected
            .items
            .as_ref()
            .is_some_and(|p| actual.items.as_ref().is_none_or(|a| !compatible(a, p)))
    {
        return false;
    }
    true
}
pub fn binding(document: &Document, binding: &str, before: usize) -> Option<Port> {
    let mut parts = binding.split('.');
    let id = parts.next()?;
    if parts.next()? != "out" {
        return None;
    }
    let node = document.nodes().take(before).find(|node| node.id == id)?;
    let mut port = expand(node.output.as_ref()?, &document.types)?;
    for field in parts {
        port = port.properties.get(field)?.clone();
    }
    Some(port)
}
pub fn form(document: &Document, form: &ValueForm, before: usize) -> Option<Port> {
    match form {
        ValueForm::Input { input } => expand(document.inputs.get(input)?, &document.types),
        ValueForm::Bind { bind } => binding(document, bind, before),
        ValueForm::Lit { lit } => Some(literal(lit)),
    }
}
fn literal(value: &serde_json::Value) -> Port {
    let mut port = Port {
        kind: match value {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        }
        .into(),
        ..Default::default()
    };
    port.choices = Some(vec![value.clone()]);
    if let Some(s) = value.as_str() {
        port.max_length = Some(s.chars().count());
    }
    if let Some(fields) = value.as_object() {
        port.properties = fields
            .iter()
            .map(|(k, v)| (k.clone(), literal(v)))
            .collect();
        port.required = fields.keys().cloned().collect();
        port.additional_properties = Some(false);
    }
    if let Some(values) = value.as_array() {
        if let Some(first) = values.first() {
            let item = literal(first);
            if values.iter().all(|v| {
                compatible(
                    &literal(v),
                    &Port {
                        choices: None,
                        ..item.clone()
                    },
                )
            }) {
                port.items = Some(Box::new(Port {
                    choices: None,
                    ..item
                }));
            }
        }
    }
    port
}
