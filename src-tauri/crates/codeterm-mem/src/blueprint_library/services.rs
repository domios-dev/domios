use super::Pending;
use parking_lot::{Mutex, RwLock};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrichmentJob {
    pub item_id: String,
    pub document_hash: String,
    pub task: String,
}

pub trait BlueprintEnricher: Send + Sync {
    fn request(&self, job: EnrichmentJob) -> Result<(), String>;
}

#[derive(Default)]
pub(crate) struct BlueprintServices {
    pub(super) catalog: Mutex<super::authoring::CatalogCache>,
    pub(crate) queue: Mutex<Option<SyncSender<Pending>>>,
    pub(super) enricher: RwLock<Option<Arc<dyn BlueprintEnricher>>>,
}

impl BlueprintServices {
    pub(super) fn enricher(&self) -> Option<Arc<dyn BlueprintEnricher>> {
        self.enricher.read().clone()
    }
}
