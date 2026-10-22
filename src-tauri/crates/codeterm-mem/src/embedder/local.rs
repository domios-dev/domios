use super::provider::{self, Content, EmbedError, EmbeddingProvider, Modality};

pub const ID: provider::Id = "local";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pooling {
    Mean,
    Cls,
}

/// One row per local candle model; the embedder eval adds candidates as rows.
#[derive(Debug, PartialEq, Eq)]
pub struct LocalModel {
    pub model: &'static str,
    pub revision: &'static str,
    pub pooling: Pooling,
    pub query_prefix: &'static str,
    pub passage_prefix: &'static str,
    pub max_tokens: usize,
    pub dimension: usize,
}

pub static MODELS: &[LocalModel] = &[LocalModel {
    model: "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2",
    revision: "main",
    pooling: Pooling::Mean,
    query_prefix: "",
    passage_prefix: "",
    max_tokens: 512,
    dimension: 384,
}];

const MODALITIES: &[Modality] = &[Modality::Text];

pub fn default_model() -> &'static LocalModel {
    &MODELS[0]
}

pub fn find(model: &str) -> Option<&'static LocalModel> {
    MODELS.iter().find(|row| row.model == model)
}

pub struct LocalProvider {
    model: &'static LocalModel,
}

impl LocalProvider {
    pub fn new(model: &'static LocalModel) -> Self {
        Self { model }
    }

    fn embed(&self, content: &Content, prefix: &str, query: bool) -> Result<Vec<f32>, EmbedError> {
        let text = provider::supported_text(MODALITIES, content)?;
        let input = format!("{prefix}{text}");
        let vector = run(self.model, &input, query)?;
        provider::validate_vector(&vector, self.model.dimension)?;
        Ok(vector)
    }
}

#[cfg(feature = "embeddings")]
fn run(model: &'static LocalModel, text: &str, query: bool) -> Result<Vec<f32>, EmbedError> {
    crate::embedding_worker::embed(model, text, query).map_err(EmbedError::Unavailable)
}

#[cfg(not(feature = "embeddings"))]
fn run(_model: &'static LocalModel, _text: &str, _query: bool) -> Result<Vec<f32>, EmbedError> {
    Err(EmbedError::NotBuilt(
        "embeddings feature not enabled — rebuild with --features embeddings".into(),
    ))
}

impl EmbeddingProvider for LocalProvider {
    fn id(&self) -> &str {
        ID
    }

    fn model(&self) -> &str {
        self.model.model
    }

    fn dimension(&self) -> usize {
        self.model.dimension
    }

    fn max_tokens(&self) -> usize {
        self.model.max_tokens
    }

    fn modalities(&self) -> &[Modality] {
        MODALITIES
    }

    fn remote(&self) -> bool {
        false
    }

    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed(content, self.model.passage_prefix, false)
    }

    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed(content, self.model.query_prefix, true)
    }
}
