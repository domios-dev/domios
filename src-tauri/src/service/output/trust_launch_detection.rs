use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use super::ServiceContext;

static SESSION_DETECT_COOLDOWN: LazyLock<StdMutex<HashMap<String, Instant>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

impl ServiceContext {
    pub(super) fn observe_trust_prompt_screen(
        &self,
        tab_id: Option<&str>,
        agent_type: Option<&str>,
        screen: Option<&str>,
    ) {
        if let (Some(tab_id), Some(agent_type), Some(screen)) = (tab_id, agent_type, screen) {
            super::super::directory_trust::observe_directory_trust_screen(
                self, tab_id, agent_type, screen,
            );
        }
    }

    /// Hookless providers use session heuristics; hooked providers inspect launch
    /// records after trust clears or the binding deadline expires.
    pub(super) fn maybe_detect_agent_session(&self, tab_id: &str, agent_type: &str) {
        let (cwd, set_at_elapsed, current_session, binding_nonce, deadline_elapsed) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            let cwd = tab.cwd.clone().unwrap_or_default();
            let elapsed = tab.agent_set_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            let sid = tab.agent_session_id.clone();
            let deadline_elapsed = tab.boot_stage
                == Some(codeterm_types::pty::BootStage::LaunchDeadlineElapsed)
                || tab.agent_set_at.is_some_and(|set_at| {
                    set_at.elapsed() >= crate::service::worker_ack::STARTUP_BINDING_DEADLINE
                });
            (
                cwd,
                elapsed,
                sid,
                tab.session_binding_nonce.clone(),
                deadline_elapsed,
            )
        };
        if !should_detect_unbound_session(current_session.as_deref()) {
            return;
        }
        let (heuristic, hooked) = self
            .agent_registry
            .with_provider(agent_type, |p| {
                let inj = matches!(
                    p.session_id_injection(),
                    crate::agent::provider::SessionIdInjection::None
                );
                (
                    should_heuristically_detect_session(
                        inj,
                        p.has_hooks(),
                        p.uses_structured_chat(),
                    ),
                    p.has_hooks(),
                )
            })
            .unwrap_or((false, false));
        let trust_prompt_resolved = binding_nonce.as_deref().is_some_and(|nonce| {
            super::super::directory_trust::trust_prompt_resolved_for_launch(tab_id, nonce)
        });
        let launch_detection = launch_session_detection_due(
            heuristic,
            hooked,
            deadline_elapsed,
            trust_prompt_resolved,
        );
        if !heuristic && !launch_detection {
            return;
        }
        if !launch_detection && cwd.is_empty() {
            return;
        }
        {
            let mut cd = SESSION_DETECT_COOLDOWN.lock().unwrap();
            let now = Instant::now();
            if let Some(last) = cd.get(tab_id) {
                if now.duration_since(*last) < std::time::Duration::from_secs(1) {
                    return;
                }
            }
            cd.insert(tab_id.to_string(), now);
        }
        if launch_detection {
            self.detect_unbound_launch_session(tab_id);
            return;
        }

        let exclude = self.mux.read().other_session_ids(tab_id);
        let max_age = std::time::Duration::from_secs(set_at_elapsed + 30);
        let found = self
            .agent_registry
            .with_provider(agent_type, |p| {
                p.detect_session_id(&cwd, &exclude, Some(max_age))
            })
            .flatten();
        if let Some(sid) = found {
            log::info!(
                "[output] post-launch detected {} session {} for Tab {}",
                agent_type,
                sid,
                tab_id
            );
            self.set_agent_session_id(tab_id, Some(sid));
            self.emit_state_changed();
        }
    }
}

pub(super) fn launch_session_detection_due(
    heuristic: bool,
    hooked: bool,
    deadline_elapsed: bool,
    trust_prompt_resolved: bool,
) -> bool {
    !heuristic && hooked && (deadline_elapsed || trust_prompt_resolved)
}

pub(super) fn should_detect_unbound_session(current_session: Option<&str>) -> bool {
    current_session.is_none()
}

pub(super) fn should_heuristically_detect_session(
    has_no_injection: bool,
    has_hooks: bool,
    uses_structured_chat: bool,
) -> bool {
    has_no_injection && !has_hooks && !uses_structured_chat
}
