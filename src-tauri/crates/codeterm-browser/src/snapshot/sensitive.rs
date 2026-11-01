use super::{AxNode, AxValue};
use serde_json::Value;
use std::collections::HashSet;

pub fn redact(nodes: &mut [AxNode], document: &Value) -> Result<(), String> {
    let root = document
        .get("root")
        .ok_or("redaction_failed: DOM document missing")?;
    let mut sensitive = HashSet::new();
    let mut stack = vec![(root, false)];
    while let Some((node, inherited)) = stack.pop() {
        let attributes = node.get("attributes").and_then(Value::as_array);
        let marked = inherited
            || attributes.is_some_and(|attrs| {
                attrs.chunks_exact(2).any(|pair| {
                    let key = pair[0].as_str().unwrap_or("").to_ascii_lowercase();
                    let value = pair[1].as_str().unwrap_or("").to_ascii_lowercase();
                    key == "data-sensitive"
                        || (key == "type" && value == "password")
                        || (["name", "id", "autocomplete"].contains(&key.as_str())
                            && [
                                "password",
                                "token",
                                "secret",
                                "api-key",
                                "apikey",
                                "one-time-code",
                            ]
                            .iter()
                            .any(|needle| value.contains(needle)))
                })
            });
        if marked {
            if let Some(id) = node.get("backendNodeId").and_then(Value::as_i64) {
                sensitive.insert(id);
            }
        }
        for key in ["children", "shadowRoots"] {
            if let Some(children) = node.get(key).and_then(Value::as_array) {
                stack.extend(children.iter().map(|child| (child, marked)));
            }
        }
        if let Some(child) = node.get("contentDocument") {
            stack.push((child, marked));
        }
    }
    let protected: HashSet<_> = nodes
        .iter()
        .filter(|node| {
            node.backend_dom_node_id
                .is_some_and(|id| sensitive.contains(&id))
                || node
                    .properties
                    .iter()
                    .any(|p| p.name == "protected" && p.value.value == Some(Value::Bool(true)))
        })
        .map(|node| node.node_id.clone())
        .collect();
    let mut hidden = protected;
    loop {
        let before = hidden.len();
        for node in nodes.iter() {
            if node
                .parent_id
                .as_ref()
                .is_some_and(|id| hidden.contains(id))
            {
                hidden.insert(node.node_id.clone());
            }
        }
        if hidden.len() == before {
            break;
        }
    }
    for node in nodes {
        if hidden.contains(&node.node_id) {
            node.value = None;
            node.name = Some(AxValue {
                value: Some(Value::String("REDACTED".into())),
            });
            node.properties.retain(|p| {
                [
                    "disabled", "readonly", "required", "focused", "checked", "selected",
                    "expanded", "invalid",
                ]
                .contains(&p.name.as_str())
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
