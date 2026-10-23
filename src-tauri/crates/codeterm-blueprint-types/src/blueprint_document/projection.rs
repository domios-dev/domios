use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalNode {
    pub id: String,
    pub order: usize,
    pub kind: String,
    pub node: Node,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_sha256: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalIr {
    pub schema_version: u32,
    pub document: Document,
    pub nodes: Vec<CanonicalNode>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PositionedIr {
    pub canonical: CanonicalIr,
    pub locations: BTreeMap<String, Location>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fact {
    pub predicate: String,
    pub arguments: Vec<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: BTreeMap<String, Vec<String>>,
    pub document_fields: Vec<String>,
}

pub fn canonical(doc: &Document) -> CanonicalIr {
    CanonicalIr {
        schema_version: SCHEMA_VERSION,
        document: doc.clone(),
        nodes: doc
            .nodes()
            .enumerate()
            .map(|(i, n)| CanonicalNode {
                id: n.id.clone(),
                order: i + 1,
                kind: n.kind().into(),
                node: n.clone(),
                body_sha256: n.code.as_ref().and_then(super::code_contract::body_sha256),
            })
            .collect(),
    }
}
pub fn json(doc: &Document) -> Result<String, String> {
    serde_json::to_value(canonical(doc))
        .map(|v| v.to_string())
        .map_err(|e| e.to_string())
}
pub fn identity(doc: &Document) -> Result<String, String> {
    Ok(Sha256::digest(json(doc)?.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
pub fn facts(doc: &Document) -> Vec<Fact> {
    let mut facts = Vec::new();
    let mut add = |predicate: &str, arguments: Vec<Value>| {
        facts.push(Fact {
            predicate: predicate.into(),
            arguments,
        })
    };
    for (i, node) in doc.nodes().enumerate() {
        add(
            "node",
            vec![json!(node.id), json!(node.kind()), json!(i + 1)],
        );
        for (outcome, target) in &node.routes {
            add("route", vec![json!(node.id), json!(outcome), json!(target)]);
        }
        for (field, port) in [("in", &node.input), ("out", &node.output)] {
            if let Some(port) = port {
                add("port", vec![json!(node.id), json!(field), json!(port)]);
            }
        }
        for (slot, value) in &node.params {
            add("param", vec![json!(node.id), json!(slot), json!(value)]);
        }
        for (slot, value) in &node.bind {
            add("bind", vec![json!(node.id), json!(slot), json!(value)]);
        }
        for cap in node
            .caps
            .iter()
            .chain(node.code.iter().flat_map(|c| &c.caps))
        {
            add("cap", vec![json!(node.id), json!(cap)]);
        }
        if let Some(call) = &node.call {
            add("call", vec![json!(node.id), json!(call)]);
        }
        if let Some(code) = &node.code {
            add("code", vec![json!(node.id), json!(code)]);
        }
        for bucket in &node.buckets {
            add("bucket", vec![json!(node.id), json!(bucket)]);
        }
    }
    facts
}
pub fn diff(before: &Document, after: &Document) -> SemanticDiff {
    let a = before
        .nodes()
        .map(|n| (n.id.clone(), n))
        .collect::<BTreeMap<_, _>>();
    let b = after
        .nodes()
        .map(|n| (n.id.clone(), n))
        .collect::<BTreeMap<_, _>>();
    let mut changed = BTreeMap::new();
    for (id, node) in &a {
        if let Some(other) = b.get(id) {
            let av = serde_json::to_value(node).unwrap_or_default();
            let bv = serde_json::to_value(other).unwrap_or_default();
            let mut fields = keys_changed(&av, &bv);
            if before.nodes().position(|n| &n.id == id) != after.nodes().position(|n| &n.id == id) {
                fields.push("order".into());
            }
            if !fields.is_empty() {
                changed.insert(id.clone(), fields);
            }
        }
    }
    let mut av = serde_json::to_value(before).unwrap_or_default();
    let mut bv = serde_json::to_value(after).unwrap_or_default();
    for value in [&mut av, &mut bv] {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("steps");
            fields.remove("other");
        }
    }
    SemanticDiff {
        added: b
            .keys()
            .filter(|id| !a.contains_key(*id))
            .cloned()
            .collect(),
        removed: a
            .keys()
            .filter(|id| !b.contains_key(*id))
            .cloned()
            .collect(),
        changed,
        document_fields: keys_changed(&av, &bv),
    }
}
fn keys_changed(a: &Value, b: &Value) -> Vec<String> {
    a.as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .chain(b.as_object().into_iter().flat_map(|m| m.keys()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect()
}
