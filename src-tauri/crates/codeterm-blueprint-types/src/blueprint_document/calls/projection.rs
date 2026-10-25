use super::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpandedCall {
    pub node: String,
    pub pin: CallPin,
    pub graph: Value,
    pub children: Vec<ExpandedCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<UnavailableCall>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableCall {
    PinnedRevisionUnavailable,
    BudgetExhausted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpandedGraph {
    pub root: Value,
    pub calls: Vec<ExpandedCall>,
}
pub fn graph(closure: &FrozenClosure) -> Result<ExpandedGraph, String> {
    closure.verify()?;
    graph_resolved(&closure.root.document, |pin| {
        Ok((*closure.callee(pin)?.document).clone())
    })
}
pub fn graph_resolved(
    document: &Document,
    mut revision: impl FnMut(&CallPin) -> Result<Document, String>,
) -> Result<ExpandedGraph, String> {
    let mut budget = MAX_CLOSURE_BYTES;
    let mut count = 0;
    Ok(ExpandedGraph {
        root: bounded(
            &serde_json::to_value(document).map_err(|e| e.to_string())?,
            0,
        ),
        calls: expand(
            document,
            &mut revision,
            &mut Vec::new(),
            &mut budget,
            &mut count,
        )?,
    })
}
fn expand(
    document: &Document,
    revision: &mut impl FnMut(&CallPin) -> Result<Document, String>,
    stack: &mut Vec<CallPin>,
    budget: &mut usize,
    count: &mut usize,
) -> Result<Vec<ExpandedCall>, String> {
    let mut result = Vec::new();
    for node in document.nodes() {
        let Some(call) = &node.call else { continue };
        let pin = call.pin.as_ref().ok_or("Missing call pin")?;
        if *count >= 64 {
            break;
        }
        *count += 1;
        let callee = if stack.len() >= MAX_CALL_DEPTH {
            Err(UnavailableCall::BudgetExhausted)
        } else if stack.iter().any(|prior| prior.id == pin.id) {
            Err(UnavailableCall::PinnedRevisionUnavailable)
        } else {
            revision(pin).map_err(|_| UnavailableCall::PinnedRevisionUnavailable)
        };
        let callee = callee.and_then(|mut callee| {
            callee.dependencies.clear();
            let bytes = serde_json::to_vec(&callee)
                .map_err(|_| UnavailableCall::PinnedRevisionUnavailable)?;
            if bytes.len() > *budget {
                return Err(UnavailableCall::BudgetExhausted);
            }
            *budget -= bytes.len();
            Ok(callee)
        });
        let (graph, children, unavailable) = match callee {
            Ok(callee) => {
                stack.push(pin.clone());
                let children = expand(&callee, revision, stack, budget, count)?;
                stack.pop();
                (
                    bounded(
                        &serde_json::to_value(&callee).map_err(|e| e.to_string())?,
                        0,
                    ),
                    children,
                    None,
                )
            }
            Err(reason) => (Value::Null, Vec::new(), Some(reason)),
        };
        result.push(ExpandedCall {
            node: node.id.clone(),
            pin: pin.clone(),
            graph,
            children,
            unavailable,
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
