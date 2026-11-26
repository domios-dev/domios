pub mod dedupe;
pub mod model;
pub mod outbox;
pub mod ports;
pub mod storage;
pub mod types;

pub use dedupe::{
    DedupeDecision, DedupeEntryState, DedupeError, DedupeKey, DedupeLimits, DedupeModel,
    DedupeRecord, ReceiverDedupe,
};
pub use model::OutboxModel;
pub use outbox::{CloudOutbox, CloudOutboxConfig};
pub use ports::{
    CloudOutboxPort, DrainAuthorizationPolicy, DrainLifecycleObserver, DrainPolicyDecision,
    ExpiryOnlyPolicy, NoopDrainObserver, OutboxClock, ReverseStreamSink, SystemOutboxClock,
};
pub use storage::FileOutboxStore;
pub use types::{
    CallerContext, CompletedApplicationReply, InstallationPair, OutboundOperation,
    OutboundPeerRequest, OutboxEnqueueReceipt, OutboxError, OutboxFrame, OutboxId, OutboxLimitKind,
    OutboxLimits, OutboxRecordStatus, OutboxSnapshot, StreamDeliveryOutcome, UnconfirmedReason,
    DEDUPE_CAPACITY_FULL, DEDUPE_CONFLICT, DELIVERY_UNCONFIRMED, OUTBOX_FULL, PAYLOAD_TOO_LARGE,
    REFUSED_BEFORE_WRITE,
};

#[cfg(test)]
mod tests;
