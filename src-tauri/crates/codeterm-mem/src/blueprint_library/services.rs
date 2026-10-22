use super::Pending;
use parking_lot::{Mutex, RwLock};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

pub trait BlueprintEmbedder: Send + Sync {
    fn passage(&self, text: &str) -> Result<Vec<f32>, String>;
    fn query(&self, text: &str) -> Result<Vec<f32>, String>;
}

pub struct GlobalEmbedder;

impl BlueprintEmbedder for GlobalEmbedder {
    fn passage(&self, text: &str) -> Result<Vec<f32>, String> {
        #[cfg(all(feature = "embeddings", not(test)))]
        {
            crate::embeddings::embed(text)
        }
        #[cfg(not(all(feature = "embeddings", not(test))))]
        {
            let _ = text;
            Err("embeddings unavailable".into())
        }
    }

    fn query(&self, text: &str) -> Result<Vec<f32>, String> {
        #[cfg(all(feature = "embeddings", not(test)))]
        {
            crate::embeddings::query_embed(text)
        }
        #[cfg(not(all(feature = "embeddings", not(test))))]
        {
            let _ = text;
            Err("embeddings unavailable".into())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrichmentJob {
    pub item_id: String,
    pub document_hash: String,
    pub task: String,
}

pub trait BlueprintEnricher: Send + Sync {
    fn request(&self, job: EnrichmentJob) -> Result<(), String>;
}

pub(crate) struct BlueprintServices {
    pub(crate) queue: Mutex<Option<SyncSender<Pending>>>,
    pub(super) embedder: RwLock<Arc<dyn BlueprintEmbedder>>,
    pub(super) enricher: RwLock<Option<Arc<dyn BlueprintEnricher>>>,
}

impl Default for BlueprintServices {
    fn default() -> Self {
        Self {
            queue: Mutex::new(None),
            embedder: RwLock::new(Arc::new(GlobalEmbedder)),
            enricher: RwLock::new(None),
        }
    }
}

impl BlueprintServices {
    pub(super) fn embedder(&self) -> Arc<dyn BlueprintEmbedder> {
        self.embedder.read().clone()
    }

    pub(super) fn enricher(&self) -> Option<Arc<dyn BlueprintEnricher>> {
        self.enricher.read().clone()
    }
}
