use super::BrowserRuntime;
use codeterm_browser::PageChannel;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_PROBE_POINTS: usize = 16;
const INSET_PX: f64 = 4.0;
const RECT_TOLERANCE_PX: f64 = 8.0;
const DEDUP_EPSILON_PX: f64 = 0.25;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.left + self.width
    }

    fn bottom(&self) -> f64 {
        self.top + self.height
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContainedModal {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub dom_id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DomNode {
    pub id: usize,
    pub parent: Option<usize>,
    pub tag: String,
    #[serde(default)]
    pub dom_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub href: String,
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub has_interactive_descendant: bool,
    #[serde(default)]
    pub modal: bool,
    #[serde(default)]
    pub landmark_or_chrome: bool,
    #[serde(default)]
    pub clickable_container: bool,
    #[serde(default)]
    pub cursor_pointer: bool,
    #[serde(default)]
    pub associated_label: bool,
    #[serde(default)]
    pub rect: Option<Rect>,
    #[serde(default)]
    pub contained_modal: Option<ContainedModal>,
}

impl DomNode {
    fn is_document_root(&self) -> bool {
        matches!(self.tag.as_str(), "html" | "body")
    }

    fn is_pinned(&self) -> bool {
        matches!(self.position.as_str(), "fixed" | "sticky")
    }

    fn is_card_item(&self) -> bool {
        matches!(self.tag.as_str(), "article" | "li")
            || matches!(self.role.as_str(), "article" | "listitem")
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HitScene {
    pub target: usize,
    pub stack: Vec<usize>,
    pub nodes: Vec<DomNode>,
}

impl HitScene {
    fn node(&self, id: usize) -> Option<&DomNode> {
        self.nodes
            .get(id)
            .filter(|node| node.id == id)
            .or_else(|| self.nodes.iter().find(|node| node.id == id))
    }

    fn ancestors_inclusive(&self, start: usize) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(id) = current {
            if chain.contains(&id) {
                break;
            }
            chain.push(id);
            current = self.node(id).and_then(|node| node.parent);
        }
        chain
    }

    fn is_descendant(&self, candidate: usize, ancestor: usize) -> bool {
        self.ancestors_inclusive(candidate)
            .into_iter()
            .skip(1)
            .any(|id| id == ancestor)
    }

    fn is_ancestor(&self, candidate: usize, descendant: usize) -> bool {
        self.is_descendant(descendant, candidate)
    }

    fn lowest_common_ancestor(&self, a: usize, b: usize) -> Option<usize> {
        let chain_b = self.ancestors_inclusive(b);
        self.ancestors_inclusive(a)
            .into_iter()
            .find(|id| chain_b.contains(id))
    }

    fn ancestors_exclusive(&self, start: usize, stop_at: usize) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut current = self.node(start).and_then(|node| node.parent);
        while let Some(id) = current {
            if id == stop_at || chain.contains(&id) {
                break;
            }
            chain.push(id);
            current = self.node(id).and_then(|node| node.parent);
        }
        chain
    }

    fn is_associated_label_hit(&self, hit_id: usize) -> bool {
        let mut current = Some(hit_id);
        let mut hops = 0;
        while let Some(id) = current {
            if hops > self.nodes.len() {
                return false;
            }
            hops += 1;
            let Some(node) = self.node(id) else {
                return false;
            };
            if node.associated_label {
                return true;
            }
            if node.interactive || node.modal {
                return false;
            }
            current = node.parent;
        }
        false
    }

    fn effective_href(&self, start: usize, stop_at: usize) -> Option<&str> {
        let mut current = Some(start);
        let mut hops = 0;
        while let Some(id) = current {
            if id == stop_at || hops > self.nodes.len() {
                break;
            }
            let node = self.node(id)?;
            if !node.href.is_empty() {
                return Some(node.href.as_str());
            }
            current = node.parent;
            hops += 1;
        }
        None
    }
}

fn rect_within_container(layer: Option<&Rect>, container: Option<&Rect>) -> bool {
    let (Some(layer), Some(container)) = (layer, container) else {
        return true;
    };
    if container.width <= 0.0 || container.height <= 0.0 {
        return false;
    }
    layer.left >= container.left - RECT_TOLERANCE_PX
        && layer.top >= container.top - RECT_TOLERANCE_PX
        && layer.right() <= container.right() + RECT_TOLERANCE_PX
        && layer.bottom() <= container.bottom() + RECT_TOLERANCE_PX
}

fn find_clickable_container(scene: &HitScene, lca_id: usize, layer: &DomNode) -> Option<usize> {
    let mut current = Some(lca_id);
    let mut visited = 0;
    while let Some(id) = current {
        if visited > scene.nodes.len() {
            return None;
        }
        visited += 1;
        let node = scene.node(id)?;
        if node.is_document_root() || node.modal || node.landmark_or_chrome {
            return None;
        }
        let clickable = node.clickable_container || (layer.cursor_pointer && node.is_card_item());
        if clickable && rect_within_container(layer.rect.as_ref(), node.rect.as_ref()) {
            return Some(id);
        }
        current = node.parent;
    }
    None
}

fn is_click_through_overlay(scene: &HitScene, layer_id: usize) -> bool {
    let Some(layer) = scene.node(layer_id) else {
        return false;
    };
    if !layer.transparent || layer.modal || layer.landmark_or_chrome || layer.is_pinned() {
        return false;
    }
    if layer.has_interactive_descendant || layer.contained_modal.is_some() {
        return false;
    }
    let Some(lca_id) = scene.lowest_common_ancestor(layer_id, scene.target) else {
        return false;
    };
    let Some(lca) = scene.node(lca_id) else {
        return false;
    };
    if lca.is_document_root() || lca.modal || lca.landmark_or_chrome {
        return false;
    }
    for ancestor_id in scene.ancestors_exclusive(layer_id, lca_id) {
        let Some(node) = scene.node(ancestor_id) else {
            return false;
        };
        if node.modal || node.landmark_or_chrome || node.is_pinned() || node.interactive {
            return false;
        }
    }
    for ancestor_id in scene.ancestors_exclusive(scene.target, lca_id) {
        let Some(node) = scene.node(ancestor_id) else {
            return false;
        };
        if node.modal || node.landmark_or_chrome {
            return false;
        }
    }
    if layer.interactive {
        let same_destination = !layer.href.is_empty()
            && scene
                .effective_href(scene.target, lca_id)
                .is_some_and(|href| href == layer.href);
        if !same_destination {
            return false;
        }
        return rect_within_container(layer.rect.as_ref(), lca.rect.as_ref());
    }
    find_clickable_container(scene, lca_id, layer).is_some()
}

fn format_label(role: &str, tag: &str, dom_id: &str, name: &str) -> String {
    let kind = if !role.is_empty() {
        role
    } else if !tag.is_empty() {
        tag
    } else {
        "element"
    };
    let mut label = String::from(kind);
    if !dom_id.is_empty() {
        label.push('#');
        label.push_str(dom_id);
    }
    if !name.is_empty() {
        label.push_str(" \"");
        label.push_str(name);
        label.push('"');
    }
    label
}

fn describe_occluder(scene: &HitScene, hit_id: usize) -> String {
    let lca_id = scene.lowest_common_ancestor(hit_id, scene.target);
    let mut best_modal = None;
    let mut best_interactive = None;
    let mut best_named = None;
    let mut current = Some(hit_id);
    let mut visited = 0;
    while let Some(id) = current {
        if Some(id) == lca_id || visited > scene.nodes.len() {
            break;
        }
        visited += 1;
        let Some(node) = scene.node(id) else {
            break;
        };
        if node.modal && best_modal.is_none() {
            best_modal = Some(node);
        }
        if node.interactive && best_interactive.is_none() {
            best_interactive = Some(node);
        }
        if !node.role.is_empty() && !node.name.is_empty() && best_named.is_none() {
            best_named = Some(node);
        }
        current = node.parent;
    }
    if let Some(node) = best_modal.or(best_interactive).or(best_named) {
        return format_label(&node.role, &node.tag, &node.dom_id, &node.name);
    }
    if let Some(node) = scene.node(hit_id) {
        if node.role.is_empty() && node.name.is_empty() {
            if let Some(modal) = &node.contained_modal {
                return format_label(&modal.role, &modal.tag, &modal.dom_id, &modal.name);
            }
        }
        return format_label(&node.role, &node.tag, &node.dom_id, &node.name);
    }
    "element".into()
}

pub(super) fn classify_scene(scene: &HitScene) -> Result<(), String> {
    if scene.stack.is_empty() {
        return Err("target_occluded: no element at pointer coordinates".into());
    }
    if scene.node(scene.target).is_none() {
        return Err("stale: detached pointer target".into());
    }
    let mut saw_overlay = false;
    for &hit_id in &scene.stack {
        if hit_id == scene.target
            || scene.is_descendant(hit_id, scene.target)
            || scene.is_associated_label_hit(hit_id)
        {
            return Ok(());
        }
        if scene.is_ancestor(hit_id, scene.target) {
            let Some(node) = scene.node(hit_id) else {
                return Err("target_occluded: pointer hit-test unavailable".into());
            };
            if node.is_document_root() {
                if !saw_overlay {
                    return Err(format!(
                        "target_occluded: covered by {}",
                        describe_occluder(scene, hit_id)
                    ));
                }
                break;
            }
            return Ok(());
        }
        if is_click_through_overlay(scene, hit_id) {
            saw_overlay = true;
            continue;
        }
        return Err(format!(
            "target_occluded: covered by {}",
            describe_occluder(scene, hit_id)
        ));
    }
    Err(format!(
        "target_occluded: covered by {}",
        describe_occluder(scene, scene.stack[0])
    ))
}

fn lerp_quad(q: &[f64], u: f64, v: f64) -> (f64, f64) {
    let x =
        (1.0 - u) * (1.0 - v) * q[0] + u * (1.0 - v) * q[2] + u * v * q[4] + (1.0 - u) * v * q[6];
    let y =
        (1.0 - u) * (1.0 - v) * q[1] + u * (1.0 - v) * q[3] + u * v * q[5] + (1.0 - u) * v * q[7];
    (x, y)
}

fn edge_length(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (bx - ax).hypot(by - ay)
}

fn inset_fraction(span: f64) -> f64 {
    if !span.is_finite() || span <= 0.0 {
        return 0.5;
    }
    (INSET_PX.min(span * 0.25) / span).clamp(0.005, 0.25)
}

pub(super) fn candidate_points(result: &Value) -> Vec<(f64, f64)> {
    let Some(quads) = result.get("quads").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut push_unique = |point: (f64, f64)| {
        if !point.0.is_finite() || !point.1.is_finite() || out.len() >= MAX_PROBE_POINTS {
            return;
        }
        if out.iter().all(|&(px, py)| {
            (px - point.0).abs() > DEDUP_EPSILON_PX || (py - point.1).abs() > DEDUP_EPSILON_PX
        }) {
            out.push(point);
        }
    };
    for (index, raw_quad) in quads.iter().enumerate() {
        let Some(entries) = raw_quad.as_array() else {
            continue;
        };
        let points: Vec<f64> = entries.iter().filter_map(Value::as_f64).collect();
        if points.len() < 8 || !points[..8].iter().all(|v| v.is_finite()) {
            continue;
        }
        let width = (edge_length(points[0], points[1], points[2], points[3])
            + edge_length(points[6], points[7], points[4], points[5]))
            * 0.5;
        let height = (edge_length(points[0], points[1], points[6], points[7])
            + edge_length(points[2], points[3], points[4], points[5]))
            * 0.5;
        let iu = inset_fraction(width);
        let iv = inset_fraction(height);
        let samples: &[(f64, f64)] = if index == 0 {
            &[
                (0.5, 0.5),
                (iu, 0.5),
                (1.0 - iu, 0.5),
                (0.5, iv),
                (0.5, 1.0 - iv),
                (iu, iv),
                (1.0 - iu, iv),
                (iu, 1.0 - iv),
                (1.0 - iu, 1.0 - iv),
            ]
        } else {
            &[(0.5, 0.5), (iu, 0.5), (1.0 - iu, 0.5)]
        };
        for &(u, v) in samples {
            push_unique(lerp_quad(&points, u, v));
        }
    }
    out
}

fn hit_test_result(reply: &Value) -> Result<(), String> {
    if let Some(details) = reply.get("exceptionDetails") {
        log::debug!(
            "[browser-hit-test] in-page hit-test threw: {}",
            details
                .pointer("/exception/description")
                .and_then(Value::as_str)
                .unwrap_or("unknown exception")
        );
        return Err("target_occluded: pointer hit-test unavailable".into());
    }
    let Some(value) = reply.pointer("/result/value") else {
        return Err("target_occluded: pointer hit-test unavailable".into());
    };
    if let Some(reason) = value.as_str() {
        return match reason {
            "clear" => Ok(()),
            r if r.starts_with("target_occluded:") || r.starts_with("stale:") => Err(r.to_string()),
            _ => Err("target_occluded: pointer hit-test unavailable".into()),
        };
    }
    if value.is_object() {
        if let Ok(scene) = serde_json::from_value::<HitScene>(value.clone()) {
            return classify_scene(&scene);
        }
    }
    Err("target_occluded: pointer hit-test unavailable".into())
}

impl BrowserRuntime {
    async fn resolve_pointer_object(
        &self,
        page: &PageChannel,
        backend_node_id: i64,
    ) -> Result<String, String> {
        let resolved = self
            .send(
                page,
                "DOM.resolveNode",
                json!({"backendNodeId": backend_node_id}),
            )
            .await?;
        resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "stale: detached pointer target".into())
    }

    async fn hit_test_object(
        &self,
        page: &PageChannel,
        object_id: &str,
        x: f64,
        y: f64,
    ) -> Result<(), String> {
        let reply = self
            .send(
                page,
                "Runtime.callFunctionOn",
                json!({
                    "objectId": object_id,
                    "functionDeclaration": include_str!("hit-test.js"),
                    "arguments": [{"value": x}, {"value": y}],
                    "returnByValue": true,
                }),
            )
            .await?;
        hit_test_result(&reply)
    }

    pub(super) async fn require_pointer_target(
        &self,
        page: &PageChannel,
        backend_node_id: i64,
        x: f64,
        y: f64,
    ) -> Result<(), String> {
        let id = self.resolve_pointer_object(page, backend_node_id).await?;
        let outcome = self.hit_test_object(page, &id, x, y).await;
        let _ = self
            .send(page, "Runtime.releaseObject", json!({"objectId": id}))
            .await;
        outcome
    }

    pub(super) async fn choose_pointer_point(
        &self,
        page: &PageChannel,
        backend_node_id: i64,
        quads: &Value,
    ) -> Result<(f64, f64), String> {
        let points = candidate_points(quads);
        if points.is_empty() {
            return Err("hidden: target has no box".into());
        }
        let id = self.resolve_pointer_object(page, backend_node_id).await?;
        let mut first_refusal = None;
        let mut chosen = None;
        for (x, y) in points {
            match self.hit_test_object(page, &id, x, y).await {
                Ok(()) => {
                    chosen = Some(Ok((x, y)));
                    break;
                }
                Err(reason) if reason.starts_with("target_occluded:") => {
                    if first_refusal.is_none() {
                        first_refusal = Some(reason);
                    }
                }
                Err(reason) => {
                    chosen = Some(Err(reason));
                    break;
                }
            }
        }
        let _ = self
            .send(page, "Runtime.releaseObject", json!({"objectId": id}))
            .await;
        chosen.unwrap_or_else(|| {
            Err(first_refusal
                .unwrap_or_else(|| "target_occluded: pointer hit-test unavailable".into()))
        })
    }
}

#[cfg(test)]
mod tests;
