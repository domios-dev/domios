use std::sync::Arc;

use codeterm_mesh::{LastActivityProvider, MeshEventSink};

pub struct AppMeshEventSink<'a> {
    pub app: &'a crate::app_runtime::AppHandle,
}

impl MeshEventSink for AppMeshEventSink<'_> {
    fn emit_mesh_state_changed(&self) {
        codeterm_events::emit_everywhere(self.app, "mesh-state-changed", serde_json::Value::Null);
    }

    fn emit_config_changed(&self, snapshot: serde_json::Value) {
        codeterm_events::emit_everywhere(self.app, "config-changed", snapshot);
    }
}

pub struct UserLastActivityProvider(pub Arc<crate::user_last_activity::UserLastActivity>);

impl LastActivityProvider for UserLastActivityProvider {
    fn read_for_gossip(&self) -> Option<u64> {
        self.0.read_for_gossip()
    }

    fn mark(&self) {
        self.0.mark();
    }
}
