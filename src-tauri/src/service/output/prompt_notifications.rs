use super::ServiceContext;
use crate::agent::prompt::PromptDecision;
use crate::multiplexer::types::PendingPrompt;

pub(super) fn should_notify_pending_prompt(
    decision: &PromptDecision,
    existing_prompt: Option<&PendingPrompt>,
) -> bool {
    let prompt = match decision {
        PromptDecision::NewPrompt { prompt, .. } | PromptDecision::PromptUpdated { prompt, .. } => {
            prompt
        }
        PromptDecision::None | PromptDecision::PromptCleared { .. } => return false,
    };
    let Some(existing_prompt) = existing_prompt else {
        return true;
    };
    let mut existing_prompt = existing_prompt.clone();
    existing_prompt.selected_values = prompt.selected_values.clone();
    &existing_prompt != prompt
}

impl ServiceContext {
    pub(super) fn notify_pending_prompt(&self, tab_id: &str, prompt: &PendingPrompt) {
        let labels = prompt
            .options
            .iter()
            .map(|option| option.label.trim().to_string())
            .collect::<Vec<_>>();
        let actions = (!labels.is_empty())
            .then(|| codeterm_types::notification::answer_actions(&labels).ok())
            .flatten();
        let body = std::iter::once(prompt.question.as_str())
            .chain(labels.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        self.create_notification_ext(
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
            None,
        );
    }
}
