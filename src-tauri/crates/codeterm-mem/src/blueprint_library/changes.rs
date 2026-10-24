use crate::MemManager;
use std::collections::BTreeSet;
use std::sync::{atomic::Ordering, Arc};

pub type Listener = dyn Fn(&str, u64) + Send + Sync;

impl MemManager {
    pub fn set_blueprint_library_listener(&self, listener: Arc<Listener>) {
        *self.blueprint.changes.write() = Some(listener);
    }

    pub(crate) fn notify_blueprint_library_changed(&self, areas: impl IntoIterator<Item = String>) {
        let areas: BTreeSet<_> = areas.into_iter().collect();
        if areas.is_empty() {
            return;
        }
        let revision = self.blueprint.revision.fetch_add(1, Ordering::Relaxed) + 1;
        let listener = self.blueprint.changes.read().clone();
        if let Some(listener) = listener {
            for area in areas {
                listener(&area, revision);
            }
        }
    }
}
