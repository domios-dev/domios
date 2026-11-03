use super::{AxNode, AxValue};
use crate::{EventStream, PageChannel};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Dom {
    nodes: HashMap<i64, DomNode>,
    uncertain: bool,
}

struct DomNode {
    parent: Option<i64>,
    backend: Option<i64>,
    marked: bool,
}

impl Dom {
    fn record(&mut self, root: &Value, parent: Option<i64>) -> Result<(), String> {
        let mut stack = vec![(root, parent)];
        while let Some((node, parent)) = stack.pop() {
            let id = node
                .get("nodeId")
                .and_then(Value::as_i64)
                .ok_or("redaction_failed: DOM node id missing")?;
            let parent = parent.or_else(|| self.nodes.get(&id).and_then(|node| node.parent));
            let marked = marked(node) || self.nodes.get(&id).is_some_and(|node| node.marked);
            self.nodes.insert(
                id,
                DomNode {
                    parent,
                    backend: node.get("backendNodeId").and_then(Value::as_i64),
                    marked,
                },
            );
            for key in ["children", "shadowRoots"] {
                if let Some(children) = node.get(key).and_then(Value::as_array) {
                    stack.extend(children.iter().map(|child| (child, Some(id))));
                }
            }
            if let Some(child) = node.get("contentDocument") {
                stack.push((child, Some(id)));
            }
        }
        Ok(())
    }

    fn event(&mut self, event: &crate::cdp_wire::CdpEvent) -> Result<(), String> {
        match event.method.as_str() {
            "DOM.attributeModified" => {
                if marked(&json!({"attributes":[event.params["name"],event.params["value"]]})) {
                    self.mark_node(&event.params["nodeId"]);
                }
                return Ok(());
            }
            "DOM.attributeRemoved" => return Ok(()),
            "DOM.childNodeRemoved" => {
                self.mark_node(&event.params["nodeId"]);
                return Ok(());
            }
            "DOM.shadowRootPopped" => {
                self.mark_node(&event.params["rootId"]);
                return Ok(());
            }
            "DOM.childNodeInserted" => {
                return self.record(&event.params["node"], event.params["parentNodeId"].as_i64());
            }
            "DOM.shadowRootPushed" => {
                return self.record(&event.params["root"], event.params["hostId"].as_i64());
            }
            "DOM.setChildNodes" => {}
            _ => return Err("redaction_failed: DOM changed during snapshot".into()),
        }
        let parent = event
            .params
            .get("parentId")
            .and_then(Value::as_i64)
            .ok_or("redaction_failed: DOM parent id missing")?;
        let nodes = event
            .params
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or("redaction_failed: DOM children missing")?;
        for node in nodes {
            self.record(node, Some(parent))?;
        }
        Ok(())
    }

    fn mark_node(&mut self, id: &Value) {
        if let Some(node) = id.as_i64().and_then(|id| self.nodes.get_mut(&id)) {
            node.marked = true;
        }
    }

    fn sensitive_ids(&self) -> HashSet<i64> {
        let mut sensitive = HashSet::new();
        let mut resolved = HashMap::new();
        for id in self.nodes.keys() {
            let mut path = Vec::new();
            let mut visiting = HashSet::new();
            let mut next = Some(*id);
            let mut marked = false;
            while let Some(id) = next {
                if let Some(inherited) = resolved.get(&id) {
                    marked |= *inherited;
                    break;
                }
                if !visiting.insert(id) {
                    marked = true;
                    break;
                }
                let Some(node) = self.nodes.get(&id) else {
                    marked = true;
                    break;
                };
                path.push(id);
                marked |= node.marked;
                if marked {
                    break;
                }
                next = node.parent;
            }
            for id in path {
                resolved.insert(id, marked);
                if marked {
                    if let Some(backend) = self.nodes[&id].backend {
                        sensitive.insert(backend);
                    }
                }
            }
        }
        sensitive
    }
}

async fn collect(
    page: &PageChannel,
    events: &mut EventStream,
    dom: &mut Dom,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let request = page.send(method, params);
    tokio::pin!(request);
    let result = loop {
        tokio::select! {
            event = events.recv() => {
                let event = event.ok_or("redaction_failed: DOM observation closed")?;
                if dom.event(&event).is_err() {
                    dom.uncertain = true;
                }
            }
            result = &mut request => break result.map_err(|error| format!("redaction_failed: {error}"))?,
        }
    };
    for _ in 0..256 {
        let Ok(event) = events.try_recv() else {
            break;
        };
        if dom.event(&event).is_err() {
            dom.uncertain = true;
        }
    }
    if events.has_lost_events() {
        dom.uncertain = true;
    }
    Ok(result)
}

pub async fn redact_page(nodes: &mut [AxNode], page: &PageChannel) {
    if let Err(error) = capture(nodes, page).await {
        log::debug!("[browser-snapshot] sensitive ancestry unavailable: {error}");
        redact_all(nodes);
    }
}

fn redact_all(nodes: &mut [AxNode]) {
    for node in nodes {
        redact_node(node);
    }
}

async fn capture(nodes: &mut [AxNode], page: &PageChannel) -> Result<(), String> {
    let mut events = page.subscribe_session(&[
        "DOM.setChildNodes",
        "DOM.documentUpdated",
        "DOM.attributeModified",
        "DOM.attributeRemoved",
        "DOM.childNodeInserted",
        "DOM.childNodeRemoved",
        "DOM.shadowRootPushed",
        "DOM.shadowRootPopped",
    ]);
    let mut dom = Dom::default();
    let document = collect(
        page,
        &mut events,
        &mut dom,
        "DOM.getDocument",
        json!({"depth":0,"pierce":true}),
    )
    .await?;
    dom.record(
        document
            .get("root")
            .ok_or("redaction_failed: DOM document missing")?,
        None,
    )?;
    let ids: HashSet<_> = nodes
        .iter()
        .filter_map(|node| node.backend_dom_node_id)
        .collect();
    let ids: Vec<_> = ids.into_iter().collect();
    let mut unavailable = HashSet::new();
    for ids in ids.chunks(64) {
        let result = collect(
            page,
            &mut events,
            &mut dom,
            "DOM.pushNodesByBackendIdsToFrontend",
            json!({"backendNodeIds":ids}),
        )
        .await?;
        let pushed = result
            .get("nodeIds")
            .and_then(Value::as_array)
            .ok_or("redaction_failed: DOM ancestry result missing")?;
        if pushed.len() != ids.len() {
            return Err("redaction_failed: DOM ancestry result length mismatch".into());
        }
        for (backend, id) in ids.iter().zip(pushed) {
            if id
                .as_i64()
                .is_none_or(|id| id == 0 || !dom.nodes.contains_key(&id))
            {
                unavailable.insert(*backend);
            }
        }
    }
    let known: HashSet<_> = dom.nodes.values().filter_map(|node| node.backend).collect();
    if dom.uncertain {
        redact_all(nodes);
    } else {
        let mut sensitive = dom.sensitive_ids();
        sensitive.extend(unavailable);
        sensitive.extend(ids.iter().filter(|id| !known.contains(id)));
        redact_ids(nodes, &sensitive);
    }
    Ok(())
}

fn marked(node: &Value) -> bool {
    node.get("attributes")
        .and_then(Value::as_array)
        .is_some_and(|attrs| {
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
        })
}

pub fn redact(nodes: &mut [AxNode], document: &Value) -> Result<(), String> {
    let root = document
        .get("root")
        .ok_or("redaction_failed: DOM document missing")?;
    let mut sensitive = HashSet::new();
    let mut stack = vec![(root, false)];
    while let Some((node, inherited)) = stack.pop() {
        let marked = inherited || marked(node);
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
