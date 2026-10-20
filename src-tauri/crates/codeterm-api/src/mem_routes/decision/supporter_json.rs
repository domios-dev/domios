use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::Json;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

pub async fn supporter_error_envelope(
    request: Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let supporter = request.uri().path().starts_with("/api/supporter/");
    let mut response = next.run(request).await;
    let status = response.status();
    let json_body = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    if supporter && (status.is_client_error() || status.is_server_error()) && !json_body {
        let (code, message) = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => (
                "authentication_failed",
                "Authenticate with a valid daemon bearer token and retry.",
            ),
            StatusCode::NOT_FOUND => (
                "unknown_adapter",
                "This Supporter route or adapter is unavailable. Refresh and retry.",
            ),
            StatusCode::BAD_REQUEST
            | StatusCode::METHOD_NOT_ALLOWED
            | StatusCode::PAYLOAD_TOO_LARGE
            | StatusCode::UNSUPPORTED_MEDIA_TYPE
            | StatusCode::UNPROCESSABLE_ENTITY => (
                "invalid_request",
                "Check the connection URL, HTTP method and JSON fields, then retry.",
            ),
            _ => (
                "unavailable",
                "The Supporter is unavailable. Refresh and retry.",
            ),
        };
        *response.body_mut() = axum::body::Body::from(
            json!({ "error": { "code": code, "message": message } }).to_string(),
        );
        response
            .headers_mut()
            .remove(axum::http::header::CONTENT_LENGTH);
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
    }
    response
}

pub struct SupporterJson<T>(pub T);

impl<S, T> FromRequest<S> for SupporterJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state).await.map(|Json(value)| Self(value)).map_err(|_| (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "code": "invalid_request", "message": "Send a JSON body with the required connection fields." } })),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        routing::post,
        Router,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn supporter_connections_protocol_and_bearer_errors_are_json_without_echoing_body() {
        for (status, expected) in [
            (StatusCode::UNAUTHORIZED, "authentication_failed"),
            (StatusCode::BAD_REQUEST, "invalid_request"),
            (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        ] {
            let router = Router::new()
                .route(
                    "/api/supporter/connections",
                    post(move || async move { (status, "opaque-private-secret") }),
                )
                .layer(axum::middleware::from_fn(supporter_error_envelope));
            let response = router
                .oneshot(
                    axum::http::Request::post("/api/supporter/connections")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()["content-type"], "application/json");
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["error"]["code"], expected);
            assert!(!String::from_utf8_lossy(&bytes).contains("opaque-private-secret"));
        }
    }

    async fn endpoint(
        SupporterJson(_): SupporterJson<super::super::connection_routes::ConnectBody>,
    ) -> StatusCode {
        StatusCode::NO_CONTENT
    }

    #[tokio::test]
    async fn supporter_connections_error_bodies_json_with_codes() {
        let router = Router::new().route("/", post(endpoint));
        for body in ["", "{", "{\"token\":\"private-token\"}"] {
            let response = router
                .clone()
                .oneshot(
                    axum::http::Request::post("/")
                        .header("Content-Type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response.headers()["content-type"], "application/json");
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["error"]["code"], "invalid_request");
            assert!(!String::from_utf8_lossy(&bytes).contains("private-token"));
        }
        for code in [
            "unavailable",
            "authentication_failed",
            "unknown_adapter",
            "invalid_model",
        ] {
            let (_, Json(value)) = super::super::connection_routes::error(
                codeterm_service_core::service::supporter::connections::ConnectionError {
                    code,
                    message: "fixture",
                },
            );
            assert_eq!(
                value,
                json!({ "error": { "code": code, "message": "fixture" } })
            );
        }
    }
}
