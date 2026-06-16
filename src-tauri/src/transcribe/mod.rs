//! Transcription as a plugin capability.
//!
//! `Transcriber` is the core-owned trait a plugin (or the built-in HTTP engine)
//! implements to turn a recorded audio file into text. The active transcriber is
//! resolved from a [`TranscribeRegistry`] by id — never by a hardcoded match — so
//! a channel plugin can supply a local engine, a mesh-delegating engine, or
//! anything else satisfying the contract.
//!
//! The native [`http_engine::HttpEngine`] is registered at startup as
//! `builtin-http` from `VoiceTranscribeConfig`, so voice notes keep working with
//! no plugin installed (zero-regression migration — gap G5).

use std::path::Path;

use async_trait::async_trait;

pub mod http_engine;

/// The text produced for an audio file.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
}

/// Whether a transcriber is ready to serve.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TranscriberStatus {
    Ready,
    Loading,
    Unloaded,
    Unavailable { reason: String },
}

/// Failure modes shared by every transcriber implementation.
#[derive(Debug)]
pub enum TranscribeError {
    Disabled,
    EmptyPath,
    InvalidUrl(String),
    HealthFailed(String),
    RequestFailed(String),
    HttpStatus(u16),
    ParseError(String),
    Empty,
    /// No transcriber is installed/active.
    NoBackend,
}

impl std::fmt::Display for TranscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "voice transcription disabled"),
            Self::EmptyPath => write!(f, "empty attachment path"),
            Self::InvalidUrl(u) => write!(f, "invalid daemon URL: {}", u),
            Self::HealthFailed(e) => write!(f, "daemon health check failed: {}", e),
            Self::RequestFailed(e) => write!(f, "transcribe request failed: {}", e),
            Self::HttpStatus(c) => write!(f, "transcribe returned HTTP {}", c),
            Self::ParseError(e) => write!(f, "failed to parse response: {}", e),
            Self::Empty => write!(f, "empty transcript"),
            Self::NoBackend => write!(f, "no transcriber installed"),
        }
    }
}

impl std::error::Error for TranscribeError {}

/// A transcription backend. Implemented natively (`HttpEngine`) and by JS plugins
/// (`JsTranscriber`).
#[async_trait]
pub trait Transcriber: Send + Sync {
    fn id(&self) -> &str;
    async fn status(&self) -> TranscriberStatus;
    /// Transcribe an audio file already on disk. `language` is a hint (may be `""`).
    async fn transcribe(&self, audio_path: &Path, language: &str)
        -> Result<Transcript, TranscribeError>;
}

/// Runtime registry of transcribers, keyed by id. Populated at startup with the
/// built-in engine and at runtime by the `PluginManager`.
pub type TranscribeRegistry = crate::plugin::capability::CapabilityRegistry<dyn Transcriber>;
