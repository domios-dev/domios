use axum::{body::Body, http::Request};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tower::ServiceExt;

use super::JsonRpcError;

pub(super) fn dispatch(params: &Option<Value>, app: &AppHandle) -> Result<Value, JsonRpcError> {
    let params = params.as_ref().ok_or_else(|| invalid("params required"))?;
    let method = params
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("method required"))?;
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("path required"))?;
    if !path.starts_with("/api/") || path.starts_with("//") {
        return Err(invalid("path must be an absolute /api/ path"));
    }

    let mut request = Request::builder().method(method).uri(path);
    if let Some(headers) = params.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if name.eq_ignore_ascii_case("authorization") {
                continue;
            }
            if let Some(value) = value.as_str() {
                request = request.header(name, value);
            }
        }
    }
    if let Some(token) = app
        .state::<crate::config::ConfigState>()
        .lock()
        .api
        .token
        .as_deref()
    {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = params
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let request = request
        .body(Body::from(body))
        .map_err(|error| invalid(error.to_string()))?;
    let router = app.state::<crate::api::routes::LocalApiRouter>().0.clone();
    tauri::async_runtime::block_on(async move {
        let response = router
            .oneshot(request)
            .await
            .map_err(|never| match never {})?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_string(), Value::String(value.to_owned())))
            })
            .collect::<serde_json::Map<_, _>>();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
            .await
            .map_err(|error| internal(error.to_string()))?;
        Ok(serde_json::json!({
            "status": status,
            "headers": headers,
            "body": String::from_utf8_lossy(&body),
        }))
    })
}

fn invalid(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: message.into(),
        data: None,
    }
}

fn internal(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: message.into(),
        data: None,
    }
}
