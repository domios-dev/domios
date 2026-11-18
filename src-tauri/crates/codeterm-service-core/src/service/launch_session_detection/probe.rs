use super::*;
use crate::agent::provider::{LaunchProcess, LaunchSessionEvidence};

#[cfg(test)]
pub(super) struct FixtureProcessTree {
    pub(super) pty_id: String,
    pub(super) processes: Vec<LaunchProcess>,
}

impl ServiceContext {
    pub(super) fn probe_process_launch_session(
        &self,
        tab_id: &str,
    ) -> Option<DetectedLaunchSession> {
        let (provider_id, pty_id, binding_nonce, cwd, launched_at_ms) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.launch_session_authenticated() {
                return None;
            }
            let launched_at_ms = tab
                .agent_set_at
                .map(|set_at| epoch_ms().saturating_sub(set_at.elapsed().as_millis() as u64))
                .unwrap_or(0);
            (
                tab.agent_type.clone()?,
                tab.pty_id.clone()?,
                tab.session_binding_nonce.clone()?,
                tab.cwd.clone().unwrap_or_default(),
                launched_at_ms,
            )
        };
        let Some(processes) = self.pty_launch_processes(&pty_id) else {
            log::debug!("[launch-session-detection] tab={tab_id} pty={pty_id} refused: missing_pty_root_pid");
            return None;
        };
        let evidence = LaunchSessionEvidence {
            cwd,
            launch_marker: LAUNCH_MARKERS.lock().get(tab_id).cloned(),
            launched_at_ms,
            processes,
        };
        self.detect_launch_from_evidence(tab_id, provider_id, pty_id, binding_nonce, evidence)
    }

    fn pty_launch_processes(&self, pty_id: &str) -> Option<Vec<LaunchProcess>> {
        #[cfg(test)]
        if let Some(tree) = self
            .try_ext::<FixtureProcessTree>()
            .filter(|tree| tree.pty_id == pty_id)
        {
            return Some(tree.processes.clone());
        }
        let root_pid = {
            let pty = self.pty.lock();
            pty.get_session(pty_id)
                .and_then(|session| session.child.lock().ok()?.process_id())?
        };
        Some(super::super::process_tree::launch_processes(root_pid))
    }

    pub(super) fn detect_launch_from_evidence(
        &self,
        tab_id: &str,
        provider_id: String,
        pty_id: String,
        binding_nonce: String,
        evidence: LaunchSessionEvidence,
    ) -> Option<DetectedLaunchSession> {
        let invocation = match super::super::provider_accounts::for_tab(self, tab_id) {
            Ok(invocation) => invocation,
            Err(error) => {
                log::debug!("[launch-session-detection] tab={tab_id} refused: account_invocation_unavailable: {error}");
                return None;
            }
        };
        let current = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_type.as_deref() == Some(provider_id.as_str())
                && tab.pty_id.as_deref() == Some(pty_id.as_str())
                && tab.session_binding_nonce.as_deref() == Some(binding_nonce.as_str())
                && !tab.launch_session_authenticated()
        });
        if !current {
            log::debug!("[launch-session-detection] tab={tab_id} pty={pty_id} refused: launch_changed_or_bound");
            return None;
        }
        let process_count = evidence.processes.len();
        let named = invocation.is_some();
        log::debug!("[launch-session-detection] tab={tab_id} pty={pty_id} provider={provider_id} named={named} process_count={process_count} probing");
        let detection = with_context(invocation.clone(), || {
            self.agent_registry
                .with_provider(&provider_id, |provider| {
                    provider.detect_launch_session(&evidence)
                })
                .flatten()
        });
        let Some(detection) = detection else {
            log::debug!("[launch-session-detection] tab={tab_id} pty={pty_id} refused: no_matching_provider_record");
            return None;
        };
        Some(DetectedLaunchSession {
            provider_id,
            pty_id,
            binding_nonce,
            detection,
            invocation,
        })
    }
}
