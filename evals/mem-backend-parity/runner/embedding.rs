use codeterm_mem::embedder::{
    local::{self, LocalProvider},
    provider::SharedProvider,
    ActiveEmbedder, Content, EmbedError, EmbedderIdentity, EmbeddingProvider, Modality, StampedVector,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

use super::fixture::{CorpusItem, Query};

pub struct Prepared {
    pub passages: Vec<StampedVector>,
    pub queries: Vec<Vec<f32>>,
    pub embedder: Arc<ActiveEmbedder>,
}

fn normalize(text: &str) -> String {
    codeterm_mem::embeddings::chunk_text(text).join(" ")
}

struct Synthetic;

impl EmbeddingProvider for Synthetic {
    fn id(&self) -> &str { "parity-synthetic" }
    fn model(&self) -> &str { "sha256-token-count-64-v1" }
    fn dimension(&self) -> usize { 64 }
    fn max_tokens(&self) -> usize { 512 }
    fn modalities(&self) -> &[Modality] { &[Modality::Text] }
    fn remote(&self) -> bool { false }
    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        let text = codeterm_mem::embedder::provider::supported_text(self.modalities(), content)?;
        let mut vector = vec![0.0_f32; self.dimension()];
        for token in text.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()) {
            let hash = Sha256::digest(token.to_lowercase().as_bytes());
            let bucket = u16::from_le_bytes([hash[0], hash[1]]) as usize % self.dimension();
            vector[bucket] += 1.0;
        }
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm == 0.0 {
            return Err(EmbedError::Refused("empty synthetic embedding".into()));
        }
        Ok(vector.into_iter().map(|v| v / norm).collect())
    }
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed_passage(content)
    }
}

struct FrozenQueries {
    identity: EmbedderIdentity,
    vectors: BTreeMap<String, Vec<f32>>,
}

impl EmbeddingProvider for FrozenQueries {
    fn id(&self) -> &str { &self.identity.provider }
    fn model(&self) -> &str { &self.identity.model }
    fn dimension(&self) -> usize { self.identity.dimension }
    fn max_tokens(&self) -> usize { 512 }
    fn modalities(&self) -> &[Modality] { &[Modality::Text] }
    fn remote(&self) -> bool { false }
    fn embed_passage(&self, _: &Content) -> Result<Vec<f32>, EmbedError> {
        Err(EmbedError::Refused("passages are frozen before store loading".into()))
    }
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        let text = codeterm_mem::embedder::provider::supported_text(self.modalities(), content)?;
        self.vectors.get(&normalize(text)).cloned()
            .ok_or_else(|| EmbedError::Refused("query is outside the fixed set".into()))
    }
}

pub fn prepare(
    mode: &str,
    model: Option<&str>,
    corpus: &[CorpusItem],
    queries: &[Query],
) -> Result<Option<Prepared>, String> {
    let provider: SharedProvider = if mode == "local" {
        let descriptor = match model {
            Some(name) => local::find(name).ok_or("unknown local model")?,
            None => local::default_model(),
        };
        println!("Local Candle model: {} revision {}; cache-only", descriptor.model, descriptor.revision);
        Arc::new(LocalProvider::new(descriptor))
    } else {
        Arc::new(Synthetic)
    };
    let identity = provider.identity();
    let mut passages = Vec::new();
    for item in corpus {
        let text = codeterm_mem::compose_embed_text(
            item.kind.as_str(), &item.title, Some(&item.area), &item.tags, &item.body,
        );
        let vector = match provider.embed_passage(&Content::text(normalize(&text))) {
            Ok(vector) => vector,
            Err(EmbedError::Unavailable(reason))
                if mode == "local" && reason.contains("offline:") && reason.contains("is not cached") =>
            {
                println!("SKIP: local Candle model cache is incomplete: {reason}");
                return Ok(None);
            }
            Err(EmbedError::NotBuilt(reason)) if mode == "local" => {
                println!("SKIP: local Candle runtime is not built: {reason}");
                return Ok(None);
            }
            Err(error) => return Err(error.to_string()),
        };
        codeterm_mem::embedder::provider::validate_vector(&vector, identity.dimension)
            .map_err(|e| e.to_string())?;
        passages.push(StampedVector { identity: identity.clone(), vector });
    }
    let mut frozen = BTreeMap::new();
    let mut vectors = Vec::new();
    for query in queries {
        let text = normalize(&query.text);
        let vector = provider.embed_query(&Content::text(&text)).map_err(|e| e.to_string())?;
        codeterm_mem::embedder::provider::validate_vector(&vector, identity.dimension)
            .map_err(|e| e.to_string())?;
        frozen.insert(text, vector.clone());
        vectors.push(vector);
    }
    Ok(Some(Prepared {
        passages,
        queries: vectors,
        embedder: ActiveEmbedder::new(Arc::new(FrozenQueries { identity, vectors: frozen })),
    }))
}
