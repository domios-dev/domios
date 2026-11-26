pub mod contract;
pub mod dedupe;
pub mod model;
pub mod outbox;
pub mod ports;
pub mod storage;
pub mod types;

pub use contract::{
    is_allowed_plan_rpc_method, validate_operation_request, OperationRouteBinding,
    OuterWireRequest, RouteTarget, INNER_NOTIFICATION_ROUTE, INNER_RPC_ROUTE, OUTER_ANNOUNCE_ROUTE,
    OUTER_RELAY_ROUTE, OUTER_RPC_ROUTE, WORKER_ACK_RPC_METHOD, WORKER_REPORT_RPC_METHOD,
};
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
