#[cfg(feature = "runtime")]
pub mod adapter;
pub mod crypto;
pub mod gateway;
pub mod keys;
pub mod permissions;
pub mod receive;
pub mod registry;
pub mod supervisor;
pub mod wire;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidEnvelope,
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
    Refused,
    ReceiverElsewhere,
    ReceiverRevisionStale,
    NotReceiver,
    RecipientNotReceiving,
    InstallationUnverified,
    SessionRevoked,
    OperationIdConflict,
    Disabled,
    DailyLimit { limit: u64, resets_at: u64 },
    LimitOutOfRange,
    Firestore { code: i32, operation: &'static str },
    ListenProtocol { kind: &'static str },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::Firestore { code, operation } = self {
            return write!(f, "Firestore {operation} failed (gRPC code {code})");
        }
        if let Self::ListenProtocol { kind } = self {
            return write!(f, "Firestore Listen failed ({kind})");
        }
        if let Self::DailyLimit { limit, resets_at } = self {
            return write!(
                f,
                "neighbor_daily_limit: {{\"limit\":{limit},\"resets_at\":{resets_at}}}"
            );
        }
        f.write_str(match self {
            Self::InvalidEnvelope => "Invalid Neighbor envelope",
            Self::InvalidKey => "Invalid Neighbor key",
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
            Self::Revoked => "Neighbor installation key revoked",
            Self::KeyNotFound => "Neighbor installation key not found",
            Self::ReceiverElsewhere => "receiver_elsewhere: Receiving on another machine",
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
            Self::Refused => "Neighbor request refused",
            Self::Disabled => "neighbors_disabled: Neighbors is turned off on this instance",
            Self::LimitOutOfRange => "limit_out_of_range: Choose a valid daily message limit",
            Self::Firestore { .. } | Self::DailyLimit { .. } | Self::ListenProtocol { .. } => unreachable!(),
        })
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
