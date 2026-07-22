//! Truthful pane-boot progress. Stages are emitted at the REAL launch
//! transitions in `orchestrator::launch` — never timer-driven — so late UI
//! attachers and live subscribers see where a spawn actually is.

use super::ServiceContext;
use crate::multiplexer::types::Pane;

pub use codeterm_types::pty::{BootStage, BootStageSink};

pub(crate) fn allows_shell_prompt_exit(stage: Option<BootStage>) -> bool {
    matches!(stage, Some(BootStage::AgentReady))
}

/// Record a stage on the pane. Only SpawnFailed carries a reason, so any other
/// stage clears a previously latched failure.
pub(crate) fn apply_boot_stage(pane: &mut Pane, stage: BootStage, reason: Option<&str>) {
    pane.boot_stage = Some(stage);
    pane.boot_failure_reason = (stage == BootStage::SpawnFailed)
        .then(|| reason.map(str::to_string))
        .flatten();
}

impl BootStageSink for ServiceContext {
    fn record(&self, pane_id: &str, stage: BootStage, reason: Option<&str>) {
        self.emit_boot_stage(pane_id, stage, reason);
    }
}

impl ServiceContext {
    /// Set the pane's current boot stage (for late attachers reading the
    /// snapshot) and fan the transition out on the event bridge. Fire-and-forget
    /// — never blocks the launch thread.
    pub(crate) fn emit_boot_stage(&self, pane_id: &str, stage: BootStage, reason: Option<&str>) {
        if let Some(p) = self.mux.write().get_pane_mut(pane_id) {
            apply_boot_stage(p, stage, reason);
        }
        super::boot_wait::notify_boot_changed();
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "pane-boot-stage",
            serde_json::json!({
                "pane_id": pane_id,
                "stage": stage,
                "reason": reason,
            }),
        );
        if stage == BootStage::AgentReady {
            self.schedule_planner_replay(pane_id);
            self.heal_recovery_state_if_ready();
        }
    }

    /// Proven health for the recovery record: every exact role binding has a
    /// rehydrated agent state. Read at real state transitions, never a timer.
    pub(crate) fn heal_recovery_state_if_ready(&self) {
        if !crate::crash::lifecycle::recovery_heal_pending() {
            return;
        }
        let healthy = {
            let mux = self.mux.read();
            mux.snapshot()
                .panes
                .values()
                .filter_map(|pane| {
                    if pane.role.is_none() || pane.agent_type.is_none() {
                        return None;
                    }
                    crate::crash::lifecycle::BindingIdentity::healthy_lazy_restored(
                        &pane.id,
                        pane.agent_session_id.as_deref(),
                        pane.agent_state.is_some(),
                    )
                })
                .collect::<Vec<_>>()
        };
        if crate::crash::lifecycle::heal_current_epoch(&healthy) {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "recovery-state-changed",
                crate::crash::lifecycle::recovery_state(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct MockSink {
        events: Mutex<Vec<(String, BootStage, Option<String>)>>,
    }

    impl BootStageSink for MockSink {
        fn record(&self, pane_id: &str, stage: BootStage, reason: Option<&str>) {
            self.events
                .lock()
                .push((pane_id.to_string(), stage, reason.map(str::to_string)));
        }
    }

    impl MockSink {
        fn stages(&self) -> Vec<BootStage> {
            self.events.lock().iter().map(|(_, s, _)| *s).collect()
        }
    }

    #[test]
    fn happy_path_emits_stages_in_order() {
        let sink = MockSink::default();
        for stage in BootStage::HAPPY_PATH {
            sink.record("pane-1", stage, None);
        }
        assert_eq!(sink.stages(), BootStage::HAPPY_PATH.to_vec());
        assert!(sink
            .events
            .lock()
            .iter()
            .all(|(pid, _, r)| pid == "pane-1" && r.is_none()));
    }

    #[test]
    fn failure_path_ends_at_spawn_failed_with_reason_and_no_ready() {
        let sink = MockSink::default();
        sink.record("pane-2", BootStage::PtySpawned, None);
        sink.record("pane-2", BootStage::ShellReady, None);
        sink.record(
            "pane-2",
            BootStage::SpawnFailed,
            Some("launch_cmd failed: boom"),
        );

        let stages = sink.stages();
        assert!(!stages.contains(&BootStage::AgentReady));
        assert_eq!(stages.last(), Some(&BootStage::SpawnFailed));
        let (_, last_stage, last_reason) = sink.events.lock().last().cloned().unwrap();
        assert_eq!(last_stage, BootStage::SpawnFailed);
        assert_eq!(last_reason.as_deref(), Some("launch_cmd failed: boom"));
    }

    #[test]
    fn stage_serializes_snake_case() {
        let json = serde_json::to_string(&BootStage::AgentLaunching).unwrap();
        assert_eq!(json, "\"agent_launching\"");
        assert_eq!(BootStage::SpawnFailed.as_str(), "spawn_failed");
    }
}
