use serde_json::{json, Value};
use tauri::AppHandle;

use super::JsonRpcError;

fn emit_canvas_focus(app: &AppHandle, payload: Value) {
    crate::api::events::emit_everywhere(app, "canvas-focus", payload);
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    app: &AppHandle,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    match method {
        "canvas.focus" => {
            let payload = json!({
                "task_id": p.and_then(|x| x.get("task_id")).and_then(|v| v.as_str()),
                "tab_id": p.and_then(|x| x.get("tab_id")).and_then(|v| v.as_str()),
                "plan_id": p.and_then(|x| x.get("plan_id")).and_then(|v| v.as_str()),
            });
            emit_canvas_focus(app, payload.clone());
            Ok(json!({ "ok": true, "event": "canvas-focus", "payload": payload }))
        }
        "canvas.filter" => {
            let facet = p
                .and_then(|x| x.get("facet"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "Missing required parameter: facet".to_string(),
                    data: None,
                })?;
            let payload = json!({ "facet": facet, "dim": true });
            emit_canvas_focus(app, payload.clone());
            Ok(json!({ "ok": true, "event": "canvas-focus", "payload": payload }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}
