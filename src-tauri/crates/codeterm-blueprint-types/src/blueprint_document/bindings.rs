use super::{checker::ports, Document, Node, Port};
use std::collections::BTreeMap;

pub fn named(node: &Node) -> bool {
    matches!(node.kind(), "code" | "command")
        && !node.bind.is_empty()
        && !node.bind.contains_key("in")
}

pub fn source_schema(doc: &Document, binding: &str) -> Result<Port, String> {
    let (id, path) = binding
        .split_once(".out")
        .ok_or("Expected a node out binding")?;
    let node = doc
        .nodes()
        .find(|n| n.id == id)
        .ok_or("Unknown bind producer")?;
    let output = node.output.clone().unwrap_or_else(|| Port {
        kind: if matches!(node.kind(), "code" | "call") {
            ""
        } else {
            "string"
        }
        .into(),
        ..Default::default()
    });
    let mut output = ports::resolve(&output, &doc.types)?;
    for part in path
        .strip_prefix('.')
        .unwrap_or(path)
        .split('.')
        .filter(|p| !p.is_empty())
    {
        if !output.required.iter().any(|name| name == part) {
            return Err(format!("{part} is not a required field of {id}.out"));
        }
        output = output
            .properties
            .remove(part)
            .ok_or("Unknown output field")?;
    }
    output.media = None;
    Ok(output)
}

pub fn envelope_schema(doc: &Document, node: &Node) -> Result<Port, String> {
    let properties = node
        .bind
        .iter()
        .map(|(name, binding)| source_schema(doc, binding).map(|port| (name.clone(), port)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(Port {
        kind: "object".into(),
        required: properties.keys().cloned().collect(),
        properties,
        additional_properties: Some(false),
        media: Some("json".into()),
        ..Default::default()
    })
}

pub fn input_schema(doc: &Document, node: &Node) -> Result<Option<Port>, String> {
    if named(node) && node.input.is_none() {
        envelope_schema(doc, node).map(Some)
    } else {
        Ok(node.input.clone())
    }
}

pub fn named_bindings(node: &Node) -> BTreeMap<String, String> {
    if named(node) {
        node.bind.clone()
    } else {
        BTreeMap::new()
    }
}
