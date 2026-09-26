use super::clear_agent_identity_for_lifecycle_exit;
use crate::service::ServiceContext;

impl ServiceContext {
    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit(&self, tab_id: &str) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_with_cause(
            tab_id,
            "agent identity cleared on lifecycle exit",
        )
    }

    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit_with_cause(
        &self,
        tab_id: &str,
        cause: &str,
    ) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_during(
            tab_id,
            cause,
            crate::crash::shutdown::active_shutdown(),
        )
    }

    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit_during(
        &self,
        tab_id: &str,
        cause: &str,
        shutdown: Option<crate::crash::lifecycle::ShutdownReason>,
    ) -> bool {
        if !self.lifecycle_clear_allowed(tab_id, cause, shutdown.as_ref()) {
            return false;
        }
        let exited = self.claim_launch_exit(tab_id, None, None);
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .is_some_and(clear_agent_identity_for_lifecycle_exit)
        };
        if let Some(exited) = exited {
            self.publish_launch_exit(&exited);
        }
        self.finish_agent_identity_clear(tab_id, cause, cleared)
    }

    pub(crate) fn clear_tab_restored_identity_if_absent(&self, tab_id: &str, cause: &str) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_with_cause(tab_id, cause)
    }

    fn lifecycle_clear_allowed(
        &self,
        tab_id: &str,
        cause: &str,
        shutdown: Option<&crate::crash::lifecycle::ShutdownReason>,
    ) -> bool {
        if let Some(reason) = shutdown {
            log::info!(
                "[agent-lifecycle] preserving Tab identity during shutdown ({reason:?}): tab={tab_id} cause={cause}"
            );
            return false;
        }
        true
    }

    fn finish_agent_identity_clear(&self, tab_id: &str, cause: &str, cleared: bool) -> bool {
        if !cleared {
            return false;
        }
        self.record_lifecycle_end(
            tab_id,
            crate::session::lifecycle::LifecycleTransition::RolledBack {
                reason: cause.to_string(),
            },
        );
        self.invalidate_guarded_agent_input(tab_id);
        log::info!("[agent-lifecycle] cleared Tab identity: tab={tab_id} cause={cause}");
        self.signal_worker_interruption(
            tab_id,
            crate::service::worker_interruption::WorkerInterruptionEvent::ProcessExited,
        );
        self.clear_dead_planner_binding(tab_id);
        self.prompt_detector.lock().clear(tab_id);
        crate::service::structured_prompt::clear_tab(tab_id);
        self.emit_state_changed();
        true
    }
}
