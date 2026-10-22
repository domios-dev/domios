use serde::{Deserialize, Serialize};

/// The one mem embedder. An empty model or a zero dimension means the
/// provider's default; a remote provider also needs `remote_opt_in`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbedderConfig {
    pub provider: String,
    pub model: String,
    pub dimension: usize,
    pub remote_opt_in: bool,
}

impl Default for EmbedderConfig {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            model: String::new(),
            dimension: 0,
            remote_opt_in: false,
        }
    }
}
