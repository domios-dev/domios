//! Hook-independent session binding. Once a hooked provider's launch has missed
//! its binding deadline, the provider reads its own record of this tab's launch.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use super::ServiceContext;
use crate::agent::provider::{LaunchSessionDetection, LaunchSessionEvidence, LaunchSessionSource};
use crate::session::lifecycle::{AgentTypeWriteOrigin, BindingSource};

static LAUNCH_MARKERS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

struct DetectedLaunchSession {
    provider_id: String,
    binding_nonce: String,
    detection: LaunchSessionDetection,
}

pub(crate) fn binding_source(source: LaunchSessionSource) -> BindingSource {
    match source {
        LaunchSessionSource::PidRegistry => BindingSource::PidRegistry,
        LaunchSessionSource::LaunchMarker => BindingSource::LaunchMarker,
    }
}

fn source_label(source: BindingSource) -> &'static str {
    match source {
        BindingSource::Hook => "hook",
        BindingSource::PidRegistry => "pid_registry",
        BindingSource::LaunchMarker => "launch_marker",
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

impl ServiceContext {
    /// The inert marker one launch command carries into provider-visible text.
    pub(crate) fn mint_launch_marker(&self, tab_id: &str) -> String {
        let marker = format!("ct-launch-{}", uuid::Uuid::new_v4().simple());
        LAUNCH_MARKERS
            .lock()
            .insert(tab_id.to_string(), marker.clone());
        marker
    }

    /// One detection per tab at a time. Detection itself refuses a bound tab,
    /// and the commit refuses a launch the tab has since rotated.
    pub(crate) fn detect_unbound_launch_session(&self, tab_id: &str) {
        if !IN_FLIGHT.lock().insert(tab_id.to_string()) {
            return;
        }
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        tauri::async_runtime::spawn(async move {
            let probe_svc = svc.clone();
            let probe_tab = tab_id.clone();
            let detected =
                tokio::task::spawn_blocking(move || probe_svc.probe_launch_session(&probe_tab))
                    .await
                    .ok()
                    .flatten();
            if let Some(detected) = detected {
                svc.commit_detected_launch_session(&tab_id, detected).await;
            }
            IN_FLIGHT.lock().remove(&tab_id);
        });
    }

    fn probe_launch_session(&self, tab_id: &str) -> Option<DetectedLaunchSession> {
        let (provider_id, pty_id, binding_nonce, cwd, launched_at_ms) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.agent_session_id.is_some() {
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
        let root_pid = {
            let pty = self.pty.lock();
            let session = pty.get_session(&pty_id)?;
            let pid = session.child.lock().ok()?.process_id();
            pid?
        };
        let evidence = LaunchSessionEvidence {
            cwd,
            launch_marker: LAUNCH_MARKERS.lock().get(tab_id).cloned(),
            launched_at_ms,
            processes: super::process_tree::launch_processes(root_pid),
        };
        let detection = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider.detect_launch_session(&evidence)
            })
            .flatten()?;
        Some(DetectedLaunchSession {
            provider_id,
            binding_nonce,
            detection,
        })
    }

    async fn commit_detected_launch_session(&self, tab_id: &str, detected: DetectedLaunchSession) {
        let DetectedLaunchSession {
            provider_id,
            binding_nonce,
            detection,
        } = detected;
        let source = binding_source(detection.source);
        let session_id = detection.session_id;
        let claim = match crate::api::hook_routes::bind_detected_session(
            &self.mux,
            self.agent_registry.as_ref(),
            tab_id,
            &provider_id,
            &session_id,
            &binding_nonce,
        ) {
            Ok(claim) => claim,
            Err((status, message, _)) => {
                log::info!(
                    "[launch-session-detection] Tab {tab_id} refused {provider_id} session {session_id} from {} ({status}): {message}",
                    source_label(source),
                );
                return;
            }
        };
        log::info!(
            "[launch-session-detection] Tab {tab_id} bound {provider_id} session {session_id} from {}",
            source_label(source),
        );
        LAUNCH_MARKERS.lock().remove(tab_id);
        if let Err((status, message)) = crate::api::hook_routes::finish_session_binding(
            self,
            tab_id,
            &provider_id,
            &session_id,
            Some(&binding_nonce),
            Some(AgentTypeWriteOrigin::Hook),
            source_label(source),
            &serde_json::Value::Null,
            claim,
            source,
        )
        .await
        {
            log::warn!(
                "[launch-session-detection] Tab {tab_id} binding follow-up failed ({status}): {message}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_sources_journal_as_their_own_binding_source() {
        assert_eq!(
            binding_source(LaunchSessionSource::PidRegistry),
            BindingSource::PidRegistry
        );
        assert_eq!(
            binding_source(LaunchSessionSource::LaunchMarker),
            BindingSource::LaunchMarker
        );
        assert_ne!(
            binding_source(LaunchSessionSource::PidRegistry),
            BindingSource::Hook
        );
    }
}
