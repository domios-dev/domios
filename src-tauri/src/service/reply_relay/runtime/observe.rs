use super::*;
use crate::service::message_gateway::DeliveryClass;

impl ServiceContext {
    pub(super) fn observe_reply_relay(&self, target: &str, entry: &Arc<ActiveRelay>) {
        if self.current_reply_relay_binding(target).as_ref() != Some(&entry.binding) {
            return;
        }
        self.observe_tool_less_startup_ack(target, &entry.binding);
        let max_hops = crate::config::load_config().agent.reply_relay_max_hops;
        let mut pending = entry.worker.lock().pending();
        for relay in entry.model.lock().pending() {
            if !pending
                .iter()
                .any(|pending| pending.delivery_id == relay.delivery_id)
            {
                pending.push(relay);
            }
        }
        for pending in pending {
            self.observe_tool_less_worker_ack(target, &entry.binding, &pending, &entry.worker);
            let after_offset = entry
                .model
                .lock()
                .read_offset(&pending)
                .max(entry.worker.lock().read_offset(&pending));
            let evidence = self
                .agent_registry
                .with_provider(&entry.binding.provider, |provider| {
                    provider.read_reply_relay_turn(
                        &entry.binding.cwd,
                        &entry.binding.session,
                        after_offset,
                    )
                })
                .flatten();
            if self.current_reply_relay_binding(target).as_ref() != Some(&entry.binding) {
                return;
            }
            if let Some(evidence) = evidence.as_ref() {
                crate::flow_trace::record_flow(
                    &pending.delivery_id,
                    "relay.observed",
                    target,
                    &entry.binding.provider,
                    match evidence {
                        crate::agent::provider::ReplyRelayEvidence::Answered(_) => "answered",
                        crate::agent::provider::ReplyRelayEvidence::Ended(_) => "ended",
                        crate::agent::provider::ReplyRelayEvidence::Unavailable(_) => "unavailable",
                    },
                    pending.task.as_ref().map(|task| task.attempt_id.as_str()),
                );
                log::debug!("[reply-relay] target={target} observed delivery={} after_offset={} evidence={}", pending.delivery_id, after_offset, match evidence {
                    crate::agent::provider::ReplyRelayEvidence::Answered(_) => "answered",
                    crate::agent::provider::ReplyRelayEvidence::Ended(_) => "ended",
                    crate::agent::provider::ReplyRelayEvidence::Unavailable(_) => "unavailable",
                });
            }
            self.observe_tool_less_worker_answer_ack(
                target,
                &entry.binding,
                &pending,
                &entry.worker,
                evidence.as_ref(),
            );
            let advanced = entry.model.lock().advance_past_unrelated_turn(
                &pending,
                entry.binding.source(),
                evidence.as_ref(),
            );
            if advanced {
                let _ = entry.signal.try_send(());
                continue;
            }
            let (evidence, report_status) = super::super::report_marker::prepare_completed_reply(
                &pending,
                entry.binding.source(),
                evidence.as_ref(),
            );
            if evidence.as_ref().is_some_and(|evidence| {
                super::super::restart::completed_pending_turn(&pending, evidence)
            }) {
                if let Err(error) = self.settle_restored_worker_turn(target, &pending.delivery_id) {
                    log::error!(
                        "[tool-less-restart] target={target} completion claim refused: {error}"
                    );
                    continue;
                }
            }
            let report = {
                entry.worker.lock().observe_prepared_completion(
                    &pending,
                    entry.binding.source(),
                    evidence.as_ref(),
                    report_status,
                )
            };
            if let Some(report) = report {
                crate::flow_trace::record_flow(
                    &pending.delivery_id,
                    "report.parsed",
                    target,
                    &entry.binding.provider,
                    "accepted",
                    pending.task.as_ref().map(|task| task.attempt_id.as_str()),
                );
                self.publish_tool_less_worker_report(target, &entry.binding, &pending, report);
            }
            let open_senders = {
                let mux = self.mux.read();
                entry
                    .model
                    .lock()
                    .pending()
                    .iter()
                    .filter(|pending| mux.get_tab(&pending.sender_tab).is_some())
                    .map(|pending| pending.sender_tab.clone())
                    .collect::<std::collections::HashSet<_>>()
            };
            let actions = entry.model.lock().observe_delivery_evidence(
                &pending.delivery_id,
                entry.binding.source(),
                evidence.as_ref(),
                |sender| open_senders.contains(sender),
                max_hops,
            );
            for (pending, action) in actions {
                self.send_reply_relay_action(target, &entry.binding, &pending, action);
            }
        }
    }

    pub(super) fn send_reply_relay_action(
        &self,
        target: &str,
        binding: &RelayBinding,
        pending: &PendingReplyRelay,
        action: ReplyRelayDecision,
    ) {
        if !matches!(&action, ReplyRelayDecision::Wait) {
            if let Err(error) = self.settle_restored_worker_turn(target, &pending.delivery_id) {
                log::error!(
                    "[tool-less-restart] target={target} completion persistence failed: {error}"
                );
                return;
            }
        }
        match action {
            ReplyRelayDecision::Wait => return,
            ReplyRelayDecision::DropClosedSender => {
                log::info!(
                    "[reply-relay] target={target} sender={} closed; dropped delivery={}",
                    pending.sender_tab,
                    pending.delivery_id
                );
                return;
            }
            ReplyRelayDecision::DropHopLimit => {
                log::info!(
                    "[reply-relay] target={target} hop limit reached delivery={}",
                    pending.delivery_id
                );
                return;
            }
            _ => {}
        }
        if self.current_reply_relay_binding(target).as_ref() != Some(binding) {
            return;
        }
        let (label, model) = {
            let mux = self.mux.read();
            if mux.get_tab(&pending.sender_tab).is_none() {
                log::info!(
                    "[reply-relay] sender={} closed before routed send; dropped delivery={}",
                    pending.sender_tab,
                    pending.delivery_id
                );
                return;
            }
            let Some(tab) = mux.get_tab(target) else {
                return;
            };
            (
                mux.display_label_for_tab(target),
                tab.agent_session_model.clone(),
            )
        };
        let (text, hops, notice) = match action {
            ReplyRelayDecision::Send { answer, hops, .. } => (answer, hops, false),
            ReplyRelayDecision::Notice { text, .. } => (
                format!("{label} {text}"),
                pending.hops.saturating_add(1),
                true,
            ),
            _ => return,
        };
        let mut envelope = MessageEnvelope::tab(label, target.to_string())
            .model(model)
            .from_agent(Some(true));
        envelope.id = Some(format!("reply-relay:{}", pending.delivery_id));
        if crate::paths::is_dev_instance() {
            envelope.trace_id = Some(
                pending
                    .task
                    .as_ref()
                    .map(|task| task.attempt_id.clone())
                    .unwrap_or_else(|| pending.delivery_id.clone()),
            );
        }
        crate::flow_trace::record_flow(
            &pending.delivery_id,
            "relay.reply",
            &pending.sender_tab,
            &binding.provider,
            "sending",
            pending.task.as_ref().map(|task| task.attempt_id.as_str()),
        );
        crate::flow_trace::record(
            pending
                .task
                .as_ref()
                .map(|task| task.attempt_id.as_str())
                .unwrap_or(&pending.delivery_id),
            "relay.link",
            target,
            &binding.provider,
            "sending",
            envelope.id.as_deref(),
        );
        envelope.relay_hops = hops;
        envelope.relay_notice = notice;
        if let Err(error) = self.gateway_deliver(
            &pending.sender_tab,
            envelope,
            &text,
            DeliveryClass::AgentTraffic,
        ) {
            log::warn!(
                "[reply-relay] target={target} sender={} routed send failed: {error}",
                pending.sender_tab
            );
        }
    }
}
