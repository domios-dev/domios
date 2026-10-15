use super::ServiceContext;

impl ServiceContext {
    pub(crate) fn push_structured_prompt(
        &self,
        tab_id: &str,
        prompt: Option<crate::multiplexer::types::PendingPrompt>,
    ) {
        {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            if tab.pending_prompt == prompt {
                return;
            }
            tab.pending_prompt = prompt.clone();
            tab.pending_prompt_kind = prompt
                .as_ref()
                .map(|_| codeterm_types::pty::PromptKind::Structured);
        }
        self.ui.emit(
            "prompt-state-changed",
            serde_json::to_value(serde_json::json!({ "tab_id": tab_id, "prompt": prompt }))
                .unwrap_or_default(),
        );
        self.emit_state_changed();
    }
}
