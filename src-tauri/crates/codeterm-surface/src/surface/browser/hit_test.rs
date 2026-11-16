use super::BrowserRuntime;
use codeterm_browser::PageChannel;
use serde_json::{json, Value};

fn hit_test_result(reply: &Value) -> Result<(), String> {
    match reply.pointer("/result/value").and_then(Value::as_str) {
        Some("clear") => Ok(()),
        Some(reason) if reason.starts_with("target_occluded:") || reason.starts_with("stale:") => {
            Err(reason.to_string())
        }
        _ => Err("target_occluded: pointer hit-test unavailable".into()),
    }
}

impl BrowserRuntime {
    pub(super) async fn require_pointer_target(
        &self,
        page: &PageChannel,
        backend_node_id: i64,
        x: f64,
        y: f64,
    ) -> Result<(), String> {
        let resolved = self
            .send(
                page,
                "DOM.resolveNode",
                json!({"backendNodeId":backend_node_id}),
            )
            .await?;
        let id = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or("stale: detached pointer target")?;
        let reply = self
            .send(
                page,
                "Runtime.callFunctionOn",
                json!({
                    "objectId":id,
                    "functionDeclaration":include_str!("hit-test.js"),
                    "arguments":[{"value":x},{"value":y}],
                    "returnByValue":true,
                }),
            )
            .await;
        let _ = self
            .send(page, "Runtime.releaseObject", json!({"objectId":id}))
            .await;
        hit_test_result(&reply?)
    }
}

#[cfg(test)]
mod tests;
