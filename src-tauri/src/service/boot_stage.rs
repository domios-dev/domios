//! Truthful tab-boot progress. Stages are emitted at the REAL launch
//! transitions in `orchestrator::launch` — never timer-driven — so late UI
//! attachers and live subscribers see where a spawn actually is.

use super::ServiceContext;
use crate::multiplexer::types::Tab;

pub use codeterm_types::pty::{BootStage, BootStageSink};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestartReadiness {
    Ready {
        pty_id: String,
    },
    Failed {
        pty_id: Option<String>,
        reason: String,
    },
    TimedOut {
        pty_id: Option<String>,
    },
}

pub(crate) fn classify_restart_readiness(
    pty_id: Option<&str>,
    stage: Option<BootStage>,
    failure_reason: Option<&str>,
    old_pty_id: &str,
) -> Option<RestartReadiness> {
    let new_pty_id = pty_id.filter(|pty_id| *pty_id != old_pty_id);
    match (new_pty_id, stage) {
        (Some(pty_id), Some(BootStage::AgentReady)) => Some(RestartReadiness::Ready {
            pty_id: pty_id.to_string(),
        }),
        (pty_id, Some(BootStage::SpawnFailed)) => Some(RestartReadiness::Failed {
            pty_id: pty_id.map(str::to_string),
            reason: failure_reason
                .unwrap_or("new agent launch failed")
                .to_string(),
        }),
        _ => None,
    }
}

pub(crate) fn wait_for_restart_readiness(
    mux: &crate::MuxState,
    tab_id: &str,
    old_pty_id: &str,
    timeout: std::time::Duration,
) -> RestartReadiness {
    let inspect = || {
        let mux = mux.read();
        match mux.get_tab(tab_id) {
            Some(tab) => classify_restart_readiness(
                tab.pty_id.as_deref(),
                tab.boot_stage,
                tab.boot_failure_reason.as_deref(),
                old_pty_id,
            ),
            None => Some(RestartReadiness::Failed {
                pty_id: None,
                reason: format!("Tab {tab_id} disappeared during restart"),
            }),
        }
    };
    let mut observed = inspect();
    let signalled = super::boot_wait::wait_until(std::time::Instant::now() + timeout, || {
        observed = inspect();
        observed.is_some()
    });
    if signalled {
        return observed.expect("signalled restart readiness");
    }
    let pty_id = mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.pty_id.clone())
        .filter(|pty_id| pty_id != old_pty_id);
    RestartReadiness::TimedOut { pty_id }
}

/// Record a stage on the tab. Only SpawnFailed carries a reason, so any other
/// stage clears a previously latched failure.
pub(crate) fn apply_boot_stage(tab: &mut Tab, stage: BootStage, reason: Option<&str>) {
    tab.boot_stage = Some(stage);
    tab.boot_failure_reason = (stage == BootStage::SpawnFailed)
        .then(|| reason.map(str::to_string))
        .flatten();
}

impl BootStageSink for ServiceContext {
    fn record(&self, tab_id: &str, stage: BootStage, reason: Option<&str>) {
        self.emit_boot_stage(tab_id, stage, reason);
    }
}

impl ServiceContext {
    /// Set the tab's current boot stage (for late attachers reading the
    /// snapshot) and fan the transition out on the event bridge. Fire-and-forget
    /// — never blocks the launch thread.
    pub(crate) fn emit_boot_stage(&self, tab_id: &str, stage: BootStage, reason: Option<&str>) {
        if let Some(p) = self.mux.write().get_tab_mut(tab_id) {
            apply_boot_stage(p, stage, reason);
        }
        super::boot_wait::notify_boot_changed();
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "tab-boot-stage",
            serde_json::json!({
                "tab_id": tab_id,
                "stage": stage,
                "reason": reason,
            }),
        );
        if stage == BootStage::AgentReady {
            self.schedule_planner_replay(tab_id);
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
                .tabs
                .values()
                .filter_map(|tab| {
                    if tab.role.is_none() || tab.agent_type.is_none() {
                        return None;
                    }
                    crate::crash::lifecycle::BindingIdentity::healthy_lazy_restored(
                        &tab.id,
                        tab.agent_session_id.as_deref(),
                        tab.agent_state.is_some(),
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
        fn record(&self, tab_id: &str, stage: BootStage, reason: Option<&str>) {
            self.events
                .lock()
                .push((tab_id.to_string(), stage, reason.map(str::to_string)));
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
            sink.record("tab-1", stage, None);
        }
        assert_eq!(sink.stages(), BootStage::HAPPY_PATH.to_vec());
        assert!(sink
            .events
            .lock()
            .iter()
            .all(|(pid, _, r)| pid == "tab-1" && r.is_none()));
    }

    #[test]
    fn failure_path_ends_at_spawn_failed_with_reason_and_no_ready() {
        let sink = MockSink::default();
        sink.record("tab-2", BootStage::PtySpawned, None);
        sink.record("tab-2", BootStage::ShellReady, None);
        sink.record(
            "tab-2",
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

    /// An options block surfaced exactly as the live path surfaces it: the
    /// structural detector on the tab's own attributed grid. No vendor wording.
    fn blocking_prompt_from_screen() -> codeterm_types::pty::PendingPrompt {
        let mut parser = vt100::Parser::new(24, 80, 0);
        for line in [
            "Select an option",
            "",
            "\u{276f} 1. first choice",
            "  2. second choice",
            "  3. third choice",
            "",
            "  enter to select \u{b7} esc to cancel",
        ] {
            parser.process(line.as_bytes());
            parser.process(b"\r\n");
        }
        let input = crate::agent::native_prompt::NativePromptInput {
            screen: codeterm_pty::attributed_screen::AttributedScreen::from_vt100(parser.screen()),
            cursor_hidden: true,
            alt_screen: false,
        };
        crate::agent::native_prompt::pending_prompt_from_input(&input)
            .expect("mechanism probe: the structural detector surfaces this options block")
    }

    fn tab_with_prompt(prompt: Option<codeterm_types::pty::PendingPrompt>) -> Tab {
        let (mut state, _root) = crate::multiplexer::state::MultiplexerState::new();
        let workspace_id = state.list_workspaces()[0].id.clone();
        let tab_id = state.create_tab(&workspace_id).unwrap();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.pending_prompt = prompt;
        tab.clone()
    }

    #[test]
    fn inferred_prompt_never_withholds_agent_ready() {
        let mut tab = tab_with_prompt(Some(blocking_prompt_from_screen()));
        apply_boot_stage(&mut tab, BootStage::AgentReady, None);
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    }

    /// The over-correction guard: a fix that never reports ready also never
    /// reports ready while blocked, and is just as wrong.
    #[test]
    fn agent_ready_is_recorded_when_nothing_owns_the_input() {
        let mut tab = tab_with_prompt(None);
        apply_boot_stage(&mut tab, BootStage::AgentReady, None);
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    }

    #[test]
    fn stage_serializes_snake_case() {
        let json = serde_json::to_string(&BootStage::AgentLaunching).unwrap();
        assert_eq!(json, "\"agent_launching\"");
        assert_eq!(BootStage::SpawnFailed.as_str(), "spawn_failed");
    }
}
