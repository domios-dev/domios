use super::model::{PendingReplyRelay, WorkerTaskBinding};
use super::runtime::RelayBinding;
use super::worker::{WorkerTurnReport, WorkerTurnState};
use crate::agent::reports::ReportDetails;
use crate::service::chat_send_evidence::{contains_exact_user_turn_since, ExactTurnCursor};
use crate::service::chat_send_transaction::{EvidenceWindow, SendEvidenceAnchor};
use crate::service::ServiceContext;
use parking_lot::Mutex;

impl ServiceContext {
    pub(super) fn reply_relay_task_binding(&self, target: &str) -> Option<WorkerTaskBinding> {
        let mut bindings = self
            .plan_store
            .live_task_attempts_for_tab(target)
            .into_iter()
            .filter(|binding| binding.plan_active && binding.attempt.retired_at.is_none());
        let binding = bindings.next()?;
        if bindings.next().is_some() {
            return None;
        }
        Some(WorkerTaskBinding {
            plan_id: binding.plan_id,
            task_id: binding.task_id,
            attempt_id: binding.attempt.id,
            owner_epoch: binding.attempt.interruption.owner_epoch,
        })
    }

    pub(super) fn observe_tool_less_worker_ack(
        &self,
        target: &str,
        binding: &RelayBinding,
        pending: &PendingReplyRelay,
        state: &Mutex<WorkerTurnState>,
    ) {
        let needs_ack = { state.lock().needs_ack(pending, binding.source(), true) };
        if !needs_ack
            || self.current_reply_relay_binding(target).as_ref() != Some(binding)
            || self.reply_relay_task_binding(target).as_ref() != pending.task.as_ref()
        {
            return;
        }
        let anchor = SendEvidenceAnchor {
            provider_id: binding.provider.clone(),
            cwd: binding.cwd.clone(),
            session_id: binding.session.clone(),
            window: EvidenceWindow::Jsonl {
                transcript_offset: pending.after_offset,
            },
            turn_normalization: pending.turn_normalization,
        };
        let mut cursor = ExactTurnCursor::for_anchor(Some(&anchor));
        if !contains_exact_user_turn_since(
            &self.agent_registry,
            &anchor,
            &pending.user_text,
            &mut cursor,
        ) || self.current_reply_relay_binding(target).as_ref() != Some(binding)
            || self.reply_relay_task_binding(target).as_ref() != pending.task.as_ref()
        {
            return;
        }
        self.persist_tool_less_worker_ack(target, binding, pending, state);
    }

    pub(super) fn observe_tool_less_worker_answer_ack(
        &self,
        target: &str,
        binding: &RelayBinding,
        pending: &PendingReplyRelay,
        state: &Mutex<WorkerTurnState>,
        evidence: Option<&crate::agent::provider::ReplyRelayEvidence>,
    ) {
        if super::worker::completed_brief_ack_eligible(pending, binding.source(), evidence) {
            self.persist_tool_less_worker_ack(target, binding, pending, state);
        }
    }

    fn persist_tool_less_worker_ack(
        &self,
        target: &str,
        binding: &RelayBinding,
        pending: &PendingReplyRelay,
        state: &Mutex<WorkerTurnState>,
    ) {
        if !state.lock().needs_ack(pending, binding.source(), true)
            || self.current_reply_relay_binding(target).as_ref() != Some(binding)
            || self.reply_relay_task_binding(target).as_ref() != pending.task.as_ref()
        {
            return;
        }
        let Some(task) = pending.task.as_ref() else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let title = self.mux.read().get_tab(target).map(|tab| tab.title.clone());
        let result = (|| {
            self.plan_store
                .update_attempt_executor_identity(
                    &task.plan_id,
                    &task.task_id,
                    target,
                    title.as_deref(),
                    Some(&binding.session),
                    Some(&binding.nonce),
                    now,
                )
                .map_err(|error| error.to_string())?;
            let acknowledgement = self
                .plan_store
                .acknowledge_task_attempt(
                    &task.plan_id,
                    &task.task_id,
                    target,
                    &task.attempt_id,
                    now,
                )
                .map_err(|error| error.to_string())?;
            self.acknowledge_worker_ack(
                &task.plan_id,
                &task.task_id,
                target,
                &acknowledgement.attempt,
            );
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => {
                state.lock().mark_acknowledged(task.clone());
                self.emit_state_changed();
            }
            Err(error) => {
                log::warn!("[tool-less-worker] target={target} exact-turn ACK refused: {error}")
            }
        }
    }

    pub(super) fn observe_tool_less_startup_ack(&self, target: &str, binding: &RelayBinding) {
        if self.reply_relay_task_binding(target).is_some()
            || !self.worker_ack.startup_handshake_owed(target)
        {
            return;
        }
        let first_user = self
            .agent_registry
            .with_provider(&binding.provider, |provider| {
                if !super::worker::host_startup_ack_eligible(
                    provider.is_tool_less(),
                    provider.supports_reply_relay(),
                    provider.session_starts_on_first_user_turn(),
                    true,
                ) {
                    return false;
                }
                provider
                    .parse_session_delta(&binding.history, 0)
                    .messages
                    .iter()
                    .any(|message| {
                        message.role == "user"
                            && message.blocks.iter().any(|block| {
                                block.kind == "text"
                                    && block
                                        .data
                                        .get("text")
                                        .and_then(|value| value.as_str())
                                        .is_some_and(|text| !text.trim().is_empty())
                            })
                    })
            })
            .unwrap_or(false);
        let attempt = self
            .mux
            .read()
            .get_tab(target)
            .and_then(|tab| tab.pty_id.clone());
        let Some(attempt) = attempt else { return };
        if first_user && self.current_reply_relay_binding(target).as_ref() == Some(binding) {
            if let Err(error) = self.acknowledge_agent_startup(target, &attempt, &binding.nonce) {
                log::warn!("[tool-less-worker] target={target} host startup ACK refused: {error}");
            }
        }
    }

    pub(super) fn publish_tool_less_worker_report(
        &self,
        target: &str,
        binding: &RelayBinding,
        pending: &PendingReplyRelay,
        report: WorkerTurnReport,
    ) {
        let Some(task) = pending.task.as_ref() else {
            return;
        };
        if self.current_reply_relay_binding(target).as_ref() != Some(binding)
            || self.reply_relay_task_binding(target).as_ref() != Some(task)
        {
            return;
        }
        let details = ReportDetails {
            plan_id: Some(task.plan_id.clone()),
            task_id: Some(task.task_id.clone()),
            owner_epoch: Some(task.owner_epoch),
            attempt_id: Some(task.attempt_id.clone()),
            caller_tab_id: Some(target.into()),
            caller_session_id: Some(binding.session.clone()),
            session_binding_nonce: Some(binding.nonce.clone()),
            status: Some(report.status),
            ..Default::default()
        };
        let kind = if report.incident {
            "coordination_incident"
        } else {
            "worker_report"
        };
        if let Err(error) = self.submit_report(
            target,
            &report.body,
            details,
            Some(&task.plan_id),
            Some(&task.task_id),
            Some(kind),
        ) {
            log::error!(
                "[tool-less-worker] target={target} task={}/{} report refused: {error}",
                task.plan_id,
                task.task_id
            );
            let _ = self.send_refused_report_diagnostic(
                target,
                &format!("Tool-less Worker host report failed: {error}"),
            );
        }
    }
}
