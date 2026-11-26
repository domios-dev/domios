use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crown::{AuthenticatedAgent, AuthenticatedOrigin};

use super::dedupe::DedupeKey;

pub const OUTBOX_FULL: &str = "outbox_full";
pub const PAYLOAD_TOO_LARGE: &str = crate::contract::delivery::PAYLOAD_TOO_LARGE;
pub const DELIVERY_UNCONFIRMED: &str = "delivery_unconfirmed";
pub const REFUSED_BEFORE_WRITE: &str = "refused_before_write";
pub const DEDUPE_CONFLICT: &str = "dedupe_digest_conflict";
pub const DEDUPE_CAPACITY_FULL: &str = "dedupe_capacity_full";

const MAX_ID_LEN: usize = 128;
const DIGEST_DOMAIN: &[u8] = b"domios-cloud-outbox-v1";

pub const DEFAULT_MAX_RECORDS: usize = 64;
pub const DEFAULT_MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_MAX_RECORD_BYTES: usize = 1024 * 1024;
pub const DEFAULT_MAX_RETAINED_SETTLED: usize = 256;
pub const DEFAULT_MAX_RETAINED_UNCONFIRMED: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutboxId(String);

impl OutboxId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn parse(raw: impl Into<String>) -> Result<Self, OutboxError> {
        let value = raw.into();
        if value.is_empty()
            || value.len() > MAX_ID_LEN
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(OutboxError::InvalidRequest {
                reason: "outbox id must be 1..=128 ASCII [A-Za-z0-9_-] characters".into(),
            });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for OutboxId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OutboxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstallationPair {
    pub source_installation_id: String,
    pub target_installation_id: String,
}

impl InstallationPair {
    pub fn new(
        source_installation_id: impl Into<String>,
        target_installation_id: impl Into<String>,
    ) -> Self {
        Self {
            source_installation_id: source_installation_id.into(),
            target_installation_id: target_installation_id.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundOperation {
    Announce,
    WorkerAck,
    WorkerReport,
    Notification,
    PlanRpc,
}

impl OutboundOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Announce => "announce",
            Self::WorkerAck => "worker_ack",
            Self::WorkerReport => "worker_report",
            Self::Notification => "notification",
            Self::PlanRpc => "plan_rpc",
        }
    }

    pub const fn http_method(self) -> &'static str {
        match self {
            Self::Announce
            | Self::WorkerAck
            | Self::WorkerReport
            | Self::Notification
            | Self::PlanRpc => "POST",
        }
    }

    pub const fn allowed_http_methods(self) -> &'static [&'static str] {
        &["POST"]
    }

    pub const fn canonical_route(self) -> &'static str {
        match self {
            Self::Announce => "/api/mesh/announce",
            Self::WorkerAck => "/api/mesh/worker/ack",
            Self::WorkerReport => "/api/mesh/worker/report",
            Self::Notification => "/api/mesh/notification",
            Self::PlanRpc => "/api/mesh/plan/rpc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerContext {
    pub origin: AuthenticatedOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AuthenticatedAgent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

impl CallerContext {
    pub fn accounted_bytes(&self) -> usize {
        self.agent
            .as_ref()
            .map_or(0, |a| a.instance_id.len().saturating_add(a.tab_id.len()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundPeerRequest {
    pub account_id: String,
    pub source_installation_id: String,
    pub target_installation_id: String,
    pub operation: OutboundOperation,
    pub caller_context: CallerContext,
    #[serde(with = "base64_bytes")]
    pub body: Vec<u8>,
}

impl OutboundPeerRequest {
    pub fn validate(&self) -> Result<(), OutboxError> {
        validate_identifier(&self.account_id, "account_id")?;
        validate_identifier(&self.source_installation_id, "source_installation_id")?;
        validate_identifier(&self.target_installation_id, "target_installation_id")?;
        if let Some(agent) = &self.caller_context.agent {
            validate_identifier(&agent.instance_id, "caller_context.agent.instance_id")?;
            validate_identifier(&agent.tab_id, "caller_context.agent.tab_id")?;
        }
        Ok(())
    }

    pub fn pair(&self) -> InstallationPair {
        InstallationPair {
            source_installation_id: self.source_installation_id.clone(),
            target_installation_id: self.target_installation_id.clone(),
        }
    }

    pub fn accounted_bytes(&self) -> usize {
        self.account_id
            .len()
            .saturating_add(self.source_installation_id.len())
            .saturating_add(self.target_installation_id.len())
            .saturating_add(self.caller_context.accounted_bytes())
            .saturating_add(self.body.len())
    }

    pub fn payload_digest(&self) -> String {
        let mut hasher = Sha256::new();
        hash_field(&mut hasher, DIGEST_DOMAIN);
        hash_field(&mut hasher, self.account_id.as_bytes());
        hash_field(&mut hasher, self.source_installation_id.as_bytes());
        hash_field(&mut hasher, self.target_installation_id.as_bytes());
        hash_field(&mut hasher, self.operation.as_str().as_bytes());
        hash_field(&mut hasher, self.operation.http_method().as_bytes());
        hash_field(&mut hasher, self.operation.canonical_route().as_bytes());
        hash_field(&mut hasher, origin_tag(self.caller_context.origin));
        match &self.caller_context.agent {
            Some(agent) => {
                hasher.update([1]);
                hash_field(&mut hasher, agent.instance_id.as_bytes());
                hash_field(&mut hasher, agent.tab_id.as_bytes());
                hasher.update([u8::from(agent.can_spawn)]);
            }
            None => hasher.update([0]),
        }
        match self.caller_context.expires_at_ms {
            Some(expires_at_ms) => {
                hasher.update([1]);
                hasher.update(expires_at_ms.to_be_bytes());
            }
            None => hasher.update([0]),
        }
        hash_field(&mut hasher, &self.body);
        format!("{:x}", hasher.finalize())
    }

    pub fn retargeted(&self, target_installation_id: String) -> Self {
        Self {
            account_id: self.account_id.clone(),
            source_installation_id: self.source_installation_id.clone(),
            target_installation_id,
            operation: self.operation,
            caller_context: self.caller_context.clone(),
            body: self.body.clone(),
        }
    }
}

fn validate_identifier(value: &str, field: &str) -> Result<(), OutboxError> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(OutboxError::InvalidRequest {
            reason: format!("{field} must be non-empty without leading/trailing whitespace or control characters"),
        });
    }
    Ok(())
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

const fn origin_tag(origin: AuthenticatedOrigin) -> &'static [u8] {
    match origin {
        AuthenticatedOrigin::Owner => b"owner",
        AuthenticatedOrigin::Agent => b"agent",
        AuthenticatedOrigin::Mesh => b"mesh",
        AuthenticatedOrigin::Service => b"service",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxFrame {
    pub id: OutboxId,
    pub sequence: u64,
    pub enqueued_at_ms: u64,
    pub payload_digest: String,
    pub request: OutboundPeerRequest,
}

impl OutboxFrame {
    pub fn dedupe_key(&self) -> DedupeKey {
        DedupeKey {
            account_id: self.request.account_id.clone(),
            source_installation_id: self.request.source_installation_id.clone(),
            target_installation_id: self.request.target_installation_id.clone(),
            id: self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxLimits {
    pub max_records: usize,
    pub max_total_bytes: usize,
    pub max_record_bytes: usize,
    pub max_retained_settled: usize,
    pub max_retained_unconfirmed: usize,
}

impl Default for OutboxLimits {
    fn default() -> Self {
        Self {
            max_records: DEFAULT_MAX_RECORDS,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_record_bytes: DEFAULT_MAX_RECORD_BYTES,
            max_retained_settled: DEFAULT_MAX_RETAINED_SETTLED,
            max_retained_unconfirmed: DEFAULT_MAX_RETAINED_UNCONFIRMED,
        }
    }
}

impl OutboxLimits {
    pub fn validate(&self) -> Result<(), OutboxError> {
        if self.max_records == 0 || self.max_total_bytes == 0 || self.max_record_bytes == 0 {
            return Err(OutboxError::InvalidRequest {
                reason: "outbox count and byte limits must be greater than zero".into(),
            });
        }
        if self.max_record_bytes > self.max_total_bytes {
            return Err(OutboxError::InvalidRequest {
                reason: "max_record_bytes cannot exceed max_total_bytes".into(),
            });
        }
        if self.max_retained_unconfirmed == 0 {
            return Err(OutboxError::InvalidRequest {
                reason: "max_retained_unconfirmed must be greater than zero".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxLimitKind {
    Count,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum OutboxError {
    Full {
        limit_kind: OutboxLimitKind,
        current: usize,
        limit: usize,
    },
    PayloadTooLarge {
        bytes: usize,
        max_bytes: usize,
    },
    InvalidRequest {
        reason: String,
    },
    DuplicateId {
        id: OutboxId,
    },
    Storage {
        detail: String,
    },
}

impl OutboxError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Full { .. } => OUTBOX_FULL,
            Self::PayloadTooLarge { .. } => PAYLOAD_TOO_LARGE,
            Self::InvalidRequest { .. } => "invalid_request",
            Self::DuplicateId { .. } => "duplicate_outbox_id",
            Self::Storage { .. } => "outbox_storage_error",
        }
    }
}

impl fmt::Display for OutboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full {
                limit_kind,
                current,
                limit,
            } => write!(
                f,
                "{}: {:?} capacity reached ({current}/{limit})",
                self.code(),
                limit_kind
            ),
            Self::PayloadTooLarge { bytes, max_bytes } => {
                write!(
                    f,
                    "{}: {bytes} bytes exceeds limit {max_bytes}",
                    self.code()
                )
            }
            Self::InvalidRequest { reason } => write!(f, "{}: {reason}", self.code()),
            Self::DuplicateId { id } => write!(f, "{}: {id}", self.code()),
            Self::Storage { detail } => write!(f, "{}: {detail}", self.code()),
        }
    }
}

impl std::error::Error for OutboxError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnconfirmedReason {
    WriteInterrupted,
    AckTimeout,
    StreamClosedBeforeAck,
    ProcessCrashAfterClaim,
    ReceiverStateUncertain,
    TransportUncertain,
}

impl UnconfirmedReason {
    pub const fn stream_likely_broken(self) -> bool {
        matches!(
            self,
            Self::WriteInterrupted | Self::StreamClosedBeforeAck | Self::TransportUncertain
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum StreamDeliveryOutcome {
    Acked {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "opt_base64_bytes"
        )]
        reply_body: Option<Vec<u8>>,
    },
    NotWritten {
        reason: String,
    },
    Unconfirmed {
        reason: UnconfirmedReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes_written: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl StreamDeliveryOutcome {
    pub fn acked(reply_body: Option<Vec<u8>>) -> Self {
        Self::Acked { reply_body }
    }

    pub fn not_written(reason: impl Into<String>) -> Self {
        Self::NotWritten {
            reason: reason.into(),
        }
    }

    pub fn unconfirmed_write_completed(reason: UnconfirmedReason, detail: Option<String>) -> Self {
        Self::Unconfirmed {
            reason,
            bytes_written: Some(true),
            detail,
        }
    }

    pub fn unconfirmed_unknown(reason: UnconfirmedReason, detail: Option<String>) -> Self {
        Self::Unconfirmed {
            reason,
            bytes_written: None,
            detail,
        }
    }

    // Unconfirmed outcomes never claim zero bytes written.
    pub const fn normalize_unconfirmed_bytes_written(bytes_written: Option<bool>) -> Option<bool> {
        match bytes_written {
            Some(true) => Some(true),
            Some(false) | None => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxEnqueueReceipt {
    pub id: OutboxId,
    pub sequence: u64,
    pub enqueued_at_ms: u64,
    pub accounted_bytes: usize,
    pub payload_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedApplicationReply {
    pub id: OutboxId,
    pub completed_at_ms: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "opt_base64_bytes"
    )]
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OutboxRecordStatus {
    Queued {
        sequence: u64,
        enqueued_at_ms: u64,
        bytes_written: Option<bool>,
    },
    InFlight {
        sequence: u64,
        claimed_at_ms: u64,
        stream_id: u64,
        bytes_written: Option<bool>,
    },
    Delivered {
        sequence: u64,
        delivered_at_ms: u64,
        bytes_written: Option<bool>,
        has_application_reply: bool,
    },
    DeliveryUnconfirmed {
        sequence: u64,
        reason: UnconfirmedReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes_written: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        settled_at_ms: u64,
    },
    RefusedBeforeWrite {
        sequence: u64,
        reason: String,
        bytes_written: Option<bool>,
        settled_at_ms: u64,
    },
}

impl OutboxRecordStatus {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Queued { .. } => "queued",
            Self::InFlight { .. } => "in_flight",
            Self::Delivered { .. } => "delivered",
            Self::DeliveryUnconfirmed { .. } => DELIVERY_UNCONFIRMED,
            Self::RefusedBeforeWrite { .. } => REFUSED_BEFORE_WRITE,
        }
    }

    pub const fn bytes_written(&self) -> Option<bool> {
        match self {
            Self::Queued { bytes_written, .. }
            | Self::InFlight { bytes_written, .. }
            | Self::Delivered { bytes_written, .. }
            | Self::DeliveryUnconfirmed { bytes_written, .. }
            | Self::RefusedBeforeWrite { bytes_written, .. } => *bytes_written,
        }
    }

    pub const fn is_unconfirmed(&self) -> bool {
        matches!(self, Self::DeliveryUnconfirmed { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OutboxSnapshot {
    pub queued_count: usize,
    pub in_flight_count: usize,
    pub active_bytes: usize,
    pub unconfirmed_count: usize,
    pub attached_stream_count: usize,
}

pub(crate) mod base64_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    const B64: base64::engine::general_purpose::GeneralPurpose =
        base64::engine::general_purpose::STANDARD;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&B64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        B64.decode(encoded.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) mod opt_base64_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    const B64: base64::engine::general_purpose::GeneralPurpose =
        base64::engine::general_purpose::STANDARD;

    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(value) => serializer.serialize_some(&B64.encode(value)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        let encoded = Option::<String>::deserialize(deserializer)?;
        match encoded {
            Some(value) => B64
                .decode(value.as_bytes())
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}
