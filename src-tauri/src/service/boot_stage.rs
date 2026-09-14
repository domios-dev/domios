//! Truthful tab-boot progress. Stages are emitted at the REAL launch
//! transitions in `orchestrator::launch` — never timer-driven — so late UI
//! attachers and live subscribers see where a spawn actually is.

use super::ServiceContext;
use crate::multiplexer::types::Tab;

pub use codeterm_types::pty::{BootStage, BootStageSink, ProvisionalLaunch};

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

/// Record a stage on the tab. Only a reason-carrying stage keeps its reason, so
/// any other stage clears a previously latched failure and its launch identity.
pub(crate) fn apply_boot_stage(tab: &mut Tab, stage: BootStage, reason: Option<&str>) {
    tab.boot_stage = Some(stage);
    tab.boot_failure_reason = stage
        .carries_reason()
        .then(|| reason.map(str::to_string))
        .flatten();
    if !stage.is_provisional() {
        tab.provisional_launch = None;
    }
}

/// The launch a provisional stage describes, captured while the tab still owns
/// the PTY that was launched on and still carries that launch's binding nonce.
/// A launch that already reached a verdict — ready or failed — is never latched.
pub(crate) fn provisional_launch_for(
    tab: &Tab,
    pty_id: &str,
    reason: &str,
) -> Option<ProvisionalLaunch> {
    let settled = matches!(
        tab.boot_stage,
        Some(BootStage::AgentReady | BootStage::SpawnFailed)
    );
    if settled || tab.pty_id.as_deref() != Some(pty_id) {
        return None;
    }
    Some(ProvisionalLaunch {
        pty_id: pty_id.to_string(),
        binding_nonce: tab.session_binding_nonce.clone()?,
        reason: reason.to_string(),
    })
}

/// The smallest authoritative late-readiness event: a nonce-verified session
/// binding naming both the PTY and the launch attempt the stage was latched on,
/// while the tab still owns that PTY, that nonce, and an accepted session. One
/// rule for every provisional source — a deadline and a missing binding are the
/// same uncertainty about the same launch.
pub(crate) fn late_readiness_retires_provisional_launch(
    tab: &Tab,
    evidence: &crate::service::ProviderSessionBinding,
) -> bool {
    if !tab.boot_stage.is_some_and(|stage| stage.is_provisional()) {
        return false;
    }
    let Some(latch) = tab.provisional_launch.as_ref() else {
        return false;
    };
    latch.pty_id == evidence.pty_id
        && tab.pty_id.as_deref() == Some(latch.pty_id.as_str())
        && latch.binding_nonce == evidence.binding_nonce
        && tab.session_binding_nonce.as_deref() == Some(latch.binding_nonce.as_str())
        && tab.launch_session_authenticated()
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
        let reason = stage.carries_reason().then_some(reason).flatten();
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

    /// The output deadline elapsed: provisional evidence about one PTY and one
    /// launch attempt, never a verdict. The reason rides along so the deadline
    /// stays diagnosable whichever way recovery goes.
    pub(crate) fn record_launch_output_deadline(&self, tab_id: &str, pty_id: &str, reason: &str) {
        self.record_provisional_launch(tab_id, pty_id, BootStage::LaunchDeadlineElapsed, reason);
    }

    /// Latch a provisional launch: uncertainty about one PTY and one launch
    /// attempt, carrying the evidence that made this observer give up. Only a
    /// nonce-verified binding for the same pair retires it.
    pub(crate) fn record_provisional_launch(
        &self,
        tab_id: &str,
        pty_id: &str,
        stage: BootStage,
        reason: &str,
    ) {
        debug_assert!(stage.is_provisional());
        let latch = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| provisional_launch_for(tab, pty_id, reason))
        };
        let Some(latch) = latch else {
            log::info!(
                "[boot-stage] Tab {tab_id} {} ignored; launch on PTY {pty_id} is no longer current",
                stage.as_str()
            );
            return;
        };
        self.emit_boot_stage(tab_id, stage, Some(reason));
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            if tab.boot_stage == Some(stage) {
                tab.provisional_launch = Some(latch);
            }
        }
        super::boot_wait::notify_boot_changed();
    }

    /// Retire a provisional launch on authoritative late readiness from the
    /// same PTY and launch attempt, whichever source latched it. Driven by the
    /// binding's arrival, so no further PTY output is required and nothing else
    /// retires it.
    pub(crate) fn retire_provisional_launch(
        &self,
        tab_id: &str,
        evidence: &crate::service::ProviderSessionBinding,
    ) -> bool {
        let retires = self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| late_readiness_retires_provisional_launch(tab, evidence));
        if !retires {
            return false;
        }
        log::info!(
            "[boot-stage] Tab {tab_id} provisional launch retired by late readiness on its own PTY {} and launch attempt",
            evidence.pty_id
        );
        self.emit_boot_stage(tab_id, BootStage::AgentReady, None);
        true
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

    const EVERY_STAGE: [BootStage; 14] = [
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
        BootStage::LaunchDeadlineElapsed,
        BootStage::LaunchBindingUnconfirmed,
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
                BootStage::LaunchDeadlineElapsed => 11,
                BootStage::LaunchBindingUnconfirmed => 12,
                BootStage::SpawnFailed => 13,
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

    fn binding(pty_id: &str, nonce: &str) -> crate::service::ProviderSessionBinding {
        crate::service::ProviderSessionBinding {
            pty_id: pty_id.to_string(),
            binding_nonce: nonce.to_string(),
        }
    }

    const DEADLINE_REASON: &str = "agent output deadline elapsed";

    /// A launch that wrote its command and then produced nothing observable.
    fn launching_tab(pty_id: &str, nonce: &str) -> Tab {
        let mut tab = tab_with_prompt(None);
        tab.pty_id = Some(pty_id.into());
        tab.agent_type = Some("codex".into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = None;
        tab.agent_session_id = None;
        tab.boot_stage = Some(BootStage::ShellReady);
        tab
    }

    /// Mirrors `record_provisional_launch`: the latch rule gates the write, and
    /// the stage lands together with the evidence that produced it.
    fn latch_provisional(tab: &mut Tab, pty_id: &str, stage: BootStage, reason: &str) {
        let latch = provisional_launch_for(tab, pty_id, reason)
            .expect("a current launch is latchable while it is unresolved");
        apply_boot_stage(tab, stage, Some(reason));
        tab.provisional_launch = Some(latch);
    }

    fn latch_deadline(tab: &mut Tab, pty_id: &str) {
        latch_provisional(
            tab,
            pty_id,
            BootStage::LaunchDeadlineElapsed,
            DEADLINE_REASON,
        );
    }

    /// The evidence the first-run producer carries when the payload reached the
    /// PTY and the authoritative binding never arrived.
    const UNCONFIRMED_REASON: &str =
        "submit_unconfirmed: launch payload written, pending evidence \
         retained; authoritative provider session binding was not reported";

    fn latch_submit_unconfirmed(tab: &mut Tab, pty_id: &str) {
        tab.boot_stage = Some(BootStage::ProviderReady);
        latch_provisional(
            tab,
            pty_id,
            BootStage::LaunchBindingUnconfirmed,
            UNCONFIRMED_REASON,
        );
    }

    /// The agent naming itself on the latched launch: its own accepted session
    /// plus this attempt's nonce echoed back.
    fn report_own_launch(tab: &mut Tab, nonce: &str) {
        tab.agent_session_id = Some("01a090f2".into());
        tab.reported_session_binding_nonce = Some(nonce.into());
    }

    #[test]
    fn deadline_failure_is_provisional_until_current_attempt_late_readiness() {
        let mut tab = launching_tab("pty-a", "nonce-a");
        latch_deadline(&mut tab, "pty-a");

        assert_eq!(tab.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
        assert_ne!(tab.boot_stage, Some(BootStage::SpawnFailed));
        assert!(tab.boot_stage.is_some_and(|stage| stage.is_provisional()));
        assert_eq!(tab.boot_failure_reason.as_deref(), Some(DEADLINE_REASON));
        assert_eq!(
            tab.provisional_launch
                .as_ref()
                .map(|latch| latch.reason.as_str()),
            Some(DEADLINE_REASON),
            "the deadline stays diagnosable while it is provisional",
        );
        assert!(
            !late_readiness_retires_provisional_launch(&tab, &binding("pty-a", "nonce-a")),
            "an unechoed nonce is not the agent naming itself",
        );

        report_own_launch(&mut tab, "nonce-a");
        assert!(late_readiness_retires_provisional_launch(
            &tab,
            &binding("pty-a", "nonce-a")
        ));

        apply_boot_stage(&mut tab, BootStage::AgentReady, None);
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        assert_eq!(tab.boot_failure_reason, None);
        assert_eq!(tab.provisional_launch, None);
    }

    /// The retirement reads the launch record, never the grid: a PTY that never
    /// paints another cell after the deadline still recovers.
    #[test]
    fn late_readiness_without_screen_update_reconciles_current_attempt() {
        let screen_state = codeterm_util::lock_hold::TimedMutex::new(
            "screen_state",
            codeterm_pty::pty::screen_state::ScreenState::new(),
        );
        let signal = screen_state.lock().arm_screen_change_signal("pty-a");
        let quiet = signal.generation();

        let mut tab = launching_tab("pty-a", "nonce-a");
        latch_deadline(&mut tab, "pty-a");
        report_own_launch(&mut tab, "nonce-a");

        assert!(late_readiness_retires_provisional_launch(
            &tab,
            &binding("pty-a", "nonce-a")
        ));
        apply_boot_stage(&mut tab, BootStage::AgentReady, None);

        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        assert_eq!(
            signal.generation(),
            quiet,
            "recovery consumed no screen update",
        );
        assert!(!signal.wait_after(quiet, std::time::Duration::ZERO));
    }

    /// Mirrors `retire_provisional_launch`: the rule is the only gate on
    /// the write, so evidence it rejects leaves the latched launch untouched.
    fn offer_late_readiness(tab: &mut Tab, evidence: &crate::service::ProviderSessionBinding) {
        if late_readiness_retires_provisional_launch(tab, evidence) {
            apply_boot_stage(tab, BootStage::AgentReady, None);
        }
    }

    #[test]
    fn stale_or_invalid_evidence_cannot_unlock_deadline_failure() {
        // A prior PTY, a superseded attempt, and both at once.
        let mut latched = launching_tab("pty-a", "nonce-a");
        latch_deadline(&mut latched, "pty-a");
        report_own_launch(&mut latched, "nonce-a");
        let latch = latched.provisional_launch.clone();
        for evidence in [
            binding("pty-old", "nonce-a"),
            binding("pty-a", "nonce-old"),
            binding("pty-old", "nonce-old"),
        ] {
            offer_late_readiness(&mut latched, &evidence);
        }
        assert_eq!(latched.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
        assert_eq!(latched.provisional_launch, latch);
        assert_eq!(
            latched.boot_failure_reason.as_deref(),
            Some(DEADLINE_REASON)
        );

        // A bare session id, with no nonce echoed at all, never unlocks input.
        let mut bare_session = launching_tab("pty-a", "nonce-a");
        latch_deadline(&mut bare_session, "pty-a");
        bare_session.agent_session_id = Some("01a090f2".into());
        offer_late_readiness(&mut bare_session, &binding("pty-a", "nonce-a"));
        assert_eq!(
            bare_session.boot_stage,
            Some(BootStage::LaunchDeadlineElapsed)
        );

        // An invalid nonce: the agent echoed something other than this attempt,
        // so neither the attempt's nonce nor the echoed one is authoritative.
        let mut invalid = launching_tab("pty-a", "nonce-a");
        latch_deadline(&mut invalid, "pty-a");
        report_own_launch(&mut invalid, "nonce-b");
        for evidence in [binding("pty-a", "nonce-a"), binding("pty-a", "nonce-b")] {
            offer_late_readiness(&mut invalid, &evidence);
        }
        assert_eq!(invalid.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
        assert_eq!(
            invalid.session_binding_nonce.as_deref(),
            Some("nonce-a"),
            "rejected evidence never overwrites the current attempt",
        );

        // The tab relinked to a new PTY after the deadline: the latched launch
        // is no longer current, so nothing can speak for it.
        let mut relinked = launching_tab("pty-a", "nonce-a");
        latch_deadline(&mut relinked, "pty-a");
        report_own_launch(&mut relinked, "nonce-a");
        relinked.pty_id = Some("pty-b".into());
        for evidence in [binding("pty-a", "nonce-a"), binding("pty-b", "nonce-a")] {
            offer_late_readiness(&mut relinked, &evidence);
        }
        assert_eq!(relinked.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
    }

    #[test]
    fn genuine_failed_launch_remains_blocked() {
        let mut failed = launching_tab("pty-a", "nonce-a");
        apply_boot_stage(
            &mut failed,
            BootStage::SpawnFailed,
            Some("PtySpawnFailed: boom"),
        );
        assert_eq!(failed.boot_stage, Some(BootStage::SpawnFailed));
        assert_eq!(
            failed.boot_failure_reason.as_deref(),
            Some("PtySpawnFailed: boom")
        );
        assert_eq!(failed.provisional_launch, None);

        report_own_launch(&mut failed, "nonce-a");
        assert!(
            !late_readiness_retires_provisional_launch(&failed, &binding("pty-a", "nonce-a")),
            "a genuine failure is not a deadline and is not retired by this rule",
        );
        assert_eq!(
            honest_boot_stage(BootStage::SpawnFailed, Some(BootStage::ShellReady), true),
            BootStage::SpawnFailed,
        );

        // A provisional stage is evidence about one observer, not a shield, so a
        // later spawn failure still latches from either source.
        let latches: [fn(&mut Tab, &str); 2] = [latch_deadline, latch_submit_unconfirmed];
        for latch in latches {
            let mut latched = launching_tab("pty-a", "nonce-a");
            latch(&mut latched, "pty-a");
            apply_boot_stage(&mut latched, BootStage::SpawnFailed, Some("PtyEof"));
            assert_eq!(latched.boot_stage, Some(BootStage::SpawnFailed));
            assert_eq!(latched.boot_failure_reason.as_deref(), Some("PtyEof"));
            assert_eq!(latched.provisional_launch, None);
            report_own_launch(&mut latched, "nonce-a");
            assert!(!late_readiness_retires_provisional_launch(
                &latched,
                &binding("pty-a", "nonce-a")
            ));
        }

        // A launch that already failed is never latched as provisional either.
        assert!(provisional_launch_for(&failed, "pty-a", UNCONFIRMED_REASON).is_none());
    }

    /// The producer refuses to latch a launch the tab no longer owns, so a
    /// deadline from a superseded attempt cannot overwrite the current one.
    #[test]
    fn a_deadline_never_latches_a_launch_the_tab_moved_on_from() {
        let mut relinked = launching_tab("pty-b", "nonce-b");
        assert!(provisional_launch_for(&relinked, "pty-a", DEADLINE_REASON).is_none());

        for settled in [BootStage::AgentReady, BootStage::SpawnFailed] {
            relinked.boot_stage = Some(settled);
            assert!(provisional_launch_for(&relinked, "pty-b", DEADLINE_REASON).is_none());
        }

        let mut nonceless = launching_tab("pty-b", "nonce-b");
        nonceless.session_binding_nonce = None;
        assert!(provisional_launch_for(&nonceless, "pty-b", DEADLINE_REASON).is_none());
    }

    /// The live #477 gap: a launch whose payload reached the PTY and whose
    /// binding never arrived is retired by the same rule as a deadline, and by
    /// nothing weaker.
    #[test]
    fn late_binding_retires_submit_unconfirmed_for_current_attempt() {
        let screen_state = codeterm_util::lock_hold::TimedMutex::new(
            "screen_state",
            codeterm_pty::pty::screen_state::ScreenState::new(),
        );
        let signal = screen_state.lock().arm_screen_change_signal("pty-a");
        let quiet = signal.generation();

        let mut tab = launching_tab("pty-a", "nonce-a");
        latch_submit_unconfirmed(&mut tab, "pty-a");

        assert_eq!(tab.boot_stage, Some(BootStage::LaunchBindingUnconfirmed));
        assert_ne!(tab.boot_stage, Some(BootStage::SpawnFailed));
        assert!(tab.boot_stage.is_some_and(|stage| stage.is_provisional()));
        assert_eq!(tab.boot_failure_reason.as_deref(), Some(UNCONFIRMED_REASON));
        assert_eq!(
            tab.provisional_launch
                .as_ref()
                .map(|latch| (latch.pty_id.as_str(), latch.binding_nonce.as_str())),
            Some(("pty-a", "nonce-a")),
            "the provisional state names the PTY and attempt it describes",
        );
        assert!(
            !late_readiness_retires_provisional_launch(&tab, &binding("pty-a", "nonce-a")),
            "an unechoed nonce is not the agent naming itself",
        );

        report_own_launch(&mut tab, "nonce-a");
        assert!(late_readiness_retires_provisional_launch(
            &tab,
            &binding("pty-a", "nonce-a")
        ));
        apply_boot_stage(&mut tab, BootStage::AgentReady, None);

        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        assert_eq!(tab.boot_failure_reason, None);
        assert_eq!(tab.provisional_launch, None);
        assert_eq!(
            signal.generation(),
            quiet,
            "retirement consumed no screen update",
        );
        assert!(!signal.wait_after(quiet, std::time::Duration::ZERO));
    }

    #[test]
    fn stale_evidence_cannot_retire_submit_unconfirmed() {
        // A prior PTY, a superseded attempt, and both at once.
        let mut latched = launching_tab("pty-a", "nonce-a");
        latch_submit_unconfirmed(&mut latched, "pty-a");
        report_own_launch(&mut latched, "nonce-a");
        let latch = latched.provisional_launch.clone();
        for evidence in [
            binding("pty-old", "nonce-a"),
            binding("pty-a", "nonce-old"),
            binding("pty-old", "nonce-old"),
        ] {
            offer_late_readiness(&mut latched, &evidence);
        }
        assert_eq!(
            latched.boot_stage,
            Some(BootStage::LaunchBindingUnconfirmed)
        );
        assert_eq!(latched.provisional_launch, latch);
        assert_eq!(
            latched.boot_failure_reason.as_deref(),
            Some(UNCONFIRMED_REASON),
            "rejected evidence never overwrites the retained delivery evidence",
        );

        // A bare session id, with no nonce echoed at all, never unlocks input.
        let mut bare_session = launching_tab("pty-a", "nonce-a");
        latch_submit_unconfirmed(&mut bare_session, "pty-a");
        bare_session.agent_session_id = Some("01a090f2".into());
        offer_late_readiness(&mut bare_session, &binding("pty-a", "nonce-a"));
        assert_eq!(
            bare_session.boot_stage,
            Some(BootStage::LaunchBindingUnconfirmed)
        );

        // An invalid nonce: the agent echoed something other than this attempt.
        let mut invalid = launching_tab("pty-a", "nonce-a");
        latch_submit_unconfirmed(&mut invalid, "pty-a");
        report_own_launch(&mut invalid, "nonce-b");
        for evidence in [binding("pty-a", "nonce-a"), binding("pty-a", "nonce-b")] {
            offer_late_readiness(&mut invalid, &evidence);
        }
        assert_eq!(
            invalid.boot_stage,
            Some(BootStage::LaunchBindingUnconfirmed)
        );
        assert_eq!(
            invalid.session_binding_nonce.as_deref(),
            Some("nonce-a"),
            "rejected evidence never overwrites the current attempt",
        );

        // The tab relinked to a new PTY: the latched launch is no longer
        // current, so nothing can speak for it.
        let mut relinked = launching_tab("pty-a", "nonce-a");
        latch_submit_unconfirmed(&mut relinked, "pty-a");
        report_own_launch(&mut relinked, "nonce-a");
        relinked.pty_id = Some("pty-b".into());
        for evidence in [binding("pty-a", "nonce-a"), binding("pty-b", "nonce-a")] {
            offer_late_readiness(&mut relinked, &evidence);
        }
        assert_eq!(
            relinked.boot_stage,
            Some(BootStage::LaunchBindingUnconfirmed)
        );
    }

    #[test]
    fn stage_serializes_snake_case() {
        let json = serde_json::to_string(&BootStage::AgentLaunching).unwrap();
        assert_eq!(json, "\"agent_launching\"");
        assert_eq!(BootStage::SpawnFailed.as_str(), "spawn_failed");
        assert_eq!(
            serde_json::to_string(&BootStage::LaunchDeadlineElapsed).unwrap(),
            "\"launch_deadline_elapsed\"",
        );
        assert_eq!(
            BootStage::LaunchDeadlineElapsed.as_str(),
            "launch_deadline_elapsed"
        );
        assert_eq!(
            serde_json::to_string(&BootStage::LaunchBindingUnconfirmed).unwrap(),
            "\"launch_binding_unconfirmed\"",
        );
        assert_eq!(
            BootStage::LaunchBindingUnconfirmed.as_str(),
            "launch_binding_unconfirmed"
        );
    }
}
