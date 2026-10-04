use crate::agent::prompt::ResolvedPrompt;
use crate::multiplexer::types::PendingPrompt;
use crate::service::ServiceContext;
use codeterm_plugin::provider::DirectoryTrustPromptResponse;
use codeterm_types::pty::PromptKind;

impl ServiceContext {
    /// Establishes an unattended launch-gate prompt on its tab; only an owner
    /// notice routes it, and the tab condition announces it once.
    pub(crate) fn publish_unattended_launch_prompt(
        &self,
        tab_id: &str,
        screen: &str,
        prompt: PendingPrompt,
        kind: PromptKind,
        notify_owner: bool,
    ) -> bool {
        let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
            tab_id,
            true,
            true,
            Some(ResolvedPrompt {
                prompt: prompt.clone(),
                explicit: false,
            }),
        );
        self.apply_prompt_decision_immediately(&decision, Some(screen), kind, true, false);
        let established = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pending_prompt.as_ref() == Some(&prompt) && tab.pending_prompt_kind == Some(kind)
        });
        if established && notify_owner {
            let _ = self.route_pending_prompt(tab_id, &prompt, kind);
        }
        established
    }

    pub(crate) fn provider_answers_directory_trust(&self, agent_type: &str, screen: &str) -> bool {
        super::super::trust_candidate::may_be_directory_trust_prompt(screen)
            && matches!(
                self.agent_registry
                    .with_provider(agent_type, |provider| {
                        provider.directory_trust_prompt_response(screen)
                    })
                    .flatten(),
                Some(DirectoryTrustPromptResponse::Answer { .. })
            )
    }
}
