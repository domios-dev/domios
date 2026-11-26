use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::account_mesh::ports::BoxFuture;

use super::types::{
    CompletedApplicationReply, InstallationPair, OutboundPeerRequest, OutboxEnqueueReceipt,
    OutboxError, OutboxFrame, OutboxId, OutboxRecordStatus, OutboxSnapshot, StreamDeliveryOutcome,
};

pub trait OutboxClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemOutboxClock;

impl OutboxClock for SystemOutboxClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum DrainPolicyDecision {
    Allow,
    Defer { reason: String },
    Refuse { reason: String },
}

pub trait DrainAuthorizationPolicy: Send + Sync {
    fn authorize_drain(&self, frame: &OutboxFrame, now_ms: u64) -> DrainPolicyDecision;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ExpiryOnlyPolicy;

impl DrainAuthorizationPolicy for ExpiryOnlyPolicy {
    fn authorize_drain(&self, frame: &OutboxFrame, now_ms: u64) -> DrainPolicyDecision {
        if let Some(expires_at_ms) = frame.request.caller_context.expires_at_ms {
            if now_ms >= expires_at_ms {
                return DrainPolicyDecision::Refuse {
                    reason: "request_expired".into(),
                };
            }
        }
        DrainPolicyDecision::Allow
    }
}

pub trait DrainLifecycleObserver: Send + Sync {
    fn on_enqueued(&self, _id: &OutboxId) {}
    fn on_claimed_persisted(&self, _id: &OutboxId, _stream_id: u64) {}
    fn on_delivered_before_settle(&self, _id: &OutboxId, _outcome: &StreamDeliveryOutcome) {}
    fn on_settled(&self, _id: &OutboxId, _status: &OutboxRecordStatus) {}
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopDrainObserver;

impl DrainLifecycleObserver for NoopDrainObserver {}

pub trait ReverseStreamSink: Send + Sync {
    fn stream_id(&self) -> u64;
    fn account_id(&self) -> &str;
    fn local_installation_id(&self) -> &str;
    fn peer_installation_id(&self) -> &str;
    fn deliver<'a>(&'a self, frame: &'a OutboxFrame) -> BoxFuture<'a, StreamDeliveryOutcome>;

    fn pair(&self) -> InstallationPair {
        InstallationPair::new(self.local_installation_id(), self.peer_installation_id())
    }
}

pub trait CloudOutboxPort: Send + Sync {
    fn enqueue(
        &self,
        request: OutboundPeerRequest,
    ) -> BoxFuture<'_, Result<OutboxEnqueueReceipt, OutboxError>>;

    fn enqueue_broadcast(
        &self,
        base: OutboundPeerRequest,
        target_installation_ids: Vec<String>,
    ) -> BoxFuture<'_, Result<Vec<OutboxEnqueueReceipt>, OutboxError>>;

    fn attach_reverse_stream(&self, sink: Arc<dyn ReverseStreamSink>) -> BoxFuture<'_, ()>;

    fn detach_reverse_stream(&self, stream_id: u64);

    fn notify_policy_changed(&self);

    fn record_status(&self, id: &OutboxId) -> Option<OutboxRecordStatus>;

    fn application_reply(&self, id: &OutboxId) -> Option<CompletedApplicationReply>;

    fn unconfirmed_records(&self) -> Vec<(OutboxId, OutboxRecordStatus)>;

    fn dismiss_unconfirmed(&self, id: &OutboxId) -> BoxFuture<'_, Result<bool, OutboxError>>;

    fn snapshot(&self) -> OutboxSnapshot;
}
