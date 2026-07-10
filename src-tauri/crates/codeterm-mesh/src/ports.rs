use std::sync::Arc;

/// App-owned UI broadcast port — replaces direct `api::events` calls from mesh.
pub trait MeshEventSink: Send + Sync {
    fn emit_mesh_state_changed(&self);
    fn emit_config_changed(&self, snapshot: serde_json::Value);
}

/// App-owned last-activity source for gossip payloads.
pub trait LastActivityProvider: Send + Sync {
    fn read_for_gossip(&self) -> Option<u64>;
    fn mark(&self) {}
}

struct DefaultLastActivity;

impl LastActivityProvider for DefaultLastActivity {
    fn read_for_gossip(&self) -> Option<u64> {
        None
    }
}

pub(crate) fn default_last_activity() -> Arc<dyn LastActivityProvider> {
    Arc::new(DefaultLastActivity)
}

#[cfg(test)]
pub(crate) struct RecordingLastActivity {
    ms: std::sync::atomic::AtomicU64,
    dev: bool,
}

#[cfg(test)]
impl RecordingLastActivity {
    pub fn new(dev: bool) -> Arc<Self> {
        Arc::new(Self {
            ms: std::sync::atomic::AtomicU64::new(0),
            dev,
        })
    }

    pub fn mark(&self) {
        self.ms.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

#[cfg(test)]
impl LastActivityProvider for RecordingLastActivity {
    fn read_for_gossip(&self) -> Option<u64> {
        if self.dev {
            None
        } else {
            Some(self.ms.load(std::sync::atomic::Ordering::Relaxed))
        }
    }

    fn mark(&self) {
        RecordingLastActivity::mark(self);
    }
}
