use super::*;
use crate::{EventBroadcaster, SystemEvent};

fn hook(id: usize) -> WebhookRegistration {
    WebhookRegistration {
        id: format!("hook-{id}"),
        url: "http://unused.invalid".into(),
        events: Vec::new(),
        headers: HashMap::new(),
        tab_id: None,
        once: false,
        payload_extra: None,
    }
}

#[tokio::test]
async fn held_webhook_delivery_is_bounded_and_does_not_block_the_event_ring() {
    let broadcaster = EventBroadcaster::new();
    let (sender, mut held) = mpsc::channel(MAX_DELIVERIES);
    let task = tokio::spawn(std::future::pending::<()>());
    *broadcaster.delivery.worker.lock() = Some((sender, task.abort_handle()));
    for id in 0..MAX_DELIVERIES + 1 {
        broadcaster.register_webhook(hook(id)).unwrap();
    }
    broadcaster.broadcast("fixture", serde_json::json!({"value": 1}));
    assert_eq!(
        broadcaster.delivery.pending.lock().hooks.len(),
        MAX_DELIVERIES
    );
    assert_eq!(broadcaster.replay_stats().webhook_refusals, 1);
    broadcaster.broadcast("fixture", serde_json::json!({"value": 2}));
    assert_eq!(
        broadcaster.delivery.pending.lock().hooks.len(),
        MAX_DELIVERIES
    );
    assert_eq!(
        broadcaster.replay_stats().webhook_refusals,
        MAX_DELIVERIES as u64 + 2
    );
    broadcaster.publish_generic("other", serde_json::Value::Null);
    assert_eq!(broadcaster.snapshot_ring().entries.len(), 1);
    assert!(matches!(
        broadcaster.snapshot_ring().entries[0].event,
        SystemEvent::Generic { .. }
    ));
    while let Ok(delivery) = held.try_recv() {
        drop(delivery);
    }
    assert!(broadcaster.delivery.pending.lock().hooks.is_empty());
    assert_eq!(broadcaster.delivery.pending.lock().bytes, 0);
}

#[test]
fn webhook_registration_is_bounded_before_retaining_payloads() {
    let broadcaster = EventBroadcaster::new();
    for id in 0..MAX_WEBHOOKS {
        broadcaster.register_webhook(hook(id)).unwrap();
    }
    assert!(broadcaster.register_webhook(hook(MAX_WEBHOOKS)).is_err());
    let mut oversized = hook(0);
    oversized.payload_extra = Some(serde_json::json!({"text": "x".repeat(MAX_REGISTRATION_BYTES)}));
    assert!(broadcaster.register_webhook(oversized).is_err());
    assert_eq!(broadcaster.list_webhooks().len(), MAX_WEBHOOKS);
}
