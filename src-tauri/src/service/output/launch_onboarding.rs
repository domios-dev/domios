use crate::agent::prompt::PromptDecision;
use crate::service::ServiceContext;

impl ServiceContext {
    pub(super) fn defer_launch_onboarding_prompt(
        &self,
        decision: &PromptDecision,
        screen_text: Option<&str>,
        prompt_kind: codeterm_types::pty::PromptKind,
        require_user_confirmation: bool,
        notify_owner: bool,
    ) -> bool {
        if prompt_kind != codeterm_types::pty::PromptKind::Provider {
            return false;
        }
        let tab_id = match decision {
            PromptDecision::NewPrompt { tab_id, .. }
            | PromptDecision::PromptUpdated { tab_id, .. } => tab_id,
            _ => return false,
        };
        let target = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| {
                    tab.agent_type.as_deref().is_some_and(|agent_type| {
                        super::super::directory_trust::launch_onboarding_check_applies(
                            tab, agent_type,
                        )
                    })
                })
                .and_then(|tab| Some((tab.pty_id.clone()?, tab.agent_type.clone()?)))
        };
        let Some((pty_id, agent_type)) = target else {
            return false;
        };
        let screen = screen_text
            .map(str::to_string)
            .or_else(|| self.screen_state.lock().get_screen_text(&pty_id));
        let Some(screen) = screen else {
            return false;
        };
        let service = self.clone();
        let decision = decision.clone();
        let tab = tab_id.to_string();
        std::thread::spawn(move || {
            if service.respond_to_launch_onboarding_until_clear(&tab, &pty_id, &agent_type) {
                return;
            }
            let current_screen = service.screen_state.lock().get_screen_text(&pty_id);
            if current_screen.as_deref() != Some(screen.as_str()) {
                return;
            }
            service.apply_prompt_decision_immediately(
                &decision,
                Some(&screen),
                prompt_kind,
                require_user_confirmation,
                notify_owner,
            );
        });
        true
    }
}
