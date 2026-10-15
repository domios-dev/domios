use super::model::{
    PendingReplyRelay, RelaySource, ReplyRelayDecision, ReplyRelayModel,
    MAX_TRACKED_RELAY_DELIVERIES,
};
use super::watcher;
use crate::service::messenger::{MessageEnvelope, MessageKind};
use crate::service::ServiceContext;
use notify::RecommendedWatcher;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc::SyncSender, Arc};

mod observe;
mod spawn;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RelayBinding {
    pub provider: String,
    pub session: String,
    pub nonce: String,
    pub cwd: String,
    pub history: PathBuf,
}

impl RelayBinding {
    pub fn source(&self) -> RelaySource<'_> {
        RelaySource {
            provider_id: &self.provider,
            session_id: &self.session,
            binding_nonce: &self.nonce,
        }
    }
}

pub(super) struct ActiveRelay {
    binding: RelayBinding,
    model: Mutex<ReplyRelayModel>,
    worker: Mutex<super::worker::WorkerTurnState>,
    signal: SyncSender<()>,
    _watcher: Mutex<Option<RecommendedWatcher>>,
}

#[derive(Default)]
pub(crate) struct ReplyRelayRegistry {
    entries: Mutex<HashMap<String, Arc<ActiveRelay>>>,
    staged: Mutex<HashMap<String, Vec<PendingReplyRelay>>>,
    pub(super) restored: Mutex<HashMap<String, (String, u64)>>,
}

impl ReplyRelayRegistry {
    fn ensure(
        &self,
        service: &ServiceContext,
        tab: &str,
        binding: RelayBinding,
    ) -> Result<Arc<ActiveRelay>, String> {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get(tab).filter(|entry| entry.binding == binding) {
            return Ok(entry.clone());
        }
        let (watcher, signal, receiver) = watcher::watch(&binding.history)?;
        let entry = Arc::new(ActiveRelay {
            binding,
            model: Mutex::new(Default::default()),
            worker: Mutex::new(Default::default()),
            signal,
            _watcher: Mutex::new(Some(watcher)),
        });
        entries.insert(tab.into(), entry.clone());
        let weak = Arc::downgrade(&entry);
        let service = service.clone();
        let tab = tab.to_string();
        let registry_tab = tab.clone();
        let spawned = std::thread::Builder::new()
            .name("reply-relay".into())
            .spawn(move || {
                for () in receiver {
                    let Some(entry) = weak.upgrade() else { break };
                    service.observe_reply_relay(&tab, &entry);
                    if entry.model.lock().unavailable_reason().is_some() {
                        entry._watcher.lock().take();
                        break;
                    }
                }
            });
        if let Err(error) = spawned {
            entries.remove(&registry_tab);
            return Err(format!("reply relay worker: {error}"));
        }
        Ok(entry)
    }

    pub(crate) fn wake(&self, tab: &str) {
        if let Some(entry) = self.entries.lock().get(tab) {
            let _ = entry.signal.try_send(());
        }
    }

    pub(crate) fn close(&self, tab: &str) {
        self.entries.lock().remove(tab);
        self.staged.lock().remove(tab);
        self.restored.lock().remove(tab);
        for entry in self.entries.lock().values() {
            let _ = entry.signal.try_send(());
        }
    }

    pub(crate) fn cancel(&self, tab: &str, delivery_id: &str) {
        if let Some(entry) = self.entries.lock().get(tab) {
            entry.model.lock().cancel(delivery_id);
            entry.worker.lock().cancel(delivery_id);
        }
        if let Some(pending) = self.staged.lock().get_mut(tab) {
            pending.retain(|pending| pending.delivery_id != delivery_id);
        }
    }
}

impl ServiceContext {
    pub(crate) fn prepare_reply_relay(
        &self,
        target: &str,
        envelope: &MessageEnvelope,
        text: &str,
        has_reply_hint: bool,
    ) -> Option<String> {
        let (target, provider, nonce, sender) = {
            let mux = self.mux.read();
            let target = mux.resolve_tab_id(target)?;
            let tab = mux.get_tab(&target)?;
            let source = mux.get_tab(&envelope.from_tab)?;
            if !super::model::reply_relay_eligible(
                true,
                envelope.kind == MessageKind::Message && !envelope.relay_notice,
                source.agent_type.is_some() || source.agent_booting,
                has_reply_hint,
                envelope.from_mesh.is_some()
                    || envelope.peer.is_some()
                    || envelope.from_plugin.is_some(),
            ) {
                return None;
            }
            (
                target.clone(),
                tab.agent_type.clone()?,
                tab.session_binding_nonce.clone()?,
                source.id.clone(),
            )
        };
        let (tool_less, supported) = self
            .agent_registry
            .with_provider(&provider, |provider| {
                (provider.is_tool_less(), provider.supports_reply_relay())
            })
            .unwrap_or_default();
        super::model::reply_relay_target(super::model::ReplyRelayRoute {
            sender: &sender,
            target: &target,
            tool_less,
            supported,
            sender_is_agent: true,
            has_reply_hint,
            remote: false,
            notice: envelope.relay_notice,
            spawn_brief: false,
        })?;
        if let Some(id) = envelope
            .id
            .as_deref()
            .filter(|_| crate::paths::is_dev_instance())
        {
            crate::flow_trace::record_flow(
                id,
                "relay.expected",
                &target,
                &provider,
                "pending",
                self.reply_relay_task_binding(&target)
                    .as_ref()
                    .map(|task| task.attempt_id.as_str()),
            );
        }
        let capture = self.chat_send_anchor(&target);
        let own_binding = self.current_reply_relay_binding(&target);
        let (session_id, after_offset) = if let Some(binding) = own_binding {
            if binding.provider != provider || binding.nonce != nonce {
                return None;
            }
            (
                binding.session,
                std::fs::metadata(&binding.history).ok()?.len(),
            )
        } else {
            if !capture.session_starts_on_first_user_turn
                || capture
                    .live_binding
                    .as_ref()
                    .is_some_and(|binding| binding.session_id.is_some())
            {
                return None;
            }
            (String::new(), 0)
        };
        let pending = PendingReplyRelay {
            sender_tab: sender,
            delivery_id: envelope
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            provider_id: provider,
            session_id,
            binding_nonce: nonce,
            after_offset,
            user_text: text.to_string(),
            turn_normalization: capture.turn_normalization,
            hops: envelope.relay_hops,
            task: self.reply_relay_task_binding(&target),
        };
        let id = pending.delivery_id.clone();
        if pending.session_id.is_empty() {
            let mut staged = self
                .ext::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
                .staged
                .lock();
            let staged = staged.entry(target).or_default();
            if staged.len() < MAX_TRACKED_RELAY_DELIVERIES {
                staged.push(pending);
            } else {
                return None;
            }
        } else if let Err(error) = self.record_bound_reply_relay(&target, pending) {
            log::warn!("[reply-relay] target={target} registration failed: {error}");
            return None;
        }
        Some(id)
    }

    pub(crate) fn trace_spawn_reply_relay(&self, target_tab: &str, pending: &PendingReplyRelay) {
        let task = crate::paths::is_dev_instance()
            .then(|| self.reply_relay_task_binding(target_tab))
            .flatten();
        crate::flow_trace::record_flow(
            &pending.delivery_id,
            "relay.expected",
            target_tab,
            &pending.provider_id,
            "spawn_brief",
            task.as_ref().map(|task| task.attempt_id.as_str()),
        );
    }

    pub(crate) fn arm_spawn_reply_relay(&self, target: &str, nonce: &str) {
        self.stage_worker_restart(target);
        if let Some(first) = self
            .ext::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
            .arm_spawn(target, nonce)
        {
            self.retain_spawn_worker_brief(target, &first.sender_tab, &first.user_text);
        }
    }

    pub(crate) fn activate_bound_reply_relays(&self, target: &str) {
        let binding = self.current_reply_relay_binding(target);
        let Some(binding) = binding else { return };
        if let Err(error) = self
            .ext::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
            .ensure(self, target, binding.clone())
        {
            log::warn!("[reply-relay] target={target} binding observer failed: {error}");
        }
        let pending = self
            .ext::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
            .staged
            .lock()
            .remove(target)
            .unwrap_or_default();
        for mut pending in pending {
            if pending.provider_id != binding.provider || pending.binding_nonce != binding.nonce {
                continue;
            }
            pending.session_id = binding.session.clone();
            pending.task = self.reply_relay_task_binding(target);
            if let Err(error) = self.record_bound_reply_relay(target, pending) {
                log::warn!("[reply-relay] target={target} activation failed: {error}");
            }
        }
        self.restore_worker_after_binding(target);
        self.ext::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
            .wake(target);
    }

    pub(crate) fn reconcile_tool_less_worker_ack(&self, target: &str) {
        let entry = self
            .ext::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
            .entries
            .lock()
            .get(target)
            .cloned();
        let Some(entry) = entry else { return };
        let pending = entry.worker.lock().pending();
        for pending in pending {
            self.observe_tool_less_worker_ack(target, &entry.binding, &pending, &entry.worker);
            if !entry
                .worker
                .lock()
                .needs_ack(&pending, entry.binding.source(), true)
            {
                continue;
            }
            let evidence = self
                .agent_registry
                .with_provider(&entry.binding.provider, |provider| {
                    provider.read_reply_relay_turn(
                        &entry.binding.cwd,
                        &entry.binding.session,
                        pending.after_offset,
                    )
                })
                .flatten();
            self.observe_tool_less_worker_answer_ack(
                target,
                &entry.binding,
                &pending,
                &entry.worker,
                evidence.as_ref(),
            );
        }
    }

    pub(super) fn current_reply_relay_binding(&self, target: &str) -> Option<RelayBinding> {
        let (provider, session, nonce, cwd) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(target)?;
            if !tab.launch_session_authenticated() {
                return None;
            }
            (
                tab.agent_type.clone()?,
                tab.agent_session_id.clone()?,
                tab.session_binding_nonce.clone()?,
                tab.cwd.clone().unwrap_or_default(),
            )
        };
        let history = self
            .agent_registry
            .with_provider(&provider, |provider| {
                (provider.is_tool_less() && provider.supports_reply_relay())
                    .then(|| provider.session_file_path(&cwd, &session))
                    .flatten()
            })
            .flatten()?;
        Some(RelayBinding {
            provider,
            session,
            nonce,
            cwd,
            history,
        })
    }

    pub(super) fn record_bound_reply_relay(
        &self,
        target: &str,
        pending: PendingReplyRelay,
    ) -> Result<(), String> {
        let binding = self
            .current_reply_relay_binding(target)
            .ok_or("relay target has no authenticated binding")?;
        if binding.source().provider_id != pending.provider_id
            || binding.session != pending.session_id
            || binding.nonce != pending.binding_nonce
        {
            return Err("relay binding changed".into());
        }
        self.remember_worker_turn(target, &pending)?;
        let entry = self
            .ext::<Arc<crate::service::reply_relay::ReplyRelayRegistry>>()
            .ensure(self, target, binding)?;
        let unavailable = { entry.model.lock().unavailable_reason().map(str::to_string) };
        if let Some(reason) = unavailable {
            self.send_reply_relay_action(
                target,
                &entry.binding,
                &pending,
                ReplyRelayDecision::Notice {
                    text: format!("Reply relay is not active for this session: {reason}"),
                    user_turn_id: None,
                },
            );
            return Ok(());
        }
        log::info!(
            "[reply-relay] target={target} register delivery={} session={} after_offset={}",
            pending.delivery_id,
            pending.session_id,
            pending.after_offset
        );
        let trace_pending = crate::paths::is_dev_instance().then(|| {
            (
                pending.delivery_id.clone(),
                pending.provider_id.clone(),
                pending.task.as_ref().map(|task| task.attempt_id.clone()),
            )
        });
        {
            let mut model = entry.model.lock();
            let mut worker = entry.worker.lock();
            let worker_pending = pending.clone();
            if !model.record(pending) {
                return Err("relay capacity or duplicate delivery".into());
            }
            worker.track(worker_pending);
        }
        if let Some((id, provider, attempt)) = trace_pending {
            crate::flow_trace::record_flow(
                &id,
                "relay.registered",
                target,
                &provider,
                "registered",
                attempt.as_deref(),
            );
        }
        let _ = entry.signal.try_send(());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
