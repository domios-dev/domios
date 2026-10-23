use super::super::Port;
use serde_json::Value;
use std::collections::BTreeMap;

pub fn resolve(port: &Port, types: &BTreeMap<String, Port>) -> Result<Port, String> {
    fn walk(
        p: &Port,
        types: &BTreeMap<String, Port>,
        stack: &mut Vec<String>,
        depth: usize,
    ) -> Result<Port, String> {
        if depth > 32 {
            return Err("Port schema exceeds depth 32".into());
        }
        if let Some(name) = &p.reference {
            if stack.contains(name) {
                return Err(format!("Cyclic port type {name}"));
            }
            let target = types
                .get(name)
                .ok_or_else(|| format!("Unknown port type {name}"))?;
            stack.push(name.clone());
            let resolved = walk(target, types, stack, depth + 1);
            stack.pop();
            return resolved;
        }
        let mut result = p.clone();
        for (key, child) in &p.properties {
            result
                .properties
                .insert(key.clone(), walk(child, types, stack, depth + 1)?);
        }
        result.items = p
            .items
            .as_ref()
            .map(|p| walk(p, types, stack, depth + 1).map(Box::new))
            .transpose()?;
        Ok(result)
    }
    walk(port, types, &mut Vec::new(), 0)
}

fn kind(port: &Port) -> &str {
    if !port.kind.is_empty() {
        return &port.kind;
    }
    if !port.properties.is_empty() || !port.required.is_empty() {
        return "object";
    }
    if port.items.is_some() {
        return "array";
    }
    if let Some(choices) = &port.choices {
        if let Some(first) = choices.first() {
            let value_kind = |v: &Value| match v {
                Value::String(_) => "string",
                Value::Bool(_) => "boolean",
                Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
                Value::Number(_) => "number",
                Value::Null => "null",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            let k = value_kind(first);
            if choices.iter().all(|v| value_kind(v) == k) {
                return k;
            }
        }
    }
    ""
}

pub fn subsumption_conflicts(source: &Port, target: &Port) -> Vec<String> {
    fn walk(source: &Port, target: &Port, path: &str, depth: usize, out: &mut Vec<String>) {
        let mut add = |reason: String| out.push(format!("{path}: {reason}"));
        if depth > 32 {
            add("schema comparison exceeds depth 32".into());
            return;
        }
        if source.reference.is_some() || target.reference.is_some() {
            add("resolve schema references before comparison".into());
            return;
        }
        if let Some(choices) = &source.choices {
            if choices.iter().all(|v| target.accepts(v))
                && (target.media.is_none() || source.media == target.media)
            {
                return;
            }
        }
        let (s, t) = (kind(source), kind(target));
        if !t.is_empty() && !(s == t || s == "integer" && t == "number") {
            add(format!(
                "{} is not {t}",
                if s.is_empty() { "untyped value" } else { s }
            ));
            return;
        }
        if target.choices.as_ref().is_some_and(|allowed| {
            source
                .choices
                .as_ref()
                .is_none_or(|values| values.iter().any(|v| !allowed.contains(v)))
        }) {
            add("source enum is wider than target enum".into());
        }
        if let Some(maximum) = target.max_length {
            if source.max_length.is_none_or(|size| size > maximum) {
                add(format!("source maxLength is wider than {maximum}"));
            }
        }
        if let Some(media) = &target.media {
            if source.media.as_ref() != Some(media) {
                add(format!("source media does not guarantee {media}"));
            }
        }
        if t == "object" || s == "object" {
            for name in &target.required {
                if !source.required.contains(name) {
                    add(format!("missing required field {name}"));
                }
            }
            if target.additional_properties == Some(false) {
                if source.additional_properties != Some(false) {
                    add("source permits extra fields in a closed target".into());
                }
                for name in source.properties.keys() {
                    if !target.properties.contains_key(name) {
                        add(format!("extra field {name} is not allowed"));
                    }
                }
            }
            for (name, wanted) in &target.properties {
                if let Some(actual) = source.properties.get(name) {
                    walk(actual, wanted, &format!("{path}.{name}"), depth + 1, out);
                } else if source.additional_properties != Some(false) {
                    out.push(format!(
                        "{path}.{name}: source permits an unconstrained property"
                    ));
                }
            }
        }
        if t == "array" || s == "array" {
            if let Some(wanted) = &target.items {
                walk(
                    source.items.as_deref().unwrap_or(&Port::default()),
                    wanted,
                    &format!("{path}[]"),
                    depth + 1,
                    out,
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(source, target, "$", 0, &mut out);
    out
}

pub(super) fn field<'a>(port: &'a Port, path: &str) -> Option<&'a Port> {
    if path.is_empty() {
        return Some(port);
    }
    path.split('.').try_fold(port, |p, part| {
        if p.required.iter().any(|key| key == part) {
            p.properties.get(part)
        } else {
            None
        }
    })
}
