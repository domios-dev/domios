use super::ServiceContext;
use crate::service::MuxEmitCause;

impl ServiceContext {
    pub(crate) fn push_structured_prompt(
        &self,
        tab_id: &str,
        prompt: Option<crate::multiplexer::types::PendingPrompt>,
    ) {
        let kind = super::super::structured_prompt::kind_for_prompt(tab_id, prompt.as_ref());
        let automatic = if kind == codeterm_types::pty::PromptKind::Permission {
            super::super::structured_prompt::claim_automatic_permission(self, tab_id)
        } else {
            None
        };
        let kind = super::super::structured_prompt::kind_for_prompt(tab_id, prompt.as_ref());
        {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            let next_kind = prompt.as_ref().map(|_| kind);
            if tab.pending_prompt == prompt && tab.pending_prompt_kind == next_kind {
                return;
            }
            tab.pending_prompt = prompt.clone();
            tab.pending_prompt_kind = next_kind;
        }
        self.ui.emit(
            "prompt-state-changed",
            serde_json::to_value(serde_json::json!({ "tab_id": tab_id, "prompt": prompt, "kind": kind }))
                .unwrap_or_default(),
        );
        self.emit_state_changed(MuxEmitCause::Prompt);
        if let Some(reply) = automatic {
            reply.dispatch(self.clone(), tab_id.into());
        }
    }
}
