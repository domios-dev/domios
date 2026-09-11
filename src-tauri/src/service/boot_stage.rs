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

/// A spawn failure cannot describe a tab whose provider is already up — its own
/// TUI frame, or a sealed launch — and whose agent proved that launch with a
/// nonce-verified session binding. Two independent observations of a running
/// process outrank the one observer that gave up on it. Faults that are
/// themselves evidence the process died void the proof first, and a tab still
/// short of `ProviderReady` is not covered at all, so genuine failures latch.
pub(crate) fn honest_boot_stage(
    stage: BootStage,
    current: Option<BootStage>,
    launch_session_authenticated: bool,
) -> BootStage {
    let provider_up = matches!(
        current,
        Some(BootStage::ProviderReady | BootStage::AgentReady)
    );
    if stage == BootStage::SpawnFailed && provider_up && launch_session_authenticated {
        BootStage::AgentReady
    } else {
        stage
    }
}

/// A latched failure that the tab's own nonce-verified session binding
/// contradicts. The binding is the agent naming itself on its own launch.
pub(crate) fn binding_disproves_latched_failure(tab: &Tab) -> bool {
    tab.boot_stage == Some(BootStage::SpawnFailed) && tab.launch_session_authenticated()
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
        let stage = self.stage_reconciled_with_session_binding(tab_id, stage);
        let reason = (stage == BootStage::SpawnFailed)
            .then_some(reason)
            .flatten();
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
        self.emit_session_binding_state(tab_id);
        // A boot stage is the only moment a tab's agent liveness can change on
        // the launch side; the restore wave settles here rather than on a tick.
        self.settle_restore_report();
        if stage == BootStage::AgentReady {
            // Sealing transition: this is the only place an operation becomes
            // restorable, and only if provider + session already committed.
            self.record_lifecycle(
                tab_id,
                crate::session::lifecycle::LifecycleTransition::LaunchReady,
            );
            self.schedule_planner_replay(tab_id);
            self.schedule_parked_user_send(tab_id);
            self.heal_recovery_state_if_ready();
        }
        if stage == BootStage::SpawnFailed {
            self.record_lifecycle_end(
                tab_id,
                crate::session::lifecycle::LifecycleTransition::RolledBack {
                    reason: reason.unwrap_or("spawn failed").to_string(),
                },
            );
        }
    }

    fn stage_reconciled_with_session_binding(&self, tab_id: &str, stage: BootStage) -> BootStage {
        if stage != BootStage::SpawnFailed {
            return stage;
        }
        let honest = {
            let mux = self.mux.read();
            match mux.get_tab(tab_id) {
                Some(tab) => {
                    honest_boot_stage(stage, tab.boot_stage, tab.launch_session_authenticated())
                }
                None => stage,
            }
        };
        if honest != stage {
            log::info!(
                "[boot-stage] Tab {tab_id} spawn_failed refused: its live agent proved this launch"
            );
        }
        honest
    }

    /// A nonce-verified session-start report is the agent naming itself on its
    /// own launch, which disproves a failure another observer already latched.
    /// Event-driven by the proof's arrival — never a poll or a retry.
    pub(crate) fn reconcile_boot_stage_with_session_binding(&self, tab_id: &str) {
        let contradicted = self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(binding_disproves_latched_failure);
        if contradicted {
            log::info!(
                "[boot-stage] Tab {tab_id} spawn_failed disproved by its nonce-verified session binding"
            );
            self.emit_boot_stage(tab_id, BootStage::AgentReady, None);
        }
    }

    /// A user send refused before the agent was ready waits here, not in the
    /// void. `AgentReady` is the only precise point at which input is known to
    /// be accepted, so it is the only place the parked text is delivered.
    fn schedule_parked_user_send(&self, tab_id: &str) {
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        std::thread::spawn(move || svc.deliver_parked_user_send(&tab_id));
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

    const EVERY_STAGE: [BootStage; 12] = [
        BootStage::PtyStarting,
        BootStage::PtySpawned,
        BootStage::ReaderReady,
        BootStage::ShellReady,
        BootStage::AgentLaunching,
        BootStage::LaunchWritten,
        BootStage::ProviderReady,
        BootStage::PreparingRoleInstructions,
        BootStage::DeliveringInstructions,
        BootStage::AgentReady,
        BootStage::PtyReattached,
        BootStage::SpawnFailed,
    ];

    /// Keeps the cross-product below honest: a new variant fails to compile
    /// here until `EVERY_STAGE` learns about it.
    #[test]
    fn every_stage_covers_the_enum() {
        fn index(stage: BootStage) -> usize {
            match stage {
                BootStage::PtyStarting => 0,
                BootStage::PtySpawned => 1,
                BootStage::ReaderReady => 2,
                BootStage::ShellReady => 3,
                BootStage::AgentLaunching => 4,
                BootStage::LaunchWritten => 5,
                BootStage::ProviderReady => 6,
                BootStage::PreparingRoleInstructions => 7,
                BootStage::DeliveringInstructions => 8,
                BootStage::AgentReady => 9,
                BootStage::PtyReattached => 10,
                BootStage::SpawnFailed => 11,
            }
        }
        let mut seen = vec![false; EVERY_STAGE.len()];
        for stage in EVERY_STAGE {
            seen[index(stage)] = true;
        }
        assert!(seen.into_iter().all(|covered| covered));
    }

    fn authenticated_tab(current: Option<BootStage>) -> Tab {
        let mut tab = tab_with_prompt(None);
        tab.boot_stage = current;
        tab.agent_type = Some("codex".into());
        tab.agent_session_id = Some("01a090f2".into());
        tab.session_binding_nonce = Some("nonce-a".into());
        tab.reported_session_binding_nonce = Some("nonce-a".into());
        tab
    }

    #[test]
    fn launch_nonce_is_proven_only_when_this_launch_echoed_it() {
        assert!(authenticated_tab(Some(BootStage::ProviderReady)).launch_session_authenticated());

        let mut unreported = authenticated_tab(Some(BootStage::ProviderReady));
        unreported.reported_session_binding_nonce = None;
        assert!(!unreported.launch_session_authenticated());

        let mut superseded = authenticated_tab(Some(BootStage::ProviderReady));
        superseded.session_binding_nonce = Some("nonce-b".into());
        assert!(!superseded.launch_session_authenticated());

        let mut nonceless = authenticated_tab(Some(BootStage::ProviderReady));
        nonceless.session_binding_nonce = None;
        nonceless.reported_session_binding_nonce = None;
        assert!(!nonceless.launch_session_authenticated());

        let mut unbound = authenticated_tab(Some(BootStage::ProviderReady));
        unbound.agent_session_id = None;
        assert!(!unbound.launch_session_authenticated());
    }

    /// The contradiction the daemon used to report: a live, nonce-verified
    /// agent behind a tab whose boot stage called the spawn failed.
    #[test]
    fn spawn_failure_is_refused_once_the_agent_proved_this_launch() {
        for current in [BootStage::ProviderReady, BootStage::AgentReady] {
            assert_eq!(
                honest_boot_stage(BootStage::SpawnFailed, Some(current), true),
                BootStage::AgentReady,
            );
        }
    }

    #[test]
    fn spawn_failure_stands_without_a_binding_proof() {
        for current in EVERY_STAGE {
            assert_eq!(
                honest_boot_stage(BootStage::SpawnFailed, Some(current), false),
                BootStage::SpawnFailed,
            );
        }
        assert_eq!(
            honest_boot_stage(BootStage::SpawnFailed, None, false),
            BootStage::SpawnFailed,
        );
    }

    /// The over-correction guard: a proof alone never excuses a launch that
    /// never got the provider up, so a real early failure still latches.
    #[test]
    fn spawn_failure_stands_before_the_provider_is_up() {
        let short_of_provider = EVERY_STAGE
            .into_iter()
            .map(Some)
            .chain(std::iter::once(None))
            .filter(|stage| {
                !matches!(
                    stage,
                    Some(BootStage::ProviderReady) | Some(BootStage::AgentReady)
                )
            });
        for current in short_of_provider {
            assert_eq!(
                honest_boot_stage(BootStage::SpawnFailed, current, true),
                BootStage::SpawnFailed,
            );
        }
    }

    #[test]
    fn no_other_stage_is_ever_rewritten() {
        for stage in EVERY_STAGE
            .into_iter()
            .filter(|s| *s != BootStage::SpawnFailed)
        {
            for current in EVERY_STAGE.into_iter().map(Some).chain([None]) {
                for proven in [false, true] {
                    assert_eq!(honest_boot_stage(stage, current, proven), stage);
                }
            }
        }
    }

    /// The invariant itself, over every reachable combination: a recorded
    /// spawn failure and a proven live launch cannot coexist on one tab.
    #[test]
    fn a_recorded_spawn_failure_never_coexists_with_a_proven_live_launch() {
        for current in EVERY_STAGE.into_iter().map(Some).chain([None]) {
            for stage in EVERY_STAGE {
                let mut tab = authenticated_tab(current);
                let honest =
                    honest_boot_stage(stage, tab.boot_stage, tab.launch_session_authenticated());
                apply_boot_stage(&mut tab, honest, Some("boom"));
                let provider_was_up = matches!(
                    current,
                    Some(BootStage::ProviderReady) | Some(BootStage::AgentReady)
                );
                assert!(
                    !(tab.boot_stage == Some(BootStage::SpawnFailed) && provider_was_up),
                    "spawn_failed recorded from {current:?} on a tab whose agent proved this launch",
                );
            }
        }
    }

    #[test]
    fn a_late_binding_disproves_the_failure_it_arrives_after() {
        let mut failed = authenticated_tab(Some(BootStage::SpawnFailed));
        assert!(binding_disproves_latched_failure(&failed));

        failed.reported_session_binding_nonce = None;
        assert!(
            !binding_disproves_latched_failure(&failed),
            "an unproven launch keeps its failure",
        );

        let ready = authenticated_tab(Some(BootStage::AgentReady));
        assert!(!binding_disproves_latched_failure(&ready));
    }

    #[test]
    fn stage_serializes_snake_case() {
        let json = serde_json::to_string(&BootStage::AgentLaunching).unwrap();
        assert_eq!(json, "\"agent_launching\"");
        assert_eq!(BootStage::SpawnFailed.as_str(), "spawn_failed");
    }
}
