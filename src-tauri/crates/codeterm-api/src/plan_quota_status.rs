use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use codeterm_service_core::service::billing::{FREE_LIMIT_REACHED, PLAN_LIMIT_LOCKED};

const MAX_INSPECTED_BODY: usize = 64 * 1024;

/// A plan refusal is the caller's quota, never a server fault, whichever route mapped it.
pub async fn typed_plan_refusal(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if !response.status().is_server_error() {
        return response;
    }
    let (parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_INSPECTED_BODY).await else {
        return Response::from_parts(parts, axum::body::Body::empty());
    };
    let text = String::from_utf8_lossy(&bytes);
    match [FREE_LIMIT_REACHED, PLAN_LIMIT_LOCKED]
        .into_iter()
        .find(|code| text.contains(code))
    {
        Some(code) => plan_refusal(code, &text),
        None => Response::from_parts(parts, axum::body::Body::from(bytes)),
    }
}

fn plan_refusal(code: &str, text: &str) -> Response {
    let message = text.find(code).map_or(text, |start| &text[start..]);
    let message = message.trim_end_matches(['"', '}', ' ', '\n']);
    axum::response::IntoResponse::into_response((
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "ok": false,
            "error": { "code": code, "message": message },
        })),
    ))
}

#[cfg(test)]
mod tests;
