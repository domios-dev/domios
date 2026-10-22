use super::services::{BlueprintEnricher, EnrichmentJob};
use crate::embedder::{provider, Content, EmbedError, EmbeddingProvider, Modality};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub(crate) struct WordEmbedder {
    pub(crate) available: AtomicBool,
}

impl WordEmbedder {
    pub(crate) fn new(available: bool) -> Arc<Self> {
        Arc::new(Self {
            available: AtomicBool::new(available),
        })
    }

    fn vector(&self, text: &str) -> Result<Vec<f32>, String> {
        if !self.available.load(Ordering::SeqCst) {
            return Err("fake embedder offline".into());
        }
        let mut vector = vec![0.0f32; self.dimension()];
        for word in text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.len() > 2)
        {
            let slot = word.bytes().fold(7usize, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(byte as usize)
            });
            let dimension = vector.len();
            vector[slot % dimension] += 1.0;
        }
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
        Ok(vector.into_iter().map(|v| v / norm).collect())
    }
}

impl EmbeddingProvider for WordEmbedder {
    fn id(&self) -> &str {
        "fixture"
    }
    fn model(&self) -> &str {
        "word"
    }
    fn dimension(&self) -> usize {
        384
    }
    fn max_tokens(&self) -> usize {
        512
    }
    fn modalities(&self) -> &[Modality] {
        &[Modality::Text]
    }
    fn remote(&self) -> bool {
        false
    }
    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        let text = provider::supported_text(self.modalities(), content)?;
        self.vector(text).map_err(EmbedError::Unavailable)
    }

    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed_passage(content)
    }
}

#[derive(Default)]
pub(crate) struct RecordingEnricher {
    pub(crate) jobs: Mutex<Vec<EnrichmentJob>>,
    pub(crate) offline: AtomicBool,
}

impl BlueprintEnricher for RecordingEnricher {
    fn request(&self, job: EnrichmentJob) -> Result<(), String> {
        if self.offline.load(Ordering::SeqCst) {
            return Err("mem workers unavailable".into());
        }
        self.jobs.lock().push(job);
        Ok(())
    }
}
