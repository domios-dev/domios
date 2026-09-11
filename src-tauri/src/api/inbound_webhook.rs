use axum::{
    body::Bytes,
    extract::{Path, State as AxumState},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use codeterm_bridge::ports::BridgeTabPort;
use serde::Serialize;
use serde_json::Value;

use crate::api::routes::AppState;
use crate::bridge_host::ServiceBridgeTabPort;
use crate::plugin::{WebhookDelivery, WebhookReceiveContext, WebhookReceiver, WebhookReceiverKind};
use crate::secrets::SecretStore;
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::messenger::MessageEnvelope;

pub(crate) const PEBBLE_WEBHOOK_SECRET: &str = "pebble_webhook_secret";
pub(crate) const MAX_WEBHOOK_BODY_BYTES: usize = 1024 * 1024;
const MAX_DELIVERY_TEXT_BYTES: usize = 64 * 1024;
const MAX_EVENT_ID_BYTES: usize = 256;

#[derive(Debug, Clone, Copy)]
struct WebhookRejection {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl WebhookRejection {
    const fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "Webhook authorization required",
        }
    }

    const fn auth_unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "authentication_unavailable",
            message: "Webhook authentication is unavailable",
        }
    }

    const fn invalid_json() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_json",
            message: "Request body must be valid JSON",
        }
    }

    const fn unsupported_media() -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            code: "unsupported_media_type",
            message: "Request body must use JSON media type",
        }
    }

    const fn body_too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "body_too_large",
            message: "Request body exceeds the webhook limit",
        }
    }

    const fn receiver_not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "webhook_receiver_not_found",
            message: "Plugin webhook receiver not found",
        }
    }

    const fn receiver_failed() -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            code: "webhook_receiver_failed",
            message: "Plugin webhook receiver failed",
        }
    }

    const fn general_agent_missing() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "general_agent_unavailable",
            message: "General Agent is unavailable",
        }
    }

    const fn delivery_failed() -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            code: "delivery_failed",
            message: "General Agent delivery failed",
        }
    }

    const fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "webhook_internal_error",
            message: "Webhook request could not be completed",
        }
    }
}

impl IntoResponse for WebhookRejection {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({
                "ok": false,
                "error": {
                    "code": self.code,
                    "message": self.message,
                },
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookResponse {
    ok: bool,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_id: Option<String>,
}

fn presented_secret(headers: &HeaderMap) -> Result<&str, WebhookRejection> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(WebhookRejection::unauthorized)?;
    let secret = value
        .strip_prefix("Bearer ")
        .filter(|secret| !secret.is_empty())
        .ok_or_else(WebhookRejection::unauthorized)?;
    Ok(secret)
}

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<(), WebhookRejection> {
    let presented = presented_secret(headers)?;
    let stored = {
        let store = {
            let guard = state.svc.secrets.read().await;
            guard.clone()
        };
        store.get(PEBBLE_WEBHOOK_SECRET).await
    }
    .map_err(|_| WebhookRejection::auth_unavailable())?;
    let expected = stored.value.expose_secret();
    if !shared_secret_matches(presented, expected) {
        return Err(WebhookRejection::unauthorized());
    }
    Ok(())
}

fn shared_secret_matches(presented: &str, expected: &str) -> bool {
    !expected.is_empty()
        && crate::service::tokens::constant_time_eq(presented.as_bytes(), expected.as_bytes())
}

fn decode_json(headers: &HeaderMap, body: &Bytes) -> Result<Value, WebhookRejection> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim());
    let json_media_type = content_type.is_some_and(|value| {
        value == "application/json"
            || (value.starts_with("application/") && value.ends_with("+json"))
    });
    if !json_media_type {
        return Err(WebhookRejection::unsupported_media());
    }
    serde_json::from_slice(body).map_err(|_| WebhookRejection::invalid_json())
}

fn registered_receiver(
    registry: &codeterm_plugin::WebhookRegistry,
    plugin_id: &str,
) -> Result<std::sync::Arc<dyn WebhookReceiver>, WebhookRejection> {
    registry
        .get(plugin_id)
        .filter(|receiver| receiver.kind() == WebhookReceiverKind::Pebble)
        .ok_or_else(WebhookRejection::receiver_not_found)
}

fn receive_plugin(
    receiver: &dyn WebhookReceiver,
    context: WebhookReceiveContext,
) -> Result<Option<WebhookDelivery>, WebhookRejection> {
    receiver
        .receive(context)
        .map_err(|_| WebhookRejection::receiver_failed())
}

fn receive_and_deliver(
    svc: crate::service::ServiceState,
    plugin_id: String,
    receiver: std::sync::Arc<dyn WebhookReceiver>,
    context: WebhookReceiveContext,
) -> Result<WebhookResponse, WebhookRejection> {
    let delivery = receive_plugin(receiver.as_ref(), context)?;
    let Some(delivery) = delivery else {
        return Ok(WebhookResponse {
            ok: true,
            status: "noop",
            event_id: None,
        });
    };
    if delivery.text.len() > MAX_DELIVERY_TEXT_BYTES
        || delivery
            .event_id
            .as_ref()
            .is_some_and(|event_id| event_id.len() > MAX_EVENT_ID_BYTES)
    {
        return Err(WebhookRejection::receiver_failed());
    }

    let target = ServiceBridgeTabPort(svc.clone())
        .resolve_general_tab()
        .map_err(|_| WebhookRejection::general_agent_missing())?;
    let event_id = delivery.event_id.filter(|event_id| !event_id.is_empty());
    let idempotency_key = event_id
        .as_deref()
        .map(|event_id| format!("pebble:{plugin_id}:{event_id}"));
    let receipt = svc
        .gateway
        .submit(
            svc.as_ref(),
            OutboundMessage {
                target,
                body: MessageBody::Text {
                    body: delivery.text,
                },
                envelope: Some(MessageEnvelope::pebble()),
                class: DeliveryClass::SystemNotice,
                origin: MessageOrigin::ServiceToken,
                idempotency_key,
            },
        )
        .map_err(|_| WebhookRejection::delivery_failed())?;
    Ok(WebhookResponse {
        ok: true,
        status: if receipt.dedup_hit {
            "noop"
        } else {
            "delivered"
        },
        event_id,
    })
}

pub async fn receive_webhook(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(plugin_id): Path<String>,
    body: Bytes,
) -> Result<Json<WebhookResponse>, WebhookRejection> {
    authenticate(&state, &headers).await?;
    if body.len() > MAX_WEBHOOK_BODY_BYTES {
        return Err(WebhookRejection::body_too_large());
    }
    let payload = decode_json(&headers, &body)?;
    let receiver = registered_receiver(&state.svc.webhook_registry, &plugin_id)?;
    let context = WebhookReceiveContext {
        payload,
        received_at: chrono::Utc::now().to_rfc3339(),
    };
    let svc = state.svc.clone();
    let result =
        tokio::task::spawn_blocking(move || receive_and_deliver(svc, plugin_id, receiver, context))
            .await
            .map_err(|_| WebhookRejection::internal())??;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn bearer_secret_requires_the_authorization_header_form() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("secret"));
        assert!(matches!(
            presented_secret(&headers),
            Err(WebhookRejection {
                status: StatusCode::UNAUTHORIZED,
                ..
            })
        ));

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer shared-secret"),
        );
        assert_eq!(presented_secret(&headers).unwrap(), "shared-secret");
    }

    #[test]
    fn wrong_secret_is_rejected_without_a_url_fallback() {
        assert!(!shared_secret_matches("wrong", "shared-secret"));
        assert_eq!(
            WebhookRejection::unauthorized().status,
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn missing_shared_secret_is_an_authentication_failure() {
        assert!(!shared_secret_matches("shared-secret", ""));
        assert_eq!(
            WebhookRejection::auth_unavailable().status,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn url_tokens_are_not_part_of_webhook_authentication() {
        let headers = HeaderMap::new();
        assert!(presented_secret(&headers).is_err());
    }

    #[test]
    fn json_decode_rejects_invalid_and_non_json_bodies() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert!(matches!(
            decode_json(&headers, &Bytes::from_static(b"{}")),
            Err(WebhookRejection {
                status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
                ..
            })
        ));

        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        assert!(matches!(
            decode_json(&headers, &Bytes::from_static(b"{")),
            Err(WebhookRejection {
                status: StatusCode::BAD_REQUEST,
                ..
            })
        ));
    }

    #[test]
    fn unknown_or_non_receiver_plugins_are_not_dispatchable() {
        let registry = codeterm_plugin::WebhookRegistry::new();
        let error = match registered_receiver(&registry, "unknown") {
            Ok(_) => panic!("unknown plugin must not resolve"),
            Err(error) => error,
        };
        assert_eq!(error.status, StatusCode::NOT_FOUND);
        assert_eq!(error.code, "webhook_receiver_not_found");
    }

    struct ErrorReceiver;

    impl WebhookReceiver for ErrorReceiver {
        fn id(&self) -> &str {
            "error"
        }

        fn kind(&self) -> WebhookReceiverKind {
            WebhookReceiverKind::Pebble
        }

        fn receive(
            &self,
            _context: WebhookReceiveContext,
        ) -> Result<Option<WebhookDelivery>, crate::plugin::WebhookReceiveError> {
            Err(crate::plugin::WebhookReceiveError::Vm(
                crate::plugin::js_runtime::VmError::Script("boom".into()),
            ))
        }
    }

    #[test]
    fn thrown_plugin_is_a_failed_delivery_not_a_noop() {
        let error = receive_plugin(
            &ErrorReceiver,
            WebhookReceiveContext {
                payload: serde_json::json!({"value": 1}),
                received_at: "2026-09-12T00:00:00Z".into(),
            },
        )
        .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.code, "webhook_receiver_failed");
    }

    struct MalformedReceiver;

    impl WebhookReceiver for MalformedReceiver {
        fn id(&self) -> &str {
            "malformed"
        }

        fn kind(&self) -> WebhookReceiverKind {
            WebhookReceiverKind::Pebble
        }

        fn receive(
            &self,
            _context: WebhookReceiveContext,
        ) -> Result<Option<WebhookDelivery>, crate::plugin::WebhookReceiveError> {
            Err(crate::plugin::WebhookReceiveError::Vm(
                crate::plugin::js_runtime::VmError::InvalidJson(
                    "webhookReceive".into(),
                    "missing text".into(),
                ),
            ))
        }
    }

    #[test]
    fn malformed_plugin_result_is_a_failed_delivery_not_a_noop() {
        let error = receive_plugin(
            &MalformedReceiver,
            WebhookReceiveContext {
                payload: serde_json::json!({"value": 1}),
                received_at: "2026-09-12T00:00:00Z".into(),
            },
        )
        .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.code, "webhook_receiver_failed");
    }

    struct SuccessfulReceiver;

    impl WebhookReceiver for SuccessfulReceiver {
        fn id(&self) -> &str {
            "successful"
        }

        fn kind(&self) -> WebhookReceiverKind {
            WebhookReceiverKind::Pebble
        }

        fn receive(
            &self,
            _context: WebhookReceiveContext,
        ) -> Result<Option<WebhookDelivery>, crate::plugin::WebhookReceiveError> {
            Ok(Some(WebhookDelivery {
                text: "focus enabled".into(),
                event_id: Some("event-1".into()),
            }))
        }
    }

    struct NoopReceiver;

    impl WebhookReceiver for NoopReceiver {
        fn id(&self) -> &str {
            "noop"
        }

        fn kind(&self) -> WebhookReceiverKind {
            WebhookReceiverKind::Pebble
        }

        fn receive(
            &self,
            _context: WebhookReceiveContext,
        ) -> Result<Option<WebhookDelivery>, crate::plugin::WebhookReceiveError> {
            Ok(None)
        }
    }

    #[test]
    fn receiver_noop_is_successful_without_general_agent_delivery() {
        let fixture = crate::service::test_fixture::ServiceFixture::new(
            crate::multiplexer::state::MultiplexerState::empty(),
        );
        let result = receive_and_deliver(
            fixture.service.clone(),
            "pebble".into(),
            std::sync::Arc::new(NoopReceiver),
            WebhookReceiveContext {
                payload: serde_json::json!({"replayed": true}),
                received_at: "2026-09-12T00:00:00Z".into(),
            },
        )
        .expect("receiver no-op is a successful response");
        assert_eq!(result.status, "noop");
        assert!(result.event_id.is_none());
    }

    #[test]
    fn missing_general_agent_is_a_non_delivering_failure() {
        let fixture = crate::service::test_fixture::ServiceFixture::new(
            crate::multiplexer::state::MultiplexerState::empty(),
        );
        let result = receive_and_deliver(
            fixture.service.clone(),
            "pebble".into(),
            std::sync::Arc::new(SuccessfulReceiver),
            WebhookReceiveContext {
                payload: serde_json::json!({"value": 1}),
                received_at: "2026-09-12T00:00:00Z".into(),
            },
        );
        let error = result.unwrap_err();
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.code, "general_agent_unavailable");
    }

    #[test]
    fn bounded_webhook_limits_are_explicit() {
        assert_eq!(MAX_WEBHOOK_BODY_BYTES, 1024 * 1024);
        assert!(MAX_DELIVERY_TEXT_BYTES < MAX_WEBHOOK_BODY_BYTES);
        assert!(MAX_EVENT_ID_BYTES < MAX_DELIVERY_TEXT_BYTES);
        assert_eq!(
            WebhookRejection::body_too_large().status,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
