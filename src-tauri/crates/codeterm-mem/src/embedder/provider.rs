use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub type Id = &'static str;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Image { mime: String, bytes: Vec<u8> },
}

impl Content {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn modality(&self) -> Modality {
        match self {
            Self::Text(_) => Modality::Text,
            Self::Image { .. } => Modality::Image,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
}

/// The stamp persisted beside the vectors: a vector is only comparable with
/// vectors carrying an equal identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmbedderIdentity {
    pub provider: String,
    pub model: String,
    pub dimension: usize,
}

impl fmt::Display for EmbedderIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{} ({} dims)",
            self.provider, self.model, self.dimension
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedError {
    /// Transient: the provider is down, rate limited or lacks its key. The
    /// caller queues the item as vector pending.
    Unavailable(String),
    /// Permanent for this content: unsupported modality, blocked by the
    /// credential filter, or rejected by the provider.
    Refused(String),
    /// The provider's runtime is not compiled into this build.
    NotBuilt(String),
}

impl fmt::Display for EmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "embedder unavailable: {reason}"),
            Self::Refused(reason) => write!(f, "embedding refused: {reason}"),
            Self::NotBuilt(reason) => write!(f, "embedder not built: {reason}"),
        }
    }
}

pub trait EmbeddingProvider: Send + Sync {
    fn id(&self) -> &str;
    fn model(&self) -> &str;
    fn dimension(&self) -> usize;
    fn max_tokens(&self) -> usize;
    fn modalities(&self) -> &[Modality];
    /// Whether embedded content leaves this machine.
    fn remote(&self) -> bool;
    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError>;
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError>;

    fn embed_passages(&self, contents: &[Content]) -> Vec<Result<Vec<f32>, EmbedError>> {
        contents
            .iter()
            .map(|content| self.embed_passage(content))
            .collect()
    }

    fn identity(&self) -> EmbedderIdentity {
        EmbedderIdentity {
            provider: self.id().to_string(),
            model: self.model().to_string(),
            dimension: self.dimension(),
        }
    }
}

pub type SharedProvider = Arc<dyn EmbeddingProvider>;

/// Refuses content outside `modalities`, returning the text of a text item.
pub fn supported_text<'c>(
    modalities: &[Modality],
    content: &'c Content,
) -> Result<&'c str, EmbedError> {
    let modality = content.modality();
    if !modalities.contains(&modality) {
        return Err(EmbedError::Refused(format!(
            "{modality:?} content is not supported by this embedder"
        )));
    }
    match content {
        Content::Text(text) => Ok(text),
        Content::Image { .. } => Err(EmbedError::Refused(
            "image embedding is declared but not implemented".into(),
        )),
    }
}

pub fn validate_vector(vector: &[f32], dimension: usize) -> Result<(), EmbedError> {
    if vector.len() != dimension {
        return Err(EmbedError::Refused(format!(
            "expected {dimension} dimensions, got {}",
            vector.len()
        )));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(EmbedError::Refused("vector has a non-finite value".into()));
    }
    Ok(())
}
