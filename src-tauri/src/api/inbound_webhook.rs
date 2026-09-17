use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::State as AxumState,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use codeterm_bridge::ports::BridgeTabPort;
use serde::Serialize;
#[cfg(test)]
use serde_json::Value;

use crate::api::routes::AppState;
use crate::bridge_host::ServiceBridgeTabPort;
use crate::plugin::{
    WebhookAudio, WebhookDelivery, WebhookReceiveContext, WebhookReceiver, WebhookReceiverKind,
    WebhookRegistry,
};
use crate::secrets::{SecretError, SecretStore};
use crate::service::message_gateway::{
    DeliveryClass, DeliverySeam, MessageBody, MessageGateway, MessageOrigin, OutboundMessage,
};
use crate::service::messenger::MessageEnvelope;

mod audio;
mod payload;

pub(crate) const PEBBLE_WEBHOOK_PATH: &str = "/api/plugins/pebble/webhook";
pub(crate) const PEBBLE_WEBHOOK_SECRET: &str = "pebble_webhook_secret";
pub(crate) const MAX_WEBHOOK_BODY_BYTES: usize = 1024 * 1024;
/// Granted on top of the text body only to a receiver whose capability declares audio.
pub(crate) const MAX_WEBHOOK_AUDIO_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_WEBHOOK_REQUEST_BYTES: usize =
    MAX_WEBHOOK_BODY_BYTES + MAX_WEBHOOK_AUDIO_BYTES;
const MAX_DELIVERY_TEXT_BYTES: usize = 64 * 1024;
const MAX_EVENT_ID_BYTES: usize = 256;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WebhookRejection {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl WebhookRejection {
    const fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }

    const fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Webhook authorization required",
        )
    }

    const fn auth_unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "authentication_unavailable",
            "Webhook authentication is unavailable",
        )
    }

    const fn invalid_json() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "Request body must be valid JSON",
        )
    }

    const fn unsupported_media() -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Use multipart/form-data with a boundary, or application/json",
        )
    }

    const fn body_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_too_large",
            "Request body exceeds the webhook limit",
        )
    }

    const fn receiver_not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "webhook_receiver_not_found",
            "Pebble webhook receiver not found",
        )
    }

    const fn receiver_ambiguous() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "webhook_receiver_ambiguous",
            "More than one Pebble webhook receiver is registered",
        )
    }

    const fn receiver_failed() -> Self {
        Self::new(
            StatusCode::BAD_GATEWAY,
            "webhook_receiver_failed",
            "Pebble webhook receiver failed",
        )
    }

    const fn general_agent_missing() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "general_agent_unavailable",
            "General Agent is unavailable",
        )
    }

    const fn delivery_failed() -> Self {
        Self::new(
            StatusCode::BAD_GATEWAY,
            "delivery_failed",
            "Webhook event delivery failed",
        )
    }

    const fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "webhook_internal_error",
            "Webhook request could not be completed",
        )
    }
}

impl IntoResponse for WebhookRejection {
    fn into_response(self) -> Response {
        log::warn!(
            "[pebble-webhook] request rejected: status={} code={}",
            self.status.as_u16(),
            self.code
        );
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
pub(crate) struct WebhookResponse {
    ok: bool,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_id: Option<String>,
}

impl WebhookResponse {
    fn delivered(event_id: Option<String>) -> Self {
        Self {
            ok: true,
            status: "delivered",
            event_id,
        }
    }

    fn noop(event_id: Option<String>) -> Self {
        Self {
            ok: true,
            status: "noop",
            event_id,
        }
    }
}

fn presented_secret(headers: &HeaderMap) -> Result<&str, WebhookRejection> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|secret| !secret.is_empty())
        .ok_or_else(WebhookRejection::unauthorized)
}

fn secret_error_kind(error: &SecretError) -> &'static str {
    match error {
        SecretError::LoggedOut => "logged_out",
        SecretError::Locked => "locked",
        SecretError::NotFound => "not_configured",
        SecretError::Ambiguous { .. } => "ambiguous",
        SecretError::AlreadyExists => "already_exists",
        SecretError::BadRequest { .. } => "bad_request",
        SecretError::Backend { .. } => "backend_unavailable",
    }
}

async fn authenticate(
    store: &dyn SecretStore,
    headers: &HeaderMap,
) -> Result<(), WebhookRejection> {
    let presented = presented_secret(headers)?;
    let stored = store.get(PEBBLE_WEBHOOK_SECRET).await.map_err(|error| {
        log::warn!(
            "[pebble-webhook] shared secret unavailable: {}",
            secret_error_kind(&error)
        );
        WebhookRejection::auth_unavailable()
    })?;
    let expected = stored.value.expose_secret();
    if expected.is_empty() {
        log::warn!("[pebble-webhook] shared secret unavailable: empty");
        return Err(WebhookRejection::auth_unavailable());
    }
    if !crate::service::tokens::constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
        log::warn!("[pebble-webhook] rejected a request with a mismatched secret");
        return Err(WebhookRejection::unauthorized());
    }
    Ok(())
}

fn pebble_receiver(
    registry: &WebhookRegistry,
) -> Result<Arc<dyn WebhookReceiver>, WebhookRejection> {
    let mut receivers = registry
        .active()
        .into_iter()
        .filter(|receiver| receiver.kind() == WebhookReceiverKind::Pebble);
    let receiver = receivers
        .next()
        .ok_or_else(WebhookRejection::receiver_not_found)?;
    if receivers.next().is_some() {
        log::warn!("[pebble-webhook] more than one plugin registers a pebble receiver");
        return Err(WebhookRejection::receiver_ambiguous());
    }
    Ok(receiver)
}

/// Authenticates before anything else, so an unauthenticated request never reaches a plugin or a body read.
async fn authorize(
    store: &dyn SecretStore,
    registry: &WebhookRegistry,
    headers: &HeaderMap,
) -> Result<Arc<dyn WebhookReceiver>, WebhookRejection> {
    authenticate(store, headers).await?;
    pebble_receiver(registry)
}

fn audio_cap(receiver: &dyn WebhookReceiver) -> Option<usize> {
    receiver.accepts_audio().then_some(MAX_WEBHOOK_AUDIO_BYTES)
}

pub(crate) struct Admitted {
    receiver: Arc<dyn WebhookReceiver>,
    context: WebhookReceiveContext,
    audio: Option<payload::AudioPart>,
}

async fn admit(
    receiver: Arc<dyn WebhookReceiver>,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<Admitted, WebhookRejection> {
    let decoded = payload::decode(headers, body, audio_cap(receiver.as_ref())).await?;
    let context = WebhookReceiveContext {
        payload: decoded.fields,
        received_at: chrono::Utc::now().to_rfc3339(),
        audio: decoded.audio.as_ref().map(|audio| WebhookAudio {
            media_type: audio.media_type.clone(),
            size: audio.bytes.len() as u64,
        }),
    };
    Ok(Admitted {
        receiver,
        context,
        audio: decoded.audio,
    })
}

#[cfg(test)]
async fn accept(
    store: &dyn SecretStore,
    registry: &WebhookRegistry,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<Admitted, WebhookRejection> {
    let receiver = authorize(store, registry, headers).await?;
    admit(receiver, headers, body).await
}

fn sanitize_event_text(text: &str) -> String {
    let mut clean: String = crate::pty::ansi::strip_ansi(text)
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect();
    // One pass can splice a new marker opening out of overlapping fragments.
    while clean.contains(crate::message_markers::OPEN) {
        clean = clean.replace(crate::message_markers::OPEN, "codeterm:");
    }
    clean.trim().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceivedEvent {
    text: String,
    transcript: Option<String>,
    event_id: Option<String>,
}

fn receive_event(
    receiver: &dyn WebhookReceiver,
    context: WebhookReceiveContext,
) -> Result<Option<ReceivedEvent>, WebhookRejection> {
    let delivery = receiver.receive(context).map_err(|error| {
        log::warn!(
            "[pebble-webhook] receiver {} failed: {error}",
            receiver.id()
        );
        WebhookRejection::receiver_failed()
    })?;
    let Some(WebhookDelivery {
        text,
        event_id,
        transcript,
    }) = delivery
    else {
        return Ok(None);
    };
    let event_id = event_id.filter(|event_id| !event_id.is_empty());
    let oversized = text.len() > MAX_DELIVERY_TEXT_BYTES
        || transcript
            .as_ref()
            .is_some_and(|transcript| transcript.len() > MAX_DELIVERY_TEXT_BYTES)
        || event_id
            .as_ref()
            .is_some_and(|event_id| event_id.len() > MAX_EVENT_ID_BYTES);
    let text = sanitize_event_text(&text);
    if oversized || text.is_empty() {
        log::warn!(
            "[pebble-webhook] receiver {} returned an empty or oversized delivery",
            receiver.id()
        );
        return Err(WebhookRejection::receiver_failed());
    }
    Ok(Some(ReceivedEvent {
        text,
        transcript: transcript
            .map(|transcript| sanitize_event_text(&transcript))
            .filter(|transcript| !transcript.is_empty()),
        event_id,
    }))
}

fn deliver_event<S: DeliverySeam>(
    gateway: &MessageGateway,
    seam: &S,
    live_bound_target: impl FnOnce(&str) -> Option<String>,
    resolve_general_agent: impl FnOnce() -> Result<String, String>,
    receiver: &dyn WebhookReceiver,
    event: ReceivedEvent,
    host_audio: Option<&audio::HostAudio>,
) -> Result<WebhookResponse, WebhookRejection> {
    let target = match live_bound_target(receiver.id()) {
        Some(tab_id) => tab_id,
        None => resolve_general_agent().map_err(|error| {
            log::warn!("[pebble-webhook] General Agent unavailable: {error}");
            WebhookRejection::general_agent_missing()
        })?,
    };
    let body = audio::render(&event.text, event.transcript.as_deref(), host_audio);
    let idempotency_key = event
        .event_id
        .as_deref()
        .map(|event_id| format!("pebble:{}:{event_id}", receiver.id()));
    let receipt = gateway
        .submit(
            seam,
            OutboundMessage {
                target,
                body: MessageBody::Text { body },
                envelope: Some(MessageEnvelope::plugin(
                    receiver.id(),
                    receiver.icon_svg().map(str::to_string),
                )),
                class: DeliveryClass::SystemNotice,
                origin: MessageOrigin::ServiceToken,
                idempotency_key,
            },
        )
        .map_err(|error| {
            log::warn!("[pebble-webhook] event delivery failed: {error}");
            WebhookRejection::delivery_failed()
        })?;
    if receipt.dedup_hit {
        return Ok(WebhookResponse::noop(event.event_id));
    }
    Ok(WebhookResponse::delivered(event.event_id))
}

fn task_failed(error: tokio::task::JoinError) -> WebhookRejection {
    log::error!("[pebble-webhook] receiver task did not complete: {error}");
    WebhookRejection::internal()
}

pub(crate) async fn receive_pebble_webhook(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<WebhookResponse>, WebhookRejection> {
    let store = state.svc.secrets.read().await.clone();
    let receiver = authorize(store.as_ref(), &state.svc.webhook_registry, &headers).await?;
    let limit = payload::body_limit(audio_cap(receiver.as_ref()));
    let body = axum::body::to_bytes(body, limit).await.map_err(|error| {
        log::warn!("[pebble-webhook] request body was not read within {limit} bytes: {error}");
        WebhookRejection::body_too_large()
    })?;
    let Admitted {
        receiver,
        context,
        audio,
    } = admit(receiver, &headers, &body).await?;
    drop(body);

    let receiving = receiver.clone();
    let event = tokio::task::spawn_blocking(move || receive_event(receiving.as_ref(), context))
        .await
        .map_err(task_failed)??;
    let Some(event) = event else {
        return Ok(Json(WebhookResponse::noop(None)));
    };

    let host_audio = match audio {
        Some(audio) => {
            let active = tokio::task::spawn_blocking(|| {
                crate::config::load_config()
                    .integrations
                    .voice_transcribe
                    .active
            })
            .await
            .map_err(task_failed)?;
            Some(
                audio::transcribe(
                    &state.svc.transcribe_registry,
                    &active,
                    &crate::api::web_routes::uploads_dir(),
                    audio,
                )
                .await,
            )
        }
        None => None,
    };

    let svc = state.svc.clone();
    let response = tokio::task::spawn_blocking(move || {
        let general_agent = ServiceBridgeTabPort((*svc).clone());
        deliver_event(
            &svc.gateway,
            svc.as_ref(),
            |receiver_id| svc.live_receiver_target(receiver_id),
            || general_agent.resolve_general_tab(),
            receiver.as_ref(),
            event,
            host_audio.as_ref(),
        )
    })
    .await
    .map_err(task_failed)??;
    Ok(Json(response))
}

#[cfg(test)]
mod tests;
