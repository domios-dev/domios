//! Transcription as a plugin capability. `Transcriber` is a core-owned trait
//! resolved from [`TranscribeRegistry`] by id (never a hardcoded match); there's
//! no native engine — the bundled `transcriber` plugin owns the lifecycle.

use std::path::Path;

use async_trait::async_trait;

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

/// A transcription backend. Implemented by JS plugins (`JsTranscriber`).
#[async_trait]
pub trait Transcriber: Send + Sync {
    fn id(&self) -> &str;
    async fn status(&self) -> TranscriberStatus;
    /// Transcribe an audio file already on disk. `language` is a hint (may be `""`).
    async fn transcribe(
        &self,
        audio_path: &Path,
        language: &str,
    ) -> Result<Transcript, TranscribeError>;
}

/// Runtime registry of transcribers, keyed by id. Populated at startup with the
/// built-in engine and at runtime by the `PluginManager`.
pub type TranscribeRegistry = crate::plugin::capability::CapabilityRegistry<dyn Transcriber>;
