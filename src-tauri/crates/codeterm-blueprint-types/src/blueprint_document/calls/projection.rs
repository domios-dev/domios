use super::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpandedCall {
    pub node: String,
    pub pin: CallPin,
    pub graph: Value,
    pub children: Vec<ExpandedCall>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpandedGraph {
    pub root: Value,
    pub calls: Vec<ExpandedCall>,
}
pub fn graph(closure: &FrozenClosure) -> Result<ExpandedGraph, String> {
    closure.verify()?;
    Ok(ExpandedGraph {
        root: bounded(
            &serde_json::to_value(&closure.root.document).map_err(|e| e.to_string())?,
            0,
        ),
        calls: expand(closure, &closure.root.document, &mut Vec::new())?,
    })
}
fn expand(
    closure: &FrozenClosure,
    document: &Document,
    stack: &mut Vec<CallPin>,
) -> Result<Vec<ExpandedCall>, String> {
    if stack.len() > MAX_CALL_DEPTH {
        return Err("budget_exhausted".into());
    }
    let mut result = Vec::new();
    for node in document.nodes() {
        let Some(call) = &node.call else { continue };
        let pin = call.pin.as_ref().ok_or("Missing call pin")?;
        let callee = closure.callee(pin)?;
        stack.push(pin.clone());
        let children = expand(closure, &callee.document, stack)?;
        stack.pop();
        result.push(ExpandedCall {
            node: node.id.clone(),
            pin: pin.clone(),
            graph: bounded(
                &serde_json::to_value(&callee.document).map_err(|e| e.to_string())?,
                0,
            ),
            children,
        });
    }
    Ok(result)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DependencyDiff {
    pub before: Option<CallPin>,
    pub after: Option<CallPin>,
    pub fields: Option<super::super::projection::SemanticDiff>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClosureDiff {
    pub root: super::super::projection::SemanticDiff,
    pub callees: BTreeMap<String, DependencyDiff>,
}
pub fn diff(before: &FrozenClosure, after: &FrozenClosure) -> ClosureDiff {
    let mut counts = BTreeMap::<String, usize>::new();
    for closure in [before, after] {
        let mut local = BTreeMap::<String, usize>::new();
        for callee in closure.callees.values() {
            *local.entry(callee.id.clone()).or_default() += 1;
        }
        for (id, count) in local {
            let entry = counts.entry(id).or_default();
            *entry = (*entry).max(count);
        }
    }
    let before_ids = by_id(before, &counts);
    let after_ids = by_id(after, &counts);
    let ids = before_ids
        .keys()
        .chain(after_ids.keys())
        .collect::<BTreeSet<_>>();
    let mut callees = BTreeMap::new();
    for id in ids {
        let a = before_ids.get(id);
        let b = after_ids.get(id);
        if a.map(|c| &c.hash) == b.map(|c| &c.hash) {
            continue;
        }
        callees.insert(
            id.clone(),
            DependencyDiff {
                before: a.map(|c| c.pin()),
                after: b.map(|c| c.pin()),
                fields: a
                    .zip(b)
                    .map(|(a, b)| super::super::projection::diff(&a.document, &b.document)),
            },
        );
    }
    ClosureDiff {
        root: super::super::projection::diff(&before.root.document, &after.root.document),
        callees,
    }
}
fn by_id<'a>(
    closure: &'a FrozenClosure,
    counts: &BTreeMap<String, usize>,
) -> BTreeMap<String, &'a FrozenCallee> {
    closure
        .callees
        .values()
        .map(|c| {
            (
                if counts.get(&c.id).copied().unwrap_or(0) > 1 {
                    c.pin().key()
                } else {
                    c.id.clone()
                },
                c,
            )
        })
        .collect()
}
pub fn bounded(value: &Value, depth: usize) -> Value {
    if depth > 24 {
        return json!({"redacted":"depth_bound"});
    }
    match value {
        Value::String(text) if crate::blueprint_memory::contains_credentials(text) => {
            json!("[redacted]")
        }
        Value::String(text) if text.chars().count() > 4096 => json!(format!(
            "{} [truncated]",
            text.chars().take(4096).collect::<String>()
        )),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .take(128)
                .map(|(key, value)| (key.clone(), bounded(value, depth + 1)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(128)
                .map(|v| bounded(v, depth + 1))
                .collect(),
        ),
        value => value.clone(),
    }
}
