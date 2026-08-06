//! Session identity on a tab: the provider session id and the per-launch
//! binding nonce that proves a session-start report belongs to this tab.

use std::time::{SystemTime, UNIX_EPOCH};

use super::tab::clear_runtime_model_evidence;
use super::ServiceContext;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

impl ServiceContext {
    /// Reject a session_id already claimed by another tab: parallel detection
    /// in one cwd otherwise locks two tabs onto the same fresh session.
    pub fn set_agent_session_id(&self, tab_id: &str, session_id: Option<String>) {
        let session_bound = session_id.is_some();
        let bound_session_id = session_id.clone();
        let mut mux = self.mux.write();
        if let Some(ref new_sid) = session_id {
            let conflict = mux
                .tab_ids_with_session(new_sid)
                .into_iter()
                .any(|other| other != tab_id);
            if conflict {
                log::info!(
                    "[set_agent_session_id] rejecting duplicate sid {} for Tab {} (claimed elsewhere)",
                    new_sid, tab_id,
                );
                return;
            }
        }
        let mut changed = false;
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            changed = tab.agent_session_id != session_id;
            tab.agent_session_id = session_id;
            tab.reported_session_binding_nonce = None;
            if changed {
                clear_runtime_model_evidence(tab);
            }
        }
        drop(mux);
        if changed {
            self.invalidate_guarded_agent_input(tab_id);
        }
        if session_bound {
            crate::service::general_agent::binding_wait::notify_session_bound();
        }
        if let Some(session_id) = bound_session_id.as_deref() {
            match self
                .plan_store
                .mark_live_task_attempt_runtime_ready_for_tab(tab_id, session_id, epoch_ms())
            {
                Ok(Some(ready)) => crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "plan-changed",
                    serde_json::json!({ "plan_id": ready.plan_id }),
                ),
                Ok(None) => {}
                Err(error) => log::warn!(
                    "[set_agent_session_id] task attempt readiness failed tab={tab_id}: {error}"
                ),
            }
        }
        self.bind_planner_session(tab_id);
        self.heal_recovery_state_if_ready();
    }

    /// Rotate the per-launch binding nonce before a provider command is
    /// written. This is the launch boundary, so it opens the operation.
    pub fn rotate_session_binding_nonce(&self, tab_id: &str) -> String {
        self.invalidate_guarded_agent_input(tab_id);
        self.open_lifecycle_epoch(tab_id);
        let nonce = uuid::Uuid::new_v4().to_string();
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.remember_superseded_binding_nonce();
            tab.session_binding_nonce = Some(nonce.clone());
            tab.reported_session_binding_nonce = None;
        }
        nonce
    }

    /// Per-launch binding nonce for a tab. Injected as CODETERM_SESSION_BINDING_NONCE so a provider's session-start report can prove it owns this tab; a report carrying a different nonce is a stale/foreign claim and gets rejected.
    pub fn ensure_session_binding_nonce(&self, tab_id: &str) -> String {
        let (nonce, minted) = {
            let mut mux = self.mux.write();
            match mux.get_tab_mut(tab_id) {
                Some(tab) => match tab.session_binding_nonce.clone() {
                    Some(nonce) => (nonce, false),
                    None => {
                        let nonce = uuid::Uuid::new_v4().to_string();
                        tab.session_binding_nonce = Some(nonce.clone());
                        tab.reported_session_binding_nonce = None;
                        (nonce, true)
                    }
                },
                None => (uuid::Uuid::new_v4().to_string(), false),
            }
        };
        // A freshly minted nonce is a new launch, so it starts a new operation.
        if minted {
            self.open_lifecycle_epoch(tab_id);
        }
        nonce
    }

    /// Per-session nonce exported as CODETERM_SHELL_NONCE. The OSC 633;E
    /// consumer rejects command text whose nonce mismatches.
    pub fn ensure_shell_integration_nonce(&self, tab_id: &str) -> String {
        let mut mux = self.mux.write();
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            if let Some(ref n) = tab.shell_integration_nonce {
                return n.clone();
            }
            let nonce = uuid::Uuid::new_v4().to_string();
            tab.shell_integration_nonce = Some(nonce.clone());
            nonce
        } else {
            uuid::Uuid::new_v4().to_string()
        }
    }
}
