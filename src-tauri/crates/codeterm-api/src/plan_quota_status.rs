use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use codeterm_service_core::service::billing::{FREE_LIMIT_REACHED, PLAN_LIMIT_LOCKED};

const MAX_INSPECTED_BODY: usize = 64 * 1024;

/// A plan refusal is the caller's quota, never a server fault, whichever route mapped it.
pub async fn typed_plan_refusal(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }
    let (parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_INSPECTED_BODY).await else {
        return Response::from_parts(parts, axum::body::Body::empty());
    };
    let raw = String::from_utf8_lossy(&bytes);
    let json = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
    let message = json
        .as_ref()
        .and_then(|value| value.as_str().or_else(|| value.get("error")?.as_str()))
        .unwrap_or(&raw);
    match refusal_code(message) {
        Some(_) => plan_refusal(message),
        None => Response::from_parts(parts, axum::body::Body::from(bytes)),
    }
}

pub(crate) fn refusal_code(message: &str) -> Option<&'static str> {
    [FREE_LIMIT_REACHED, PLAN_LIMIT_LOCKED]
        .into_iter()
        .find(|code| {
            message
                .strip_prefix(code)
                .is_some_and(|suffix| suffix.starts_with(": "))
        })
}

pub(crate) fn route_error(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    let message = message.into();
    let code = refusal_code(&message);
    (
        if code.is_some() {
            StatusCode::FORBIDDEN
        } else {
            status
        },
        axum::Json(serde_json::json!({
            "ok": false, "error": {"code": code.unwrap_or("request_failed"), "message": message}
        })),
    )
}

fn plan_refusal(message: &str) -> Response {
    axum::response::IntoResponse::into_response(route_error(StatusCode::FORBIDDEN, message))
}

#[cfg(test)]
mod tests;
