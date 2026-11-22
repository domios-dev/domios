mod binding;
mod close_retention;
mod concierge;
mod epoch_index;
mod maintenance;
mod projection;
mod retention;
mod store;

pub use super::spawn_chatbackend::live_transcript::{
    AttachedTranscriptTurn, DEFAULT_TRANSCRIPT_TURN_LIMIT,
};
pub use close_retention::TranscriptReader;
pub use codeterm_types::live_transcript::*;
pub(in crate::service) use concierge::{external_handoff, is_transcript_delivery_token};
pub use concierge::{ConciergeStatus, LiveTranscriptStatus, ScoutStatus, TranscriptTrigger};
pub use projection::{MAX_AGENT_PAGE_CHARS, MIN_AGENT_PAGE_CHARS};
pub use retention::DEFAULT_RAW_BUDGET_BYTES;
pub use store::{TranscriptStore, MAX_ROW_BYTES, RAW_SEGMENT_BYTES, RAW_SEGMENT_ROWS};

use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, OnceLock},
};

#[derive(Default)]
pub(super) struct TranscriptOwners {
    loaded: OnceLock<Result<(), String>>,
    stores: Mutex<HashMap<String, TranscriptStore>>,
    gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    routers: Mutex<HashMap<String, String>>,
}

impl TranscriptOwners {
    pub(super) fn gate(&self, owner: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.gates.lock().entry(owner.into()).or_default().clone()
    }

    pub(super) fn lookup_live(&self, live_tab_id: &str) -> Option<TranscriptStore> {
        Self::lookup(
            &self.stores.lock(),
            |state| matches!(state, TranscriptOwnerState::Open { live_tab_id: id } if id == live_tab_id),
        )
    }

    pub(super) fn lookup_closed(
        &self,
        identity: &ClosedTranscriptOwner,
    ) -> Option<TranscriptStore> {
        Self::lookup(
            &self.stores.lock(),
            |state| matches!(state, TranscriptOwnerState::Closed { identity: original } if &original == identity),
        )
    }

    pub(super) fn lookup_closed_tab(&self, tab_id: &str) -> Option<TranscriptStore> {
        Self::lookup(
            &self.stores.lock(),
            |state| matches!(state, TranscriptOwnerState::Closed { identity } if identity.tab_id == tab_id),
        )
    }

    fn lookup(
        stores: &HashMap<String, TranscriptStore>,
        matches: impl Fn(TranscriptOwnerState) -> bool,
    ) -> Option<TranscriptStore> {
        stores
            .values()
            .find(|store| matches(store.owner_state()))
            .cloned()
    }

    pub(super) fn open_live(
        &self,
        root: &Path,
        live_tab_id: &str,
    ) -> Result<TranscriptStore, String> {
        let mut stores = self.stores.lock();
        if let Some(store) = Self::lookup(
            &stores,
            |state| matches!(state, TranscriptOwnerState::Open { live_tab_id: id } if id == live_tab_id),
        ) {
            return Ok(store);
        }
        if stores.contains_key(live_tab_id) {
            return Err("Transcript owner has not been reopened".into());
        }
        let store = TranscriptStore::open(root, live_tab_id, DEFAULT_RAW_BUDGET_BYTES)
            .map_err(|error| error.to_string())?;
        if !matches!(store.owner_state(), TranscriptOwnerState::Open { live_tab_id: id } if id == live_tab_id)
        {
            return Err("Transcript store has a different live owner".into());
        }
        stores.insert(live_tab_id.into(), store.clone());
        Ok(store)
    }

    pub(super) fn load_existing(&self, root: &Path) -> Result<Vec<TranscriptStore>, String> {
        self.ensure_loaded(root)?;
        Ok(self.stores.lock().values().cloned().collect())
    }

    pub(super) fn ensure_loaded(&self, root: &Path) -> Result<(), String> {
        self.loaded.get_or_init(|| self.read_existing(root)).clone()
    }

    pub(super) fn attached_store_for_router(
        &self,
        router: &str,
    ) -> Option<(TranscriptStore, ConciergeEpoch)> {
        let owner = self.routers.lock().get(router).cloned()?;
        let store = self.stores.lock().get(&owner).cloned()?;
        let BindingTarget::Attached { epoch_id } = store.binding().target else {
            return None;
        };
        let epoch = store.epoch(&epoch_id).ok().flatten()?.epoch;
        (epoch.runtime_tab_id == router).then_some((store, epoch))
    }

    pub(super) fn register_router(&self, router: &str, owner: &str) {
        self.routers.lock().insert(router.into(), owner.into());
    }

    pub(super) fn forget_router(&self, router: &str) {
        self.routers.lock().remove(router);
    }

    fn read_existing(&self, root: &Path) -> Result<(), String> {
        let entries = match std::fs::read_dir(root.join("transcripts")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        let mut stores = self.stores.lock();
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
            {
                continue;
            }
            let Some(id) = entry
                .file_name()
                .into_string()
                .ok()
                .filter(|id| store::validate_tab_id(id).is_ok())
            else {
                continue;
            };
            if stores.contains_key(&id) || !entry.path().join("manifest.json").is_file() {
                continue;
            }
            match TranscriptStore::open(root, &id, DEFAULT_RAW_BUDGET_BYTES) {
                Ok(store) => {
                    stores.insert(id, store);
                }
                Err(error) => {
                    log::warn!("[live-transcript] owner={id} durable state unavailable: {error}")
                }
            }
        }
        Ok(())
    }
}
