use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::multiplexer::types::{Tab, TabBacking};
use crate::service::delivery_error::DeliveryError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChatIdentity {
    pub(crate) tab_id: String,
    pub(crate) plugin_id: String,
    pub(crate) session_id: String,
    pub(crate) binding_nonce: String,
}

impl ChatIdentity {
    pub fn from_tab(tab: &Tab) -> Option<Self> {
        let TabBacking::ChatBackend {
            plugin_id,
            session_id,
        } = &tab.backing
        else {
            return None;
        };
        if !tab.launch_session_authenticated()
            || tab.agent_session_id.as_deref() != Some(session_id)
            || tab.boot_stage != Some(codeterm_types::pty::BootStage::AgentReady)
            || tab.agent_booting
            || tab.agent_type.as_deref() != Some(plugin_id.as_str())
            || tab.watcher.is_some()
        {
            return None;
        }
        Some(Self {
            tab_id: tab.id.clone(),
            plugin_id: plugin_id.clone(),
            session_id: session_id.clone(),
            binding_nonce: tab.session_binding_nonce.clone()?,
        })
    }
}

#[derive(Clone)]
pub(crate) struct CapturedChat {
    pub(crate) identity: ChatIdentity,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) expires_at: std::time::Instant,
}

impl CapturedChat {
    pub(crate) fn validate(&self, current: Option<ChatIdentity>) -> Result<(), DeliveryError> {
        if self.active.load(Ordering::Acquire)
            && std::time::Instant::now() < self.expires_at
            && current.as_ref() == Some(&self.identity)
        {
            Ok(())
        } else {
            Err(DeliveryError::TabBusyRefused {
                tab: self.identity.tab_id.clone(),
                detail: "local recording recipient or capture generation changed".into(),
            })
        }
    }

    pub(crate) fn handoff(
        &self,
        current: Option<ChatIdentity>,
        send: impl FnOnce(&str, &str) -> Result<(), String>,
    ) -> Result<(), DeliveryError> {
        self.validate(current)?;
        send(&self.identity.plugin_id, &self.identity.session_id).map_err(|detail| {
            // A backend call can fail after accepting input; it cannot prove zero writes.
            DeliveryError::SubmitUnconfirmed {
                tab: self.identity.tab_id.clone(),
                detail,
            }
        })
    }
}

#[cfg(test)]
mod tests;
