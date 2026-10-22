use super::*;
use crate::blueprint_document::{Node, ValueForm};
use crate::blueprint_execution::ExecutionContract;
use crate::blueprint_pipeline::{IoType, Step, StepKind};
use serde_json::{json, Value};

pub fn node(document: &Document, node: &Node) -> Result<Step, String> {
    let kind = if let Some(call) = &node.call {
        if call.pin.is_none() {
            return Err("Cannot lower an unresolved call".into());
        }
        StepKind::Call {
            call: call.clone(),
            grants: node.caps.clone(),
        }
    } else if let Some(code) = &node.code {
        StepKind::Code { code: code.clone() }
    } else if node
        .uses
        .as_ref()
        .is_some_and(super::super::NodeKind::effect)
    {
        StepKind::Effect {
            node: Box::new(node.clone()),
        }
    } else if let Some(run) = &node.run {
        StepKind::Command {
            command: literal(run)?
                .as_str()
                .ok_or("Command must be fixed author text")?
                .into(),
        }
    } else {
        let mut params = serde_json::Map::new();
        for (key, value) in &node.params {
            params.insert(key.clone(), literal(value)?);
        }
        let mut value = Value::Object(params);
        value["kind"] = json!(node.kind());
        if node.kind() == "ask" {
            value["buckets"] = json!(node.buckets);
        }
        if matches!(node.kind(), "http" | "github") {
            let kind = value["kind"].clone();
            value
                .as_object_mut()
                .ok_or("Expected node parameters")?
                .remove("kind");
            value = json!({"kind":kind,"probe":value});
        }
        serde_json::from_value(value)
            .map_err(|e| format!("Unsupported native callee node {}: {e}", node.id))?
    };
    Ok(Step {
        name: node.id.clone(),
        input: IoType::Text,
        kind,
        execution: Some(ExecutionContract {
            input: if node.call.is_some() {
                None
            } else {
                node.input.clone()
            },
            input_binding: node.bind.get("in").cloned(),
            output: node.output.clone(),
            types: document.types.clone(),
            limits: node.limits.clone(),
        }),
        on: node
            .routes
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    match v.as_str() {
                        "@next" => "next".into(),
                        "@stop" => "stop".into(),
                        v => v.into(),
                    },
                )
            })
            .collect(),
    })
}
fn literal(value: &ValueForm) -> Result<Value, String> {
    value
        .value()
        .cloned()
        .ok_or("Lowering refuses unresolved data in an executable slot".into())
}
