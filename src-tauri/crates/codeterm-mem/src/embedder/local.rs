use super::provider::{self, Content, EmbedError, EmbeddingProvider, Modality};

pub const ID: provider::Id = "local";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pooling {
    Mean,
    Cls,
}

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

pub static MODELS: &[LocalModel] = &[
    LocalModel {
        model: "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2",
        revision: "main",
        pooling: Pooling::Mean,
        query_prefix: "",
        passage_prefix: "",
        max_tokens: 512,
        dimension: 384,
    },
    LocalModel {
        model: "intfloat/multilingual-e5-small",
        revision: "614241f622f53c4eeff9890bdc4f31cfecc418b3",
        pooling: Pooling::Mean,
        query_prefix: "query: ",
        passage_prefix: "passage: ",
        max_tokens: 512,
        dimension: 384,
    },
    LocalModel {
        model: "BAAI/bge-small-en-v1.5",
        revision: "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a",
        pooling: Pooling::Cls,
        query_prefix: "Represent this sentence for searching relevant passages: ",
        passage_prefix: "",
        max_tokens: 512,
        dimension: 384,
    },
    LocalModel {
        model: "intfloat/e5-small-v2",
        revision: "ffb93f3bd4047442299a41ebb6fa998a38507c52",
        pooling: Pooling::Mean,
        query_prefix: "query: ",
        passage_prefix: "passage: ",
        max_tokens: 512,
        dimension: 384,
    },
];

impl Pooling {
    #[cfg(any(feature = "embeddings", test))]
    pub(crate) fn weights(self, mask: &[u32]) -> Result<Vec<f32>, String> {
        let total = mask.iter().map(|&value| value as f32).sum::<f32>();
        if mask.is_empty() || total == 0.0 {
            return Err("pooling needs at least one unmasked token".into());
        }
        Ok(match self {
            Self::Mean => mask.iter().map(|&value| value as f32 / total).collect(),
            Self::Cls => (0..mask.len()).map(|index| f32::from(index == 0)).collect(),
        })
    }
}

impl LocalModel {
    fn input(&self, content: &Content, query: bool) -> Result<String, EmbedError> {
        let text = provider::supported_text(MODALITIES, content)?;
        let prefix = if query {
            self.query_prefix
        } else {
            self.passage_prefix
        };
        Ok(format!("{prefix}{text}"))
    }
}

#[cfg(test)]
mod tests;

const MODALITIES: &[Modality] = &[Modality::Text];

pub fn default_model() -> &'static LocalModel {
    &MODELS[0]
}

pub fn find(model: &str) -> Option<&'static LocalModel> {
    MODELS.iter().find(|row| row.model == model)
}

pub(super) fn retain_models(
    published: &provider::EmbedderIdentity,
    target: Option<&provider::EmbedderIdentity>,
) {
    #[cfg(feature = "embeddings")]
    {
        let models = std::iter::once(published)
            .chain(target)
            .filter(|identity| identity.provider == ID)
            .map(|identity| identity.model.clone())
            .collect();
        crate::embedding_worker::retain_models(models);
    }
    #[cfg(not(feature = "embeddings"))]
    let _ = (published, target);
}

pub struct LocalProvider {
    model: &'static LocalModel,
}

impl LocalProvider {
    pub fn new(model: &'static LocalModel) -> Self {
        Self { model }
    }

    fn embed(&self, content: &Content, query: bool) -> Result<Vec<f32>, EmbedError> {
        let input = self.model.input(content, query)?;
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
        self.embed(content, false)
    }

    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed(content, true)
    }
}
