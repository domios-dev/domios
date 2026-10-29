#[cfg(feature = "runtime")]
pub mod adapter;
pub mod crypto;
pub mod files;
pub mod gateway;
pub mod keys;
pub mod permissions;
pub mod receive;
pub mod registry;
pub mod supervisor;
pub mod wire;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    File(files::refusal::Failure),
    InvalidEnvelope,
    MessageTooLarge { limit: u64 },
    RefusalTooLong { limit: u64, actual: u64 },
    ReplyTargetUnknown,
    ReplyTargetStopped,
    EmptyBody,
    TabRequestsNotAllowed,
    PermissionDenied,
    MachineAccessUnsupported,
    MemoryAreaUnknown,
    InvalidKey,
    BadSignature,
    WrongPeer,
    Expired,
    Decryption,
    LocalKeyUnavailable,
    SecretStore,
    Transport,
    CursorStore,
    Gateway,
    Locked,
    Revoked,
    KeyNotFound,
    KeyLimit { limit: u64, active: u64 },
    FeatureUnsupported,
    EntitlementsUnavailable,
    Refused,
    ReceiverElsewhere,
    ReceivingElsewhere,
    ReceiverNotChosen,
    ReceiverRevisionStale,
    NotReceiver,
    RecipientNotReceiving,
    RecipientPaused,
    InstallationUnverified,
    SessionRevoked,
    OperationIdConflict,
    Disabled,
    DailyLimit { limit: u64, resets_at: u64 },
    AggregateDailyLimited { limit: u64, resets_at: u64 },
    LimitOutOfRange,
    InvalidRequest,
    FriendshipNotFound,
    FriendshipEnded,
    RecipientLocked,
    RateLimited { retry_after_seconds: u64 },
    AggregateLimited { retry_after_seconds: u64 },
    Firestore { code: i32, operation: &'static str },
    ListenProtocol { kind: &'static str },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::File(failure) = self {
            return std::fmt::Display::fmt(failure, f);
        }
        if let Self::MessageTooLarge { limit } = self {
            return write!(f, "message_too_large: {{\"limit\":{limit}}}");
        }
        if let Self::KeyLimit { limit, active } = self {
            return write!(f, "key_limit: {{\"limit\":{limit},\"active\":{active}}}");
        }
        if let Self::RefusalTooLong { limit, actual } = self {
            return write!(
                f,
                "refusal_too_long: {{\"limit\":{limit},\"actual\":{actual}}}"
            );
        }
        if let Self::Firestore { code, operation } = self {
            return write!(f, "Firestore {operation} failed (gRPC code {code})");
        }
        if let Self::ListenProtocol { kind } = self {
            return write!(f, "Firestore Listen failed ({kind})");
        }
        if let Self::DailyLimit { limit, resets_at }
        | Self::AggregateDailyLimited { limit, resets_at } = self
        {
            let reason = if matches!(self, Self::DailyLimit { .. }) {
                "neighbor_daily_limit"
            } else {
                "aggregate_daily_limited"
            };
            return write!(
                f,
                "{reason}: {{\"limit\":{limit},\"resets_at\":{resets_at}}}"
            );
        }
        let window = match self {
            Self::RateLimited {
                retry_after_seconds,
            } => Some(("rate_limited", retry_after_seconds)),
            Self::AggregateLimited {
                retry_after_seconds,
            } => Some(("aggregate_limited", retry_after_seconds)),
            _ => None,
        };
        if let Some((reason, seconds)) = window {
            return write!(f, "{reason}: {{\"retry_after_seconds\":{seconds}}}");
        }
        f.write_str(match self {
            Self::InvalidEnvelope => "Invalid Neighbor envelope",
            Self::ReplyTargetStopped => "reply_target_stopped: The owner stopped this request; nothing was sent",
            Self::EmptyBody => "empty_body: The message is empty; nothing was sent. Provide text or an attachment.",
            Self::ReplyTargetUnknown => "reply_target_unknown: Nothing was sent. Reply to an incoming message in this friendship's local history",
            Self::TabRequestsNotAllowed => "tab_requests_not_allowed: Tab requests are not allowed for this Neighbor",
            Self::PermissionDenied => "neighbor_permission_denied: This request is outside the Neighbor permission grant",
            Self::MachineAccessUnsupported => "neighbor_machine_access_unsupported: This concierge launch cannot enforce the requested machine access on this platform",
            Self::MemoryAreaUnknown => "neighbor_memory_area_unknown: Choose memory areas from the current taxonomy",
            Self::InvalidKey => "invalid_key: Invalid Neighbor key",
            Self::BadSignature => "Invalid Neighbor signature",
            Self::WrongPeer => "Neighbor identity mismatch",
            Self::Expired => "Neighbor message expired",
            Self::Decryption => "Cannot decrypt Neighbor message",
            Self::LocalKeyUnavailable => "Neighbor recipient key recovery required",
            Self::SecretStore => "Neighbor OS secret store unavailable",
            Self::Transport => "Neighbor transport unavailable",
            Self::CursorStore => "Neighbor cursor storage unavailable",
            Self::Gateway => "Neighbor gateway unavailable",
            Self::Locked => "friends_locked: Upgrade to use your Neighbors channel",
            Self::Revoked => "key_revoked: Neighbor installation key revoked",
            Self::KeyNotFound => "key_not_found: Neighbor installation key not found",
            Self::FeatureUnsupported => "feature_unsupported: The account receiver must update before moving one friend",
            Self::EntitlementsUnavailable => "entitlements_unavailable: The service could not verify access; try again",
            Self::ReceiverElsewhere => "receiver_elsewhere: Receiving on another machine",
            Self::ReceiverNotChosen => "receiver_not_chosen: No machine has been chosen to receive this friend",
            Self::ReceivingElsewhere => "receiving_elsewhere: This machine is not receiving; no concierge can run here",
            Self::ReceiverRevisionStale => {
                "receiver_revision_stale: Receiving changed; refresh and confirm again"
            }
            Self::NotReceiver => "not_receiver: This machine is not receiving",
            Self::SessionRevoked => "session_revoked: This machine was signed out; sign in again",
            Self::OperationIdConflict => "operation_id_conflict: Receiving operation conflicted; refresh status and confirm again",
            Self::InstallationUnverified => {
                "installation_unverified: Sign in on this installation again"
            }
            Self::RecipientNotReceiving => {
                "recipient_not_receiving: This Neighbor is not receiving right now"
            }
            Self::RecipientPaused => "recipient_paused: This Neighbor paused messages from you",
            Self::Refused => "Neighbor request refused",
            Self::Disabled => "neighbors_disabled: Neighbors is turned off on this instance",
            Self::LimitOutOfRange => "limit_out_of_range: Choose a valid daily message limit",
            Self::InvalidRequest => "invalid_request: Invalid Neighbor request",
            Self::FriendshipNotFound => "friendship_not_found: Neighbor friendship not found",
            Self::FriendshipEnded => "friendship_ended: This Neighbor conversation has ended; refresh the friendship before sending",
            Self::RecipientLocked => "recipient_locked: This Neighbor's plan does not allow the channel",
            Self::File(_) | Self::KeyLimit { .. } | Self::MessageTooLarge { .. } | Self::RefusalTooLong { .. } | Self::Firestore { .. } | Self::DailyLimit { .. } | Self::AggregateDailyLimited { .. } | Self::ListenProtocol { .. } | Self::RateLimited { .. } | Self::AggregateLimited { .. } => unreachable!(),
        })
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
