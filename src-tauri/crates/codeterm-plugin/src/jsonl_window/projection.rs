use super::ordered::OrderedValue;
use indexmap::IndexMap;
use serde_json::{value::RawValue, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct Selection {
    retain: bool,
    children: BTreeMap<String, Selection>,
}

impl Selection {
    fn include(&mut self, path: &[String]) {
        if let Some((key, rest)) = path.split_first() {
            self.children.entry(key.clone()).or_default().include(rest);
        } else {
            self.retain = true;
        }
    }
}

fn select(
    raw: &RawValue,
    selection: &Selection,
    remaining: &mut usize,
) -> Result<OrderedValue, String> {
    if selection.retain {
        if raw.get().len() > *remaining {
            return Err("Stored record metadata exceeds page budget".into());
        }
        *remaining -= raw.get().len();
        return serde_json::from_str(raw.get()).map_err(|error| error.to_string());
    }
    let mut output = IndexMap::new();
    if !raw.get().trim_start().starts_with('{') {
        return Ok(OrderedValue::Object(output));
    }
    let fields: IndexMap<String, &RawValue> =
        serde_json::from_str(raw.get()).map_err(|error| error.to_string())?;
    for (key, raw) in fields {
        if let Some(child) = selection.children.get(&key) {
            output.insert(key, select(raw, child, remaining)?);
        }
    }
    Ok(OrderedValue::Object(output))
}

pub fn project(record: &str, paths: &[Vec<String>], budget: usize) -> Result<Value, String> {
    serde_json::to_value(project_ordered(record, paths, budget)?).map_err(|error| error.to_string())
}

pub(crate) fn project_ordered(
    record: &str,
    paths: &[Vec<String>],
    budget: usize,
) -> Result<OrderedValue, String> {
    let raw: &RawValue = serde_json::from_str(record).map_err(|error| error.to_string())?;
    let mut selection = Selection::default();
    for path in paths {
        selection.include(path);
    }
    let mut remaining = budget;
    let output = select(raw, &selection, &mut remaining)?;
    if serde_json::to_vec(&output)
        .map_err(|error| error.to_string())?
        .len()
        > budget
    {
        return Err("Stored record metadata exceeds page budget".into());
    }
    Ok(output)
}

pub fn retain_paths(record: &Value, paths: &[Vec<String>]) -> Value {
    let mut output = serde_json::json!({});
    for path in paths {
        if let Some(value) = path.iter().try_fold(record, |value, key| value.get(key)) {
            insert(&mut output, path, value.clone());
        }
    }
    output
}

fn insert(output: &mut Value, path: &[String], value: Value) {
    let Some((key, rest)) = path.split_first() else {
        *output = value;
        return;
    };
    if !output.is_object() {
        *output = serde_json::json!({});
    }
    insert(&mut output[key], rest, value);
}
