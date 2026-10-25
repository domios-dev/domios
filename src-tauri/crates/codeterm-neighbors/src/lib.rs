#[cfg(feature = "runtime")]
pub mod adapter;
pub mod crypto;
pub mod gateway;
pub mod keys;
pub mod permissions;
pub mod receive;
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
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            Self::Refused => "Neighbor request refused",
        })
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
