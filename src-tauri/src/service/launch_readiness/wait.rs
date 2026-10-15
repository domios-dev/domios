use super::*;

impl ServiceContext {
    /// Own one launch's bounded readiness observation to its end. Runs on its
    /// own thread so no launch blocks on it.
    pub(crate) fn observe_launch_readiness(
        &self,
        attempt: LaunchAttempt,
        deadline: std::time::Duration,
        session_starts_on_first_user_turn: bool,
    ) -> std::sync::mpsc::Receiver<LaunchSettlement> {
        let (settled, receiver) = std::sync::mpsc::sync_channel(1);
        let svc = self.clone();
        std::thread::spawn(move || {
            let normal_deadline = Instant::now() + deadline;
            let mut ready_before_prompt = false;
            let published_settlement = |exit_code| {
                take_published_failure(&attempt)
                    .map(|reason| LaunchSettlement::Failed { reason, exit_code })
                    .unwrap_or(LaunchSettlement::Superseded)
            };
            let evidence = match observe_with_owner_prompt(&attempt, normal_deadline, |question| {
                let stage = svc
                    .mux
                    .read()
                    .get_tab(&attempt.tab_id)
                    .and_then(|tab| tab.boot_stage);
                if let Some(next) =
                    owner_prompt_stage(question.is_some(), stage, &mut ready_before_prompt)
                {
                    svc.emit_boot_stage(&attempt.tab_id, next, None)
                }
            }) {
                LaunchObservation::Evidence(evidence) => evidence,
                LaunchObservation::OwnerDeadline { question } => {
                    let reason = owner_prompt_deadline_reason(&question);
                    let _ = settled.send(LaunchSettlement::OwnerPromptDeadline { reason });
                    return;
                }
                LaunchObservation::SuspectedExit { exit_code, proof } => {
                    svc.handle_proven_provider_exit(
                        &attempt.tab_id,
                        &attempt.pty_id,
                        exit_code,
                        &proof,
                    );
                    let settlement = published_settlement(exit_code);
                    retire_attempt(&attempt);
                    let _ = settled.send(settlement);
                    return;
                }
                LaunchObservation::Deadline
                    if svc.confirm_resumed_launch_session(&attempt.tab_id) =>
                {
                    LaunchEvidence::Acknowledged
                }
                LaunchObservation::Deadline
                    if session_starts_on_first_user_turn
                        && svc.launch_attempt_is_waiting_for_first_turn(&attempt) =>
                {
                    let authenticated = {
                        let mux = svc.mux.read();
                        mux.get_tab(&attempt.tab_id)
                            .is_some_and(|tab| tab.launch_session_authenticated())
                    };
                    if authenticated {
                        LaunchEvidence::Acknowledged
                    } else {
                        log::info!(
                            "[launch-readiness] Tab {} awaits first turn on live PTY {}",
                            attempt.tab_id,
                            attempt.pty_id
                        );
                        let _ = settled.send(LaunchSettlement::AwaitingFirstTurn);
                        return;
                    }
                }
                LaunchObservation::Deadline => LaunchEvidence::Unobserved {
                    stage: BootStage::LaunchDeadlineElapsed,
                    reason: SPAWN_READINESS_DEADLINE.to_string(),
                },
                LaunchObservation::Retired => {
                    let _ = settled.send(published_settlement(None));
                    return;
                }
            };
            let hook_missed_deadline = matches!(evidence, LaunchEvidence::Unobserved { .. });
            let settlement = svc.settle_launch_readiness(&attempt, evidence);
            if hook_missed_deadline && settlement != LaunchSettlement::Superseded {
                svc.detect_unbound_launch_session(&attempt.tab_id);
            }
            let _ = settled.send(settlement);
        });
        receiver
    }

    fn launch_attempt_is_waiting_for_first_turn(&self, attempt: &LaunchAttempt) -> bool {
        let awaiting_first_turn = {
            let mux = self.mux.read();
            mux.get_tab(&attempt.tab_id)
                .is_some_and(|tab| launch_attempt_is_waiting_for_first_turn(tab, attempt))
        };
        awaiting_first_turn && self.pty.lock().get_session(&attempt.pty_id).is_some()
    }
}
