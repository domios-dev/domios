use super::*;
use codeterm_blueprint_types::blueprint_document::{projection, Document, Port};
use codeterm_mem::blueprint_library::{authoring::AuthoringRecall, BlueprintHit};
use std::collections::BTreeMap;

pub(super) fn recall(
    svc: &ServiceContext,
    caller: &SentinelCaller,
    intent: &str,
    limit: usize,
) -> Result<Value, String> {
    if !caller_is_live(&svc.mux.read(), caller) {
        return Err("Blueprint caller is no longer live".into());
    }
    let result = svc.mem.recall_authoring_blueprints(
        &area(&caller.workspace),
        caller
            .team
            .as_deref()
            .ok_or("Typed call recall requires a Team")?,
        intent,
        limit,
    )?;
    let mut value = response(result)?;
    if let Some(cards) = value["cards"].as_array_mut() {
        for card in cards {
            card["tags"] = json!(svc
                .mem
                .blueprint_tags(card["id"].as_str().ok_or("Recall card id missing")?)?);
        }
    }
    Ok(value)
}

fn response(result: AuthoringRecall) -> Result<Value, String> {
    let cards = result
        .hits
        .into_iter()
        .map(card)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        json!({"cards":cards,"recall":result.state,"unavailable":result.unavailable.into_iter().map(|(id,reason)|json!({"id":id,"reason":reason})).collect::<Vec<_>>()}),
    )
}

fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(360)
        .collect()
}

fn signature(port: &Port, document: &Document, depth: usize) -> Result<Value, String> {
    if depth > 32 {
        return Err("Callable port schema exceeds depth 32".into());
    }
    if let Some(reference) = &port.reference {
        return signature(
            document
                .types
                .get(reference)
                .ok_or_else(|| format!("Unresolved port type {reference}"))?,
            document,
            depth + 1,
        );
    }
    let mut schema = serde_json::to_value(port).map_err(|e| e.to_string())?;
    let object = schema.as_object_mut().ok_or("Invalid port schema")?;
    object.remove("description");
    object.remove("default");
    if !port.properties.is_empty() {
        let fields = port
            .properties
            .iter()
            .map(|(key, p)| Ok((key.clone(), signature(p, document, depth + 1)?)))
            .collect::<Result<serde_json::Map<_, _>, String>>()?;
        object.insert("properties".into(), Value::Object(fields));
    }
    if let Some(items) = &port.items {
        object.insert("items".into(), signature(items, document, depth + 1)?);
    }
    Ok(schema)
}

fn card(hit: BlueprintHit) -> Result<Value, String> {
    let document = &hit.entry.document;
    let mut body = document.clone();
    body.dependencies.clear();
    let id = if hit.entry.id.starts_with("catalog:") || hit.entry.id.starts_with("mem:") {
        hit.entry.id.clone()
    } else {
        format!("mem:{}", hit.entry.id)
    };
    let provenance = if id.starts_with("catalog:") {
        "catalog"
    } else {
        "mem"
    };
    let inputs = document
        .inputs
        .iter()
        .map(|(name, p)| Ok((name.clone(), signature(p, document, 0)?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let result = document
        .result
        .port
        .as_ref()
        .map(|p| signature(p, document, 0))
        .transpose()?;
    let mut node_kinds = BTreeMap::<&str, usize>::new();
    for node in document.nodes() {
        *node_kinds.entry(node.kind()).or_default() += 1;
    }
    Ok(json!({
        "id":id,"provenance":format!("{provenance}:"),"hash":projection::identity(&body)?,"score":hit.score,
        "when_to_use":one_line(&hit.entry.when_to_use()),"why":one_line(&hit.why),
        "signature":{"inputs":inputs,"result":result},"node_kinds":node_kinds,
        "enrichment_pending":hit.enrichment_pending,
    }))
}

#[cfg(test)]
mod tests;
