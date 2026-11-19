use crate::multiplexer::types::Tab;

#[derive(serde::Deserialize)]
pub struct ExpectedChatRecipient {
    pub provider_id: String,
    pub binding_nonce: Option<String>,
    pub session_id: Option<String>,
}

impl ExpectedChatRecipient {
    pub(crate) fn matches(&self, tab: &Tab) -> bool {
        if !matches!(tab.backing, crate::multiplexer::types::TabBacking::Pty) {
            return false;
        }
        if let Some(exit) = tab
            .exited_chat_recipient()
            .or_else(|| tab.resuming_exited_chat_recipient())
        {
            if self.session_id.is_none() {
                return self.provider_id == exit.provider_id
                    && self.binding_nonce.is_some()
                    && self.binding_nonce == tab.session_binding_nonce
                    && tab.agent_type.is_some();
            }
            return self.provider_id == exit.provider_id
                && self.session_id.as_ref() == Some(&exit.session_id);
        }
        if tab.agent_type.as_ref() != Some(&self.provider_id) {
            return false;
        }
        match self.session_id.as_deref() {
            Some(session_id) => tab.agent_session_id.as_deref() == Some(session_id),
            None => self.binding_nonce.is_some() && self.binding_nonce == tab.session_binding_nonce,
        }
    }
}
