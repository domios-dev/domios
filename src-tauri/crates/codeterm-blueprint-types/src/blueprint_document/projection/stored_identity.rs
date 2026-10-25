use super::{canonical, identity, Document};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn matches_identity(document: &Document, recorded: &str) -> Result<bool, String> {
    if identity(document)? == recorded {
        return Ok(true);
    }
    let ir = canonical(document);
    // macOS builds enabled serde_json preserve_order, so their pins hashed fields in declaration order.
    let declaration_order = serde_json::to_string(&ir).map_err(|error| error.to_string())?;
    if format!("{:x}", Sha256::digest(declaration_order.as_bytes())) == recorded {
        return Ok(true);
    }
    let current = serde_json::to_value(ir).map_err(|error| error.to_string())?;
    // Stored 1.15.2 pins include defaults omitted by fresh canonical identities.
    let mut prior = current.clone();
    restore_document_defaults(&mut prior["document"]);
    if let Some(nodes) = prior["nodes"].as_array_mut() {
        for node in nodes {
            restore_node_defaults(&mut node["node"]);
        }
    }
    // macOS builds hashed keys in insertion order (serde_json preserve_order); others sorted them.
    Ok([current, prior].into_iter().any(|mut value| {
        digest(&value) == recorded || {
            value.sort_all_objects();
            digest(&value) == recorded
        }
    }))
}

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

fn restore_document_defaults(document: &mut Value) {
    let Some(fields) = document.as_object_mut() else {
        return;
    };
    fields.entry("result").or_insert_with(|| json!({}));
    if let Some(trigger) = fields.get_mut("trigger").and_then(Value::as_object_mut) {
        if trigger.get("type").and_then(Value::as_str) == Some("event") {
            trigger.entry("params").or_insert_with(|| json!({}));
        }
    }
    for key in ["steps", "other"] {
        if let Some(nodes) = fields.get_mut(key).and_then(Value::as_array_mut) {
            for node in nodes {
                restore_node_defaults(node);
            }
        }
    }
    if let Some(blueprints) = fields.get_mut("blueprints").and_then(Value::as_object_mut) {
        for blueprint in blueprints.values_mut() {
            restore_document_defaults(blueprint);
        }
    }
    if let Some(dependencies) = fields
        .get_mut("dependencies")
        .and_then(Value::as_object_mut)
    {
        for dependency in dependencies.values_mut() {
            restore_document_defaults(&mut dependency["document"]);
        }
    }
}

fn restore_node_defaults(node: &mut Value) {
    let Some(fields) = node.as_object_mut() else {
        return;
    };
    for key in ["model_classification", "session", "paused"] {
        fields.entry(key).or_insert_with(|| json!(false));
    }
    if let Some(code) = fields.get_mut("code").and_then(Value::as_object_mut) {
        code.entry("limits").or_insert_with(|| json!({}));
    }
}

#[cfg(test)]
mod tests;
