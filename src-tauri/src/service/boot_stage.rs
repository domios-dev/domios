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

/// `replaced_pty_id: None` is an in-place relaunch on the tab's own PTY; its
/// caller retires the prior stage first, so only the new launch can be ready.
pub(crate) fn classify_restart_readiness(
    pty_id: Option<&str>,
    stage: Option<BootStage>,
    failure_reason: Option<&str>,
    replaced_pty_id: Option<&str>,
) -> Option<RestartReadiness> {
    let new_pty_id = pty_id.filter(|pty_id| Some(*pty_id) != replaced_pty_id);
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
    replaced_pty_id: Option<&str>,
    timeout: std::time::Duration,
) -> RestartReadiness {
    let inspect = || {
        let mux = mux.read();
        match mux.get_tab(tab_id) {
            Some(tab) => classify_restart_readiness(
                tab.pty_id.as_deref(),
                tab.boot_stage,
                tab.boot_failure_reason.as_deref(),
                replaced_pty_id,
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
        .filter(|pty_id| Some(pty_id.as_str()) != replaced_pty_id);
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

/// A provider started inside the tab's own shell, outside any owned launch,
/// proved itself with this PTY's nonce. No launch pipeline will ever seal it.
pub(crate) fn binding_observes_in_tab_launch(tab: &Tab) -> bool {
    let shell_stage = matches!(
        tab.boot_stage,
        None | Some(
            BootStage::PtyStarting
                | BootStage::PtySpawned
                | BootStage::ReaderReady
                | BootStage::ShellReady
                | BootStage::PtyReattached
        )
    );
    shell_stage && !tab.agent_booting && tab.pty_id.is_some() && tab.launch_session_authenticated()
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
    binding_nonce: &str,
    reason: &str,
) -> Option<ProvisionalLaunch> {
    let settled = matches!(
        tab.boot_stage,
        Some(BootStage::AgentReady | BootStage::SpawnFailed)
    );
    if settled
        || tab.pty_id.as_deref() != Some(pty_id)
        || tab.session_binding_nonce.as_deref() != Some(binding_nonce)
    {
        return None;
    }
    Some(ProvisionalLaunch {
        pty_id: pty_id.to_string(),
        binding_nonce: binding_nonce.to_string(),
        reason: reason.to_string(),
    })
}

fn apply_provisional_launch(
    tab: &mut Tab,
    pty_id: &str,
    binding_nonce: &str,
    stage: BootStage,
    reason: &str,
) -> bool {
    let Some(latch) = provisional_launch_for(tab, pty_id, binding_nonce, reason) else {
        return false;
    };
    apply_boot_stage(tab, stage, Some(reason));
    tab.provisional_launch = Some(latch);
    true
}

fn apply_late_readiness(tab: &mut Tab, evidence: &crate::service::ProviderSessionBinding) -> bool {
    if !late_readiness_retires_provisional_launch(tab, evidence) {
        return false;
    }
    apply_boot_stage(tab, BootStage::AgentReady, None);
    true
}

fn commit_boot_stage_transition(
    mux: &crate::MuxState,
    tab_id: &str,
    transition: impl FnOnce(&mut Tab) -> bool,
    publish: impl FnOnce(),
) -> bool {
    let mut mux = mux.write();
    let Some(tab) = mux.get_tab_mut(tab_id) else {
        return false;
    };
    if !transition(tab) {
        return false;
    }
    publish();
    true
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
    pub(crate) fn record_startup_failure_reason(
        &self,
        authority: &crate::service::startup_lease::StartupAuthority,
        reason: &str,
    ) -> bool {
        {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(&authority.child_tab_id) else {
                return false;
            };
            if tab.pty_id.as_deref() != Some(authority.attempt_id.as_str())
                || tab.agent_session_id != authority.session_id
                || tab.session_binding_nonce != authority.binding_nonce
                || tab.launch_session_authenticated()
            {
                return false;
            }
            tab.boot_failure_reason = Some(reason.to_string());
        }
        self.emit_state_changed();
        true
    }

    /// Set the tab's current boot stage (for late attachers reading the
    /// snapshot) and fan the transition out on the event bridge. Fire-and-forget
    /// — never blocks the launch thread.
    pub(crate) fn emit_boot_stage(&self, tab_id: &str, stage: BootStage, reason: Option<&str>) {
        if stage.is_provisional() {
            log::warn!(
                "[boot-stage] Tab {tab_id} {} ignored without launch-attempt evidence",
                stage.as_str()
            );
            return;
        }
        let stage = self.stage_reconciled_with_session_binding(tab_id, stage);
        let reason = stage.carries_reason().then_some(reason).flatten();
        if !self.record_boot_stage_transition(tab_id, stage, reason, |tab| {
            apply_boot_stage(tab, stage, reason);
            true
        }) {
            return;
        }
        self.finish_boot_stage_transition(tab_id, stage, reason);
    }

    fn record_boot_stage_transition(
        &self,
        tab_id: &str,
        stage: BootStage,
        reason: Option<&str>,
        transition: impl FnOnce(&mut Tab) -> bool,
    ) -> bool {
        let reason = stage.carries_reason().then_some(reason).flatten();
        commit_boot_stage_transition(&self.mux, tab_id, transition, || {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "tab-boot-stage",
                serde_json::json!({
                    "tab_id": tab_id,
                    "stage": stage,
                    "reason": reason,
                }),
            );
        })
    }

    fn finish_boot_stage_transition(&self, tab_id: &str, stage: BootStage, reason: Option<&str>) {
        super::boot_wait::notify_boot_changed();
        crate::service::general_agent::binding_wait::notify_binding_state_changed();
        self.emit_session_binding_state(tab_id);
        // A boot stage is the only moment a tab's agent liveness can change on
        // the launch side; the restore wave settles here rather than on a tick.
        self.settle_restore_report();
        if stage == BootStage::ShellReady {
            self.resume_exited_member_on_shell_ready(tab_id);
        }
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
    pub(crate) fn record_launch_output_deadline(
        &self,
        attempt: &crate::service::launch_readiness::LaunchAttempt,
        reason: &str,
    ) {
        self.record_provisional_launch(attempt, BootStage::LaunchDeadlineElapsed, reason);
    }

    /// Latch a provisional launch: uncertainty about one PTY and one launch
    /// attempt, carrying the evidence that made this observer give up. Only a
    /// nonce-verified binding for the same pair retires it.
    pub(crate) fn record_provisional_launch(
        &self,
        attempt: &crate::service::launch_readiness::LaunchAttempt,
        stage: BootStage,
        reason: &str,
    ) {
        debug_assert!(stage.is_provisional());
        let recorded =
            self.record_boot_stage_transition(&attempt.tab_id, stage, Some(reason), |tab| {
                apply_provisional_launch(
                    tab,
                    &attempt.pty_id,
                    &attempt.binding_nonce,
                    stage,
                    reason,
                )
            });
        if !recorded {
            log::info!(
                "[boot-stage] Tab {} {} ignored; launch on PTY {} and its binding are no longer current",
                attempt.tab_id,
                stage.as_str(),
                attempt.pty_id,
            );
            return;
        }
        self.finish_boot_stage_transition(&attempt.tab_id, stage, Some(reason));
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
        if !self.record_boot_stage_transition(tab_id, BootStage::AgentReady, None, |tab| {
            apply_late_readiness(tab, evidence)
        }) {
            return false;
        }
        log::info!(
            "[boot-stage] Tab {tab_id} provisional launch retired by late readiness on its own PTY {} and launch attempt",
            evidence.pty_id
        );
        self.finish_boot_stage_transition(tab_id, BootStage::AgentReady, None);
        true
    }

    /// A nonce-verified session-start report is the agent naming itself on its
    /// own launch, which disproves a failure another observer already latched.
    /// Event-driven by the proof's arrival — never a poll or a retry.
    pub(crate) fn reconcile_boot_stage_with_session_binding(&self, tab_id: &str) {
        let contradicted =
            self.record_boot_stage_transition(tab_id, BootStage::AgentReady, None, |tab| {
                if !binding_disproves_latched_failure(tab) {
                    return false;
                }
                apply_boot_stage(tab, BootStage::AgentReady, None);
                true
            });
        if contradicted {
            log::info!(
                "[boot-stage] Tab {tab_id} spawn_failed disproved by its nonce-verified session binding"
            );
            self.finish_boot_stage_transition(tab_id, BootStage::AgentReady, None);
            return;
        }
        let binding = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| binding_observes_in_tab_launch(tab))
                .and_then(|tab| {
                    Some((
                        tab.pty_id.clone()?,
                        tab.agent_session_id.clone()?,
                        tab.reported_session_binding_nonce.clone()?,
                    ))
                })
        };
        let Some((pty_id, session_id, binding_nonce)) = binding else {
            return;
        };
        if self.pty.lock().get_session(&pty_id).is_none() {
            return;
        }
        let recorded =
            self.record_boot_stage_transition(tab_id, BootStage::AgentReady, None, |tab| {
                let same_binding = tab.pty_id.as_deref() == Some(pty_id.as_str())
                    && tab.agent_session_id.as_deref() == Some(session_id.as_str())
                    && tab.reported_session_binding_nonce.as_deref()
                        == Some(binding_nonce.as_str())
                    && tab.launch_session_authenticated();
                if !same_binding || !binding_observes_in_tab_launch(tab) {
                    return false;
                }
                apply_boot_stage(tab, BootStage::AgentReady, None);
                true
            });
        if !recorded {
            return;
        }
        log::info!(
            "[boot-stage] Tab {tab_id} in-tab provider launch observed live by its nonce-verified session binding on PTY {pty_id}"
        );
        self.finish_boot_stage_transition(tab_id, BootStage::AgentReady, None);
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
#[path = "boot_stage/in_tab_launch_tests.rs"]
mod in_tab_launch_tests;

#[cfg(test)]
mod tests;
