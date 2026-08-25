//! Proof of life for one registry member.
//!
//! A notification goes in at the same seam the daemon uses, and a Telegram
//! `sendMessage` request comes out the far end at an HTTP endpoint this test
//! owns. Every hop between is production code: the registry resolves the
//! channel, the channel resolves its audience from config, the shared renderer
//! builds the text, the store admits the push, the channel hands a real
//! `OutboundMessage` to the real bridge outbound queue, and the real
//! `TelegramService` turns it into the API call. Only Telegram's own servers
//! are stood in for.

use std::sync::Arc;

use codeterm_bridge::bridge::manager::{create_outbound_channel, OutboundMessage, OutboundSender};
use codeterm_bridge::bridge::telegram::TelegramService;
use codeterm_bridge::bridge::traits::OutboundContent;
use codeterm_types::config::schema::{AgentLevelChoice, AppConfig, PushChannelConfig};
use parking_lot::Mutex as PLMutex;

use super::*;
use crate::notification::channels::{self, ChannelHost};
use crate::notification::push;
use crate::notification::store::{NotificationProvenance, NotificationStore};
use crate::service::notification::{
    create_notification_with_delivery, NotificationAdmission, NotificationOrigin, PushDispatch,
    SurfaceDispatch,
};

const CHAT_ID: i64 = 100000001;

/// The daemon's own host, minus the supervisor handle a running app owns: the
/// outbound queue is the production one, created by the production constructor.
struct BridgeBackedHost {
    outbound: OutboundSender,
}

impl ChannelHost for BridgeBackedHost {
    fn service_ready(&self, service: &str) -> bool {
        codeterm_bridge::services::find(service).is_some_and(|descriptor| descriptor.outbound)
            && self.outbound.lock().is_some()
    }

    fn enqueue(&self, service: &str, message: OutboundMessage) -> Result<(), String> {
        if !self.service_ready(service) {
            return Err(format!("the {service} outbound channel is closed"));
        }
        let outbound = self.outbound.lock();
        let tx = outbound
            .as_ref()
            .ok_or_else(|| format!("the {service} outbound channel is closed"))?;
        tx.send(message)
            .map_err(|_| format!("the {service} outbound channel rejected the message"))
    }
}

/// A stand-in for Telegram's API that records what actually arrived.
async fn spawn_api_stand_in(captured: Arc<PLMutex<Vec<(String, serde_json::Value)>>>) -> String {
    use axum::{body, http::Request, Json, Router};

    let app = Router::new().fallback(move |req: Request<body::Body>| {
        let captured = captured.clone();
        async move {
            let path = req.uri().path().to_string();
            let bytes = body::to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            captured.lock().push((path, body));
            Json(serde_json::json!({"ok": true, "result": {"message_id": 42}}))
        }
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn opted_in_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.notifications.agent_levels = NotifyLevel::ALL
        .into_iter()
        .map(|level| AgentLevelChoice::open(level.as_str(), &["telegram"]))
        .collect();
    cfg.notifications.channels.insert(
        "telegram".to_string(),
        PushChannelConfig {
            enabled: true,
            recipients: vec![CHAT_ID.to_string()],
            per_scope_limit: 12,
        },
    );
    cfg
}

#[tokio::test]
async fn push_delivery_reaches_the_send_endpoint_end_to_end() {
    let captured: Arc<PLMutex<Vec<(String, serde_json::Value)>>> =
        Arc::new(PLMutex::new(Vec::new()));
    let api_base = spawn_api_stand_in(captured.clone()).await;

    let cfg = opted_in_config();
    let (outbound, mut inbox) = create_outbound_channel();
    let host = BridgeBackedHost { outbound };

    // The registry resolves the channel and the channel resolves its audience.
    let plan = plan_push(
        &cfg,
        &channels::roster(),
        &host,
        PushIntent {
            level: Some(NotifyLevel::Failure),
            event_key: Some("release-42".to_string()),
        },
    );
    assert_eq!(
        plan.targets.len(),
        1,
        "exactly the opted-in channel is routed"
    );
    assert_eq!(plan.targets[0].service, "telegram");
    assert!(plan.refusals.is_empty());
    assert_eq!(plan.per_scope_limit, 12);

    // The same entry point the daemon calls, with the real dispatch.
    let store = Arc::new(PLMutex::new(NotificationStore::new(10)));
    let title = "release build failed";
    let body = "the size gate flagged one file";
    let targets = plan.targets;
    let refusals = plan.refusals;
    let id = create_notification_with_delivery(
        NotificationAdmission::Explicit,
        &store,
        |_, _| {},
        title.to_string(),
        Some(body.to_string()),
        Some("error"),
        None,
        None,
        NotificationOrigin::default(),
        |_| None,
        PushDispatch {
            intent: plan.intent,
            per_scope_limit: plan.per_scope_limit,
            deliver: |envelope: PushEnvelope| deliver_plan(&targets, &refusals, &host, &envelope),
        },
        SurfaceDispatch::silent(),
        |_| {},
    );
    assert!(!id.is_empty());

    // A real outbound message landed on the real bridge queue.
    let queued = inbox.recv().await.expect("the outbound queue carried it");
    assert_eq!(queued.user_id, Some(CHAT_ID), "the send names its chat id");
    assert!(!queued.append);
    let OutboundContent::Text { text } = queued.content else {
        panic!("a push is text");
    };
    let expected = push::render_push(
        NotifyLevel::Failure,
        title,
        Some(body),
        &NotificationProvenance::default(),
    );
    assert_eq!(text, expected, "the queued text is the rendered push");

    // And that message becomes the API call Telegram would receive.
    let telegram = TelegramService::with_base(
        "test-token".to_string(),
        api_base,
        crate::bridge_host::attachment_store(),
    );
    telegram
        .send_message(CHAT_ID, &text)
        .await
        .expect("the send completes");

    let seen = captured.lock().clone();
    assert_eq!(seen.len(), 1, "one request reached the endpoint");
    let (path, payload) = &seen[0];
    assert!(
        path.ends_with("/sendMessage"),
        "the notification arrived as a message send, got {path}"
    );
    assert_eq!(payload["chat_id"], serde_json::json!(CHAT_ID));
    assert_eq!(payload["text"], serde_json::json!(expected));
    assert!(
        payload["text"]
            .as_str()
            .is_some_and(|sent| sent.contains(title) && sent.contains(body)),
        "the agent's own words reached the far end"
    );
}
