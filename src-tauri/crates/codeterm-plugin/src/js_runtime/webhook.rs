//! QuickJS adapter for the manifest-declared webhook receiver capability.

use std::sync::Arc;

use crate::js_runtime::manifest::WebhookReceiverKind;
use crate::webhook::{
    WebhookDelivery, WebhookReceiveContext, WebhookReceiveError, WebhookReceiver,
};

use super::vm::{call_json_strict, PluginVm};

pub struct JsWebhookReceiver {
    vm: Arc<PluginVm>,
    id: String,
    kind: WebhookReceiverKind,
    icon_svg: Option<String>,
    accepts_audio: bool,
}

impl JsWebhookReceiver {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>, kind: WebhookReceiverKind) -> Self {
        Self::with_icon(vm, id, kind, None)
    }

    pub fn accepting_audio(mut self, accepts_audio: bool) -> Self {
        self.accepts_audio = accepts_audio;
        self
    }

    pub fn with_icon(
        vm: Arc<PluginVm>,
        id: impl Into<String>,
        kind: WebhookReceiverKind,
        icon_svg: Option<String>,
    ) -> Self {
        Self {
            vm,
            id: id.into(),
            kind,
            icon_svg,
            accepts_audio: false,
        }
    }
}

impl WebhookReceiver for JsWebhookReceiver {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> WebhookReceiverKind {
        self.kind
    }

    fn icon_svg(&self) -> Option<&str> {
        self.icon_svg.as_deref()
    }

    fn accepts_audio(&self) -> bool {
        self.accepts_audio
    }

    fn receive(
        &self,
        ctx: WebhookReceiveContext,
    ) -> Result<Option<WebhookDelivery>, WebhookReceiveError> {
        let ctx = serde_json::to_value(ctx).map_err(WebhookReceiveError::ContextSerialization)?;
        call_json_strict(&self.vm, "webhookReceive", &[ctx]).map_err(WebhookReceiveError::Vm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vm_with(source: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-webhook", source.to_string()).expect("vm"))
    }

    fn context() -> WebhookReceiveContext {
        WebhookReceiveContext {
            payload: json!({"event_id": "evt-1", "transcript": "Focus mode enabled"}),
            received_at: "2026-09-11T07:00:00Z".into(),
            audio: None,
        }
    }

    #[test]
    fn pebble_audio_receiver_context_carries_audio_metadata_and_delivery_carries_transcript() {
        let receiver = receiver(
            r#"module.exports.default = {
                webhookReceive(ctx) {
                    return { text: ctx.audio.mediaType + ":" + ctx.audio.size, transcript: ctx.payload.transcript };
                }
            };"#,
        )
        .accepting_audio(true);
        assert!(receiver.accepts_audio());
        let mut ctx = context();
        ctx.audio = Some(crate::webhook::WebhookAudio {
            media_type: "audio/mp4".into(),
            size: 3,
        });
        let delivery = receiver.receive(ctx).expect("delivery").expect("message");
        assert_eq!(delivery.text, "audio/mp4:3");
        assert_eq!(delivery.transcript.as_deref(), Some("Focus mode enabled"));
    }

    #[test]
    fn pebble_audio_receiver_is_not_audio_capable_by_default_and_context_omits_absent_audio() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive(ctx) { return { text: String("audio" in ctx) }; } };"#,
        );
        assert!(!receiver.accepts_audio());
        let delivery = receiver
            .receive(context())
            .expect("delivery")
            .expect("message");
        assert_eq!(delivery.text, "false");
    }

    #[test]
    fn pebble_audio_receiver_non_string_transcript_is_rejected() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive() { return { text: "ok", transcript: 7 }; } };"#,
        );
        assert!(receiver.receive(context()).is_err());
    }

    fn receiver(source: &str) -> JsWebhookReceiver {
        JsWebhookReceiver::new(vm_with(source), "pebble", WebhookReceiverKind::Pebble)
    }

    #[test]
    fn dispatches_exact_context_and_delivery() {
        let receiver = receiver(
            r#"module.exports.default = {
                webhookReceive(ctx) {
                    return { text: ctx.payload.transcript, eventId: ctx.payload.event_id + ":" + ctx.receivedAt };
                }
            };"#,
        );
        let delivery = receiver
            .receive(context())
            .expect("delivery")
            .expect("message");
        assert_eq!(delivery.text, "Focus mode enabled");
        assert_eq!(
            delivery.event_id.as_deref(),
            Some("evt-1:2026-09-11T07:00:00Z")
        );
        assert_eq!(receiver.kind(), WebhookReceiverKind::Pebble);
    }

    #[test]
    fn null_is_a_deliberate_noop() {
        let receiver =
            receiver(r#"module.exports.default = { webhookReceive() { return null; } };"#);
        assert_eq!(receiver.receive(context()).expect("no-op"), None);
    }

    #[test]
    fn delivery_without_event_id_is_valid() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive() { return { text: "ok" }; } };"#,
        );
        let delivery = receiver
            .receive(context())
            .expect("delivery")
            .expect("message");
        assert_eq!(delivery.text, "ok");
        assert_eq!(delivery.event_id, None);
    }

    #[test]
    fn missing_export_is_an_explicit_error() {
        let receiver = receiver(r#"module.exports.default = {};"#);
        assert!(matches!(
            receiver.receive(context()),
            Err(WebhookReceiveError::Vm(
                crate::js_runtime::VmError::MissingFunction(_)
            ))
        ));
    }

    #[test]
    fn thrown_code_is_an_explicit_error() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive() { throw new Error("boom"); } };"#,
        );
        assert!(matches!(
            receiver.receive(context()),
            Err(WebhookReceiveError::Vm(crate::js_runtime::VmError::Script(
                _
            )))
        ));
    }

    #[test]
    fn promise_return_is_rejected() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive: async function() { return { text: "late" }; } };"#,
        );
        assert!(matches!(
            receiver.receive(context()),
            Err(WebhookReceiveError::Vm(crate::js_runtime::VmError::Script(
                _
            )))
        ));
    }

    #[test]
    fn malformed_return_is_an_explicit_error() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive() { return { eventId: "missing text" }; } };"#,
        );
        assert!(matches!(
            receiver.receive(context()),
            Err(WebhookReceiveError::Vm(
                crate::js_runtime::VmError::InvalidJson(_, _)
            ))
        ));
    }

    #[test]
    fn undefined_return_is_not_an_implicit_noop() {
        let receiver = receiver(r#"module.exports.default = { webhookReceive() {} };"#);
        assert!(receiver.receive(context()).is_err());
    }

    #[test]
    fn unknown_return_fields_are_rejected() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive() { return { text: "ok", extra: true }; } };"#,
        );
        assert!(receiver.receive(context()).is_err());
    }

    #[test]
    fn null_event_id_is_rejected() {
        let receiver = receiver(
            r#"module.exports.default = { webhookReceive() { return { text: "ok", eventId: null }; } };"#,
        );
        assert!(receiver.receive(context()).is_err());
    }
}
