use super::ServiceContext;
use crate::agent::prompt::PromptDecision;
use crate::multiplexer::types::PendingPrompt;

pub(super) fn should_notify_pending_prompt(
    decision: &PromptDecision,
    existing_prompt: Option<&PendingPrompt>,
    existing_kind: Option<codeterm_types::pty::PromptKind>,
    prompt_kind: codeterm_types::pty::PromptKind,
) -> bool {
    let prompt = match decision {
        PromptDecision::NewPrompt { prompt, .. } | PromptDecision::PromptUpdated { prompt, .. } => {
            prompt
        }
        PromptDecision::None | PromptDecision::PromptCleared { .. } => return false,
    };
    let (Some(existing_prompt), Some(existing_kind)) = (existing_prompt, existing_kind) else {
        return true;
    };
    codeterm_types::pty::PromptIdentity::new(existing_kind, existing_prompt)
        != codeterm_types::pty::PromptIdentity::new(prompt_kind, prompt)
}

impl ServiceContext {
    pub(super) fn notify_pending_prompt(
        &self,
        tab_id: &str,
        prompt: &PendingPrompt,
        kind: codeterm_types::pty::PromptKind,
    ) {
        let labels = prompt
            .options
            .iter()
            .map(|option| option.label.trim().to_string())
            .collect::<Vec<_>>();
        let identity = codeterm_types::pty::PromptIdentity::new(kind, prompt);
        let actions = (!prompt.options.is_empty()).then(|| {
            prompt
                .options
                .iter()
                .map(|option| {
                    codeterm_types::notification::NotificationAction::RespondPromptOption {
                        label: option.label.trim().to_string(),
                        value: option.value.clone(),
                        identity: identity.clone(),
                    }
                })
                .collect::<Vec<_>>()
        });
        let actions = actions.filter(|actions| {
            codeterm_types::notification::validate_answer_labels(actions).is_ok()
        });
        let body = std::iter::once(prompt.question.as_str())
            .chain(labels.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        self.create_prompt_notification_ext(
            "Agent needs your response".to_string(),
            Some(body),
            Some("warning"),
            actions,
            Some(true),
            super::super::notification::NotificationOrigin::local_tab(tab_id.to_string()),
            super::super::notification::PushIntent {
                level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
                event_key: Some(format!("pending-prompt:{tab_id}:{}", uuid::Uuid::new_v4())),
            },
        );
    }
}
