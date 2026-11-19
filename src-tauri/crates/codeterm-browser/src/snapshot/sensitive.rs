use super::{AxNode, AxValue};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use url::Url;

fn marked(node: &Value) -> bool {
    node.get("attributes")
        .and_then(Value::as_array)
        .is_some_and(|attrs| {
            attrs.as_chunks::<2>().0.iter().any(|pair| {
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
        })
}

pub fn redact(nodes: &mut [AxNode], document: &Value) -> Result<(), String> {
    let root = document
        .get("root")
        .ok_or("redaction_failed: DOM document missing")?;
    let mut sensitive = HashSet::new();
    let mut hrefs = HashMap::new();
    let mut stack = vec![(root, false, None)];
    while let Some((node, inherited, inherited_base)) = stack.pop() {
        let marked = inherited || marked(node);
        let base = node["baseURL"].as_str().or(inherited_base);
        if marked {
            if let Some(id) = node.get("backendNodeId").and_then(Value::as_i64) {
                sensitive.insert(id);
            }
        }
        if let Some(id) = node["backendNodeId"].as_i64() {
            let href = node["attributes"].as_array().and_then(|attrs| {
                attrs
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .find(|pair| pair[0] == "href")?[1]
                    .as_str()
            });
            if let Some(url) = href
                .and_then(|href| {
                    Url::parse(href)
                        .ok()
                        .or_else(|| Url::parse(base?).ok()?.join(href).ok())
                })
                .filter(|url| {
                    matches!(url.scheme(), "http" | "https")
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.as_str().len() <= 2048
                })
            {
                hrefs.insert(id, url.to_string());
            }
        }
        for key in ["children", "shadowRoots"] {
            if let Some(children) = node.get(key).and_then(Value::as_array) {
                stack.extend(children.iter().map(|child| (child, marked, base)));
            }
        }
        if let Some(child) = node.get("contentDocument") {
            stack.push((child, marked, base));
        }
    }
    for node in nodes.iter_mut() {
        if node
            .role
            .as_ref()
            .is_some_and(|role| role.as_str() == "link")
        {
            node.href = node
                .backend_dom_node_id
                .and_then(|id| hrefs.get(&id))
                .cloned();
        }
    }
    redact_ids(nodes, &sensitive);
    Ok(())
}

fn redact_ids(nodes: &mut [AxNode], sensitive: &HashSet<i64>) {
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
            redact_node(node);
        }
    }
}

fn redact_node(node: &mut AxNode) {
    node.value = None;
    node.href = None;
    node.name = Some(AxValue {
        value: Some(Value::String("REDACTED".into())),
    });
    node.properties.retain(|p| {
        [
            "disabled", "readonly", "required", "focused", "checked", "selected", "expanded",
            "invalid",
        ]
        .contains(&p.name.as_str())
    });
}

#[cfg(test)]
mod tests;
