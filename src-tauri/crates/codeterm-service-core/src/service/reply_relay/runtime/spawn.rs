use super::{PendingReplyRelay, ReplyRelayRegistry};

impl ReplyRelayRegistry {
    pub(crate) fn spawn_task_message(
        &self,
        mux: &crate::multiplexer::state::MultiplexerState,
        registry: &crate::agent::registry::AgentRegistry,
        target_tab: &str,
        body: &str,
    ) -> Option<(String, Option<PendingReplyRelay>)> {
        let (src, label, model, from_agent) = {
            let target = mux.get_tab(target_tab)?;
            let src = target
                .spawned_by_tab_id
                .clone()
                .or_else(|| target.parent_tab_id.clone())?;
            if src.is_empty() {
                return None;
            }
            let source = mux.get_tab(&src);
            (
                src.clone(),
                mux.display_label_for_tab(&src),
                source.and_then(|tab| tab.agent_session_model.clone()),
                source.is_some_and(|tab| tab.agent_type.is_some() || tab.agent_booting),
            )
        };
        let text = crate::service::messenger::delivery_message(
            true,
            &src,
            target_tab,
            &label,
            model.as_deref(),
            None,
            from_agent,
            body,
        )?;
        let pending = from_agent
            .then(|| self.stage_spawn_reply_relay(mux, registry, target_tab, &src, &text))
            .flatten();
        Some((text, pending))
    }
    fn stage_spawn_reply_relay(
        &self,
        mux: &crate::multiplexer::state::MultiplexerState,
        registry: &crate::agent::registry::AgentRegistry,
        target: &str,
        sender: &str,
        text: &str,
    ) -> Option<PendingReplyRelay> {
        let provider = mux.get_tab(target).and_then(|tab| tab.agent_type.clone());
        let provider = provider?;
        let sender_is_agent = mux
            .get_tab(sender)
            .is_some_and(|tab| tab.agent_type.is_some() || tab.agent_booting);
        if crate::service::reply_relay::model::reply_relay_target(
            crate::service::reply_relay::model::ReplyRelayRoute {
                sender,
                target,
                tool_less: true,
                supported: true,
                sender_is_agent,
                has_reply_hint: false,
                remote: false,
                notice: false,
                spawn_brief: true,
            },
        )
        .is_none()
        {
            return None;
        }
        let normalization = registry
            .with_provider(&provider, |provider| {
                (provider.is_tool_less()
                    && provider.supports_reply_relay()
                    && provider.session_starts_on_first_user_turn())
                .then(|| provider.turn_evidence_normalization())
            })
            .flatten();
        let normalization = normalization?;
        let delivery_id = uuid::Uuid::new_v4().to_string();
        let pending = PendingReplyRelay {
            sender_tab: sender.into(),
            delivery_id,
            provider_id: provider,
            session_id: String::new(),
            binding_nonce: String::new(),
            after_offset: 0,
            user_text: text.into(),
            turn_normalization: normalization,
            hops: 0,
            task: None,
        };
        self.staged
            .lock()
            .insert(target.into(), vec![pending.clone()]);
        Some(pending)
    }

    pub(super) fn arm_spawn(&self, target: &str, nonce: &str) -> Option<PendingReplyRelay> {
        self.entries.lock().remove(target);
        let mut staged = self.staged.lock();
        let turns = staged.get_mut(target)?;
        for pending in turns.iter_mut() {
            if pending.binding_nonce.is_empty() {
                pending.binding_nonce = nonce.into();
            }
        }
        turns.first().cloned()
    }
}
