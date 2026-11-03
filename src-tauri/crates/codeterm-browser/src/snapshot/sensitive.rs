use super::{AxNode, AxValue};
use crate::{EventStream, PageChannel};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Dom {
    nodes: HashMap<i64, DomNode>,
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
                    self.mark_node(&event.params["nodeId"], "sensitive attribute changed");
                }
                return Ok(());
            }
            "DOM.attributeRemoved" => return Ok(()),
            "DOM.childNodeRemoved" => {
                self.mark_node(&event.params["nodeId"], "node detached during capture");
                return Ok(());
            }
            "DOM.shadowRootPopped" => {
                self.mark_node(
                    &event.params["rootId"],
                    "shadow root detached during capture",
                );
                return Ok(());
            }
            "DOM.childNodeInserted" => {
                return self.record(&event.params["node"], event.params["parentNodeId"].as_i64());
            }
            "DOM.shadowRootPushed" => {
                return self.record(&event.params["root"], event.params["hostId"].as_i64());
            }
            "DOM.setChildNodes" => {}
            "DOM.documentUpdated" => {
                log::debug!("[browser-snapshot] DOM document updated; retaining observed ancestry and redacting unresolved backend nodes individually");
                return Ok(());
            }
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

    fn mark_node(&mut self, id: &Value, reason: &str) {
        if let Some(node) = id.as_i64().and_then(|id| self.nodes.get_mut(&id)) {
            node.marked = true;
            log::debug!(
                "[browser-snapshot] backend node {:?} treated as sensitive: {reason}",
                node.backend
            );
        }
    }

    fn sensitive_ids(&self) -> HashSet<i64> {
        let mut sensitive = HashSet::new();
        let mut resolved = HashMap::new();
        for id in self.nodes.keys() {
            let backend = self.nodes[id].backend;
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
                    log::debug!("[browser-snapshot] backend node {backend:?} treated as sensitive: cyclic DOM ancestry at frontend node {id}");
                    marked = true;
                    break;
                }
                let Some(node) = self.nodes.get(&id) else {
                    log::debug!("[browser-snapshot] backend node {backend:?} treated as sensitive: missing DOM ancestor {id}");
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
                if let Err(error) = dom.event(&event) {
                    log::debug!("[browser-snapshot] incomplete {} ancestry record: {error}; unresolved backend nodes will be redacted individually", event.method);
                }
            }
            result = &mut request => break result.map_err(|error| format!("redaction_failed: {error}"))?,
        }
    };
    for _ in 0..256 {
        let Ok(event) = events.try_recv() else {
            break;
        };
        if let Err(error) = dom.event(&event) {
            log::debug!("[browser-snapshot] incomplete {} ancestry record: {error}; unresolved backend nodes will be redacted individually", event.method);
        }
    }
    if events.has_lost_events() {
        log::debug!("[browser-snapshot] DOM ancestry events lost; unresolved backend nodes will be redacted individually");
    }
    Ok(result)
}

pub async fn redact_page(nodes: &mut [AxNode], page: &PageChannel) {
    let ids: HashSet<_> = nodes
        .iter()
        .filter_map(|node| node.backend_dom_node_id)
        .collect();
    let ids: Vec<_> = ids.into_iter().collect();
    let mut dom = Dom::default();
    let mut unavailable = HashSet::new();
    if let Err(error) = capture(&ids, page, &mut dom, &mut unavailable).await {
        log::debug!("[browser-snapshot] sensitive ancestry unavailable: {error}");
    }
    let known: HashSet<_> = dom.nodes.values().filter_map(|node| node.backend).collect();
    let mut sensitive = dom.sensitive_ids();
    sensitive.extend(unavailable);
    for id in ids.iter().filter(|id| !known.contains(id)) {
        log::debug!(
            "[browser-snapshot] backend node {id} treated as sensitive: no DOM ancestry received"
        );
        sensitive.insert(*id);
    }
    redact_ids(nodes, &sensitive);
}

async fn capture(
    ids: &[i64],
    page: &PageChannel,
    dom: &mut Dom,
    unavailable: &mut HashSet<i64>,
) -> Result<(), String> {
    page.send("DOM.disable", json!({}))
        .await
        .map_err(|error| format!("DOM frontend reset failed: {error}"))?;
    let mut events = page.subscribe_session_reliable(&[
        "DOM.setChildNodes",
        "DOM.documentUpdated",
        "DOM.attributeModified",
        "DOM.attributeRemoved",
        "DOM.childNodeInserted",
        "DOM.childNodeRemoved",
        "DOM.shadowRootPushed",
        "DOM.shadowRootPopped",
    ]);
    let document = collect(
        page,
        &mut events,
        dom,
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
    for ids in ids.chunks(64) {
        let result = collect(
            page,
            &mut events,
            dom,
            "DOM.pushNodesByBackendIdsToFrontend",
            json!({"backendNodeIds":ids}),
        )
        .await;
        let pushed = result
            .as_ref()
            .ok()
            .and_then(|result| result.get("nodeIds"))
            .and_then(Value::as_array);
        for (index, backend) in ids.iter().enumerate() {
            let id = pushed
                .and_then(|ids| ids.get(index))
                .and_then(Value::as_i64);
            let reason = match id {
                None => Some("DOM push returned no frontend id"),
                Some(0) => Some("DOM backend node is detached"),
                Some(id)
                    if dom
                        .nodes
                        .get(&id)
                        .is_some_and(|node| node.backend != Some(*backend)) =>
                {
                    Some("DOM frontend/backend identity mismatch")
                }
                _ => None,
            };
            if let Some(reason) = reason {
                log::debug!("[browser-snapshot] backend node {backend} treated as sensitive: {reason}; push error: {:?}", result.as_ref().err());
                unavailable.insert(*backend);
            }
        }
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
