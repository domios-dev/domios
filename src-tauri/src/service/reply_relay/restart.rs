use super::model::PendingReplyRelay;
use crate::multiplexer::types::{RestorableWorkerTurn, WorkerRestartState};
use crate::service::ServiceContext;

#[derive(Debug, PartialEq, Eq)]
enum BriefRestoreDecision {
    Observe,
    Submit,
}

fn brief_restore_decision(
    restores_history: bool,
    exact_current_turn: bool,
    already_claimed: bool,
) -> BriefRestoreDecision {
    if restores_history || exact_current_turn || already_claimed {
        BriefRestoreDecision::Observe
    } else {
        BriefRestoreDecision::Submit
    }
}

fn restore_frame_and_payload(
    original: &str,
    typed_instruction: bool,
) -> Option<(codeterm_types::domios::DomiosFrame, String)> {
    use codeterm_types::domios::{DomiosFrame, DOMIOS_CLOSE_TAG, DOMIOS_TYPED_INSTRUCTION};
    let mut body = original.strip_suffix(DOMIOS_CLOSE_TAG)?.to_string();
    if typed_instruction {
        body = body
            .strip_suffix(&format!(" {DOMIOS_TYPED_INSTRUCTION}"))
            .unwrap_or(&body)
            .to_string();
    }
    let payload = format!(
        "{body}{}{DOMIOS_CLOSE_TAG}",
        if typed_instruction {
            format!(" {DOMIOS_TYPED_INSTRUCTION}")
        } else {
            String::new()
        }
    );
    Some((
        DomiosFrame {
            opening_tag: String::new(),
            body,
            suffix: String::new(),
        },
        payload,
    ))
}

fn persist_restart_state(service: &ServiceContext) -> Result<(), String> {
    match crate::service::session_persist::write_session_snapshot(
        &service.app_handle,
        &service.mux,
    )? {
        crate::session::store::SaveOutcome::Saved => Ok(()),
        crate::session::store::SaveOutcome::RefusedReduction(_) => {
            Err("Worker restart state persistence refused".into())
        }
    }
}

pub(super) fn completed_pending_turn(
    pending: &PendingReplyRelay,
    evidence: &crate::agent::provider::ReplyRelayEvidence,
) -> bool {
    if !evidence.is_valid() {
        return false;
    }
    match evidence {
        crate::agent::provider::ReplyRelayEvidence::Answered(turn) => {
            super::model::matches_user_turn(
                pending,
                &turn.user_turn_id,
                turn.user_record_start,
                &turn.user_text,
            )
        }
        crate::agent::provider::ReplyRelayEvidence::Ended(turn) => super::model::matches_user_turn(
            pending,
            &turn.user_turn_id,
            turn.user_record_start,
            &turn.user_text,
        ),
        crate::agent::provider::ReplyRelayEvidence::Unavailable(_) => true,
    }
}

impl From<&PendingReplyRelay> for RestorableWorkerTurn {
    fn from(pending: &PendingReplyRelay) -> Self {
        Self {
            delivery_id: pending.delivery_id.clone(),
            sender_tab: pending.sender_tab.clone(),
            provider_id: pending.provider_id.clone(),
            session_id: pending.session_id.clone(),
            binding_nonce: pending.binding_nonce.clone(),
            after_offset: pending.after_offset,
            user_text: pending.user_text.clone(),
            hops: pending.hops,
        }
    }
}

fn consume_pending_turn(
    state: &mut WorkerRestartState,
    delivery: &str,
) -> Vec<RestorableWorkerTurn> {
    let removed = state
        .pending
        .iter()
        .filter(|turn| turn.delivery_id == delivery)
        .cloned()
        .collect();
    state.pending.retain(|turn| turn.delivery_id != delivery);
    removed
}

impl ServiceContext {
    pub(super) fn remember_worker_turn(
        &self,
        target: &str,
        pending: &PendingReplyRelay,
    ) -> Result<(), String> {
        let Some(task) = pending.task.as_ref() else {
            return Ok(());
        };
        {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(target).ok_or("Worker closed")?;
            let slot = &mut tab.restorable_launch.worker_restart;
            if slot
                .as_ref()
                .is_none_or(|state| state.attempt_id != task.attempt_id)
            {
                *slot = Some(WorkerRestartState {
                    attempt_id: task.attempt_id.clone(),
                    original_brief: pending.user_text.clone(),
                    sender_tab: pending.sender_tab.clone(),
                    ..Default::default()
                });
            }
            let state = slot.as_mut().unwrap();
            if state
                .pending
                .iter()
                .map(|turn| turn.user_text.len())
                .sum::<usize>()
                + pending.user_text.len()
                > 4 * 1024 * 1024
            {
                return Err("Worker restart payload capacity reached".into());
            }
            if let Some(existing) = state
                .pending
                .iter_mut()
                .find(|turn| turn.delivery_id == pending.delivery_id)
            {
                *existing = pending.into();
            } else {
                if state.pending.len() >= super::model::MAX_TRACKED_RELAY_DELIVERIES {
                    return Err("Worker restart evidence capacity reached".into());
                }
                state.pending.push(pending.into());
            }
        }
        persist_restart_state(self)?;
        Ok(())
    }

    pub(crate) fn settle_restored_worker_turn(
        &self,
        target: &str,
        delivery: &str,
    ) -> Result<(), String> {
        let removed = {
            let mut mux = self.mux.write();
            let Some(state) = mux
                .get_tab_mut(target)
                .and_then(|tab| tab.restorable_launch.worker_restart.as_mut())
            else {
                return Ok(());
            };
            consume_pending_turn(state, delivery)
        };
        if !removed.is_empty() {
            if let Err(error) = persist_restart_state(self) {
                if let Some(state) = self
                    .mux
                    .write()
                    .get_tab_mut(target)
                    .and_then(|tab| tab.restorable_launch.worker_restart.as_mut())
                {
                    state.pending.extend(removed);
                }
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn retain_spawn_worker_brief(&self, target: &str, sender: &str, brief: &str) {
        let Some(task) = self.reply_relay_task_binding(target) else {
            return;
        };
        {
            if let Some(tab) = self.mux.write().get_tab_mut(target) {
                if tab.restorable_launch.worker_restart.is_none() {
                    tab.restorable_launch.worker_restart = Some(WorkerRestartState {
                        attempt_id: task.attempt_id,
                        original_brief: brief.into(),
                        sender_tab: sender.into(),
                        ..Default::default()
                    });
                }
            }
        }
        if let Err(error) = persist_restart_state(self) {
            log::error!(
                "[tool-less-restart] target={target} original brief persistence failed: {error}"
            );
        }
    }

    pub(super) fn stage_worker_restart(&self, target: &str) {
        let own = {
            let mux = self.mux.read();
            mux.get_tab(target).and_then(|tab| {
                tab.restorable_launch.worker_restart.as_ref()?;
                Some((
                    tab.agent_type.clone()?,
                    tab.cwd.clone()?,
                    tab.agent_session_id.clone().unwrap_or_default(),
                ))
            })
        };
        let Some((provider, cwd, session)) = own else {
            return;
        };
        let boundary = self
            .agent_registry
            .with_provider(&provider, |provider| {
                if !provider.is_tool_less() || !provider.supports_reply_relay() {
                    return None;
                }
                if session.is_empty() {
                    return Some(0);
                }
                let path = provider.session_file_path(&cwd, &session)?;
                match std::fs::metadata(path) {
                    Ok(metadata) => Some(metadata.len()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(0),
                    Err(error) => {
                        log::error!("[tool-less-restart] history boundary unavailable: {error}");
                        None
                    }
                }
            })
            .flatten();
        if let Some(boundary) = boundary {
            self.reply_relays
                .restored
                .lock()
                .insert(target.into(), (session, boundary));
        }
    }

    pub(super) fn restore_worker_after_binding(&self, target: &str) {
        let Some(boundary) = self.reply_relays.restored.lock().remove(target) else {
            return;
        };
        if let Err(error) = self.restore_worker_turns(target, boundary) {
            log::error!("[tool-less-restart] target={target} restore refused: {error}");
        }
    }

    fn restore_worker_turns(
        &self,
        target: &str,
        (old_session, boundary): (String, u64),
    ) -> Result<(), String> {
        let binding = self
            .current_reply_relay_binding(target)
            .ok_or("restored Worker binding unavailable")?;
        let task = self
            .reply_relay_task_binding(target)
            .ok_or("restored Worker attempt unavailable")?;
        let state = self
            .mux
            .read()
            .get_tab(target)
            .and_then(|tab| tab.restorable_launch.worker_restart.clone())
            .ok_or("restored Worker brief unavailable")?;
        if state.attempt_id != task.attempt_id {
            return Err("restored Worker attempt changed".into());
        }
        let (restores, normalization, typed_instruction) = self
            .agent_registry
            .with_provider(&binding.provider, |provider| {
                (
                    provider.restores_conversation_history_on_resume()
                        && old_session == binding.session,
                    provider.turn_evidence_normalization(),
                    provider.paste_capability().is_some_and(|capability| {
                        capability.requires_typed_instruction_for_routed_messages
                    }),
                )
            })
            .ok_or("restored Worker provider unavailable")?;
        for turn in state.pending {
            if turn.provider_id != binding.provider || turn.session_id != binding.session {
                continue;
            }
            self.record_bound_reply_relay(
                target,
                PendingReplyRelay {
                    sender_tab: turn.sender_tab,
                    delivery_id: turn.delivery_id,
                    provider_id: binding.provider.clone(),
                    session_id: binding.session.clone(),
                    binding_nonce: binding.nonce.clone(),
                    after_offset: turn.after_offset,
                    user_text: turn.user_text,
                    turn_normalization: normalization,
                    hops: turn.hops,
                    task: Some(task.clone()),
                },
            )?;
        }
        if restores {
            return Ok(());
        }
        let after_offset = if old_session == binding.session {
            boundary
        } else {
            0
        };
        let (frame, restored_payload) =
            restore_frame_and_payload(&state.original_brief, typed_instruction)
                .ok_or("original Worker brief has no routed closing frame")?;
        let anchor = crate::service::chat_send_transaction::SendEvidenceAnchor {
            provider_id: binding.provider.clone(),
            cwd: binding.cwd.clone(),
            session_id: binding.session.clone(),
            window: crate::service::chat_send_transaction::EvidenceWindow::Jsonl {
                transcript_offset: after_offset,
            },
            turn_normalization: normalization,
        };
        let exact = crate::service::chat_send_evidence::contains_exact_user_turn_since(
            &self.agent_registry,
            &anchor,
            &restored_payload,
            &mut crate::service::chat_send_evidence::ExactTurnCursor::for_anchor(Some(&anchor)),
        );
        let decision = brief_restore_decision(
            false,
            exact,
            state.claimed_nonce.as_deref() == Some(&binding.nonce),
        );
        let pending = PendingReplyRelay {
            sender_tab: state.sender_tab,
            delivery_id: format!("worker-resume:{}:{}", task.attempt_id, binding.nonce),
            provider_id: binding.provider.clone(),
            session_id: binding.session.clone(),
            binding_nonce: binding.nonce.clone(),
            after_offset,
            user_text: restored_payload,
            turn_normalization: normalization,
            hops: 0,
            task: Some(task.clone()),
        };
        {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(target).ok_or("restored Worker closed")?;
            if !tab.launch_session_authenticated()
                || tab.session_binding_nonce.as_deref() != Some(&binding.nonce)
            {
                return Err("restored binding changed before brief admission".into());
            }
            let state = tab
                .restorable_launch
                .worker_restart
                .as_mut()
                .ok_or("restored brief removed")?;
            if state.attempt_id != task.attempt_id {
                return Err("restored attempt changed".into());
            }
            state.claimed_nonce = Some(binding.nonce.clone());
        }
        self.record_bound_reply_relay(target, pending.clone())?;
        if decision == BriefRestoreDecision::Submit {
            if self.current_reply_relay_binding(target).as_ref() != Some(&binding)
                || self.reply_relay_task_binding(target).as_ref() != Some(&task)
            {
                return Err("restored attempt changed before brief write".into());
            }
            let mut cursor =
                crate::service::chat_send_evidence::ExactTurnCursor::for_anchor(Some(&anchor));
            if crate::service::chat_send_evidence::contains_exact_user_turn_since(
                &self.agent_registry,
                &anchor,
                &pending.user_text,
                &mut cursor,
            ) {
                self.reply_relays.wake(target);
                return Ok(());
            }
            let pty = self
                .mux
                .read()
                .get_tab(target)
                .and_then(|tab| tab.pty_id.clone())
                .ok_or("restored Worker has no PTY")?;
            self.send_to_authenticated_agent_session_pinned_routed(
                target,
                &pty,
                &binding.session,
                &binding.nonce,
                frame,
            )?;
        }
        self.reply_relays.wake(target);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restoring_provider_observes_without_redelivery() {
        assert_eq!(
            brief_restore_decision(true, false, false),
            BriefRestoreDecision::Observe
        );
    }
    #[test]
    fn non_restoring_provider_claims_exactly_one_brief() {
        assert_eq!(
            brief_restore_decision(false, false, false),
            BriefRestoreDecision::Submit
        );
        assert_eq!(
            brief_restore_decision(false, false, true),
            BriefRestoreDecision::Observe
        );
    }
    #[test]
    fn current_session_delivery_is_never_replayed() {
        assert_eq!(
            brief_restore_decision(false, true, false),
            BriefRestoreDecision::Observe
        );
    }
    #[test]
    fn snapshot_keeps_original_brief_and_only_unconsumed_turn_windows() {
        let turn = RestorableWorkerTurn {
            delivery_id: "first".into(),
            sender_tab: "dispatcher".into(),
            provider_id: "fixture".into(),
            session_id: "own".into(),
            binding_nonce: "old-launch".into(),
            after_offset: 100,
            user_text: "opaque initial brief".into(),
            hops: 0,
        };
        let mut followup = turn.clone();
        followup.delivery_id = "followup".into();
        followup.after_offset = 700;
        followup.user_text = "opaque followup".into();
        let mut state = WorkerRestartState {
            attempt_id: "same-attempt".into(),
            original_brief: turn.user_text.clone(),
            sender_tab: turn.sender_tab.clone(),
            pending: vec![turn, followup.clone()],
            claimed_nonce: Some("old-launch".into()),
        };
        assert_eq!(consume_pending_turn(&mut state, "first").len(), 1);
        let restored: WorkerRestartState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored.original_brief, "opaque initial brief");
        assert_eq!(restored.pending, [followup]);
        assert_eq!(restored.attempt_id, "same-attempt");
        assert!(consume_pending_turn(&mut state, "first").is_empty());
    }
    #[test]
    fn restored_routed_frame_preserves_brief_and_declared_typed_suffix() {
        let original = "<domios from=\"fixture\">opaque\nwhole brief</domios>";
        let (frame, payload) = restore_frame_and_payload(original, false).unwrap();
        assert_eq!(frame.to_message(), original);
        assert_eq!(payload, original);
        let (typed_frame, typed_payload) = restore_frame_and_payload(original, true).unwrap();
        assert_eq!(typed_frame.body, frame.body);
        assert!(typed_payload.starts_with(&frame.body));
        let (same_frame, same_payload) = restore_frame_and_payload(&typed_payload, true).unwrap();
        assert_eq!(same_frame, typed_frame);
        assert_eq!(same_payload, typed_payload);
        assert!(restore_frame_and_payload("unframed", false).is_none());
    }
}
