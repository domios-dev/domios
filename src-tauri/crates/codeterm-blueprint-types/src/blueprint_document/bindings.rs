use super::{checker::ports, Document, Node, Port};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding<'a> {
    pub node: &'a str,
    pub selection: &'a str,
}

pub fn parse(binding: &str) -> Result<Binding<'_>, &'static str> {
    let (node, rest) = binding
        .split_once('.')
        .ok_or("Expected a node out binding")?;
    let selection = if rest == "out" {
        ""
    } else {
        rest.strip_prefix("out.")
            .filter(|path| !path.split('.').any(str::is_empty))
            .ok_or("Expected a node out binding")?
    };
    if node.is_empty()
        || node.len() > 64
        || matches!(node, "next" | "stop")
        || !node
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err("Expected a node out binding");
    }
    Ok(Binding { node, selection })
}

pub(super) const NAMED_MEDIA_UNSUPPORTED: &str = "media ports in named binds are not supported yet";

pub(crate) fn unsupported_media(media: Option<&str>) -> bool {
    media.is_some_and(|media| !matches!(media, "json" | "text"))
}

pub fn named(node: &Node) -> bool {
    matches!(node.kind(), "code" | "command")
        && !node.bind.is_empty()
        && !node.bind.contains_key("in")
}

pub fn source_schema(doc: &Document, binding: &str) -> Result<Port, String> {
    let Binding {
        node: id,
        selection: path,
    } = parse(binding)?;
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
    if unsupported_media(output.media.as_deref())
        || unsupported_media(inferred_media(doc, node).as_deref())
    {
        return Err(NAMED_MEDIA_UNSUPPORTED.into());
    }
    for part in path.split('.').filter(|part| !part.is_empty()) {
        if !output.required.iter().any(|name| name == part) {
            return Err(format!("{part} is not a required field of {id}.out"));
        }
        output = output
            .properties
            .remove(part)
            .ok_or("Unknown output field")?;
    }
    if unsupported_media(output.media.as_deref()) {
        return Err(NAMED_MEDIA_UNSUPPORTED.into());
    }
    output.media = None;
    Ok(output)
}

fn inferred_media(doc: &Document, node: &Node) -> Option<String> {
    if let Some(pin) = node.call.as_ref().and_then(|call| call.pin.as_ref()) {
        let callee = doc.dependencies.get(&pin.key())?;
        return ports::resolve(
            callee.document.result.port.as_ref()?,
            &callee.document.types,
        )
        .ok()?
        .media;
    }
    let form = match node.kind() {
        "convert" => node.params.get("to"),
        "input" => node
            .params
            .get("convert_to")
            .or_else(|| node.params.get("kind")),
        _ => None,
    }?;
    let value = match form {
        super::ValueForm::Input { input } => doc.inputs.get(input)?.default.as_ref(),
        _ => form.value(),
    }?;
    value.as_str().map(str::to_owned)
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

#[cfg(test)]
mod tests;
