use serde_json::Value;

use super::PluginVm;
use crate::agent_verbs::{AgentVerbCatalog, PLUGIN_INDEX_CONTRIBUTION_KIND};
use crate::plugin_connection::{login_command, PluginConnectionState};

#[derive(Default)]
pub(super) struct ConnectionSnapshot {
    enabled: bool,
    state: Option<PluginConnectionState>,
}

impl PluginVm {
    pub(crate) fn configure_connection(&self, catalog: &AgentVerbCatalog) {
        self.connection.lock().enabled = login_command(catalog).is_some();
    }

    pub(crate) fn connection_state(&self) -> Option<PluginConnectionState> {
        self.connection.lock().state
    }

    pub(crate) fn observe_connection(&self, method: &str, response: &Value) {
        let changed = {
            let mut snapshot = self.connection.lock();
            if !snapshot.enabled {
                return;
            }
            let state = PluginConnectionState::from_response(response).or_else(|| {
                if response.get("error").and_then(Value::as_str).is_some() {
                    return (method == "health").then_some(PluginConnectionState::Unknown);
                }
                match method {
                    "logout" => Some(PluginConnectionState::SignedOut),
                    "login" | "loginStart" => Some(PluginConnectionState::Pending),
                    "health" => Some(PluginConnectionState::Unknown),
                    _ => None,
                }
            });
            let Some(state) = state else {
                return;
            };
            let changed = snapshot.state != Some(state);
            snapshot.state = Some(state);
            changed
        };
        if changed {
            if let Some(service) = self.service() {
                service.contributions_changed(PLUGIN_INDEX_CONTRIBUTION_KIND);
            }
        }
    }
}
