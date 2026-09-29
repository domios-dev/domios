use super::ServiceContext;
use crate::agent::prompt::PromptDecision;
use crate::multiplexer::types::PendingPrompt;
use std::collections::HashMap;
use std::sync::LazyLock;

static TRUST_NOTIFICATION_OWNERS: LazyLock<parking_lot::Mutex<HashMap<String, String>>> =
    LazyLock::new(Default::default);

fn claim_trust_notification(
    folder_provider: String,
    tab_id: &str,
    owner_still_pending: impl FnOnce(&str) -> bool,
) -> bool {
    let mut owners = TRUST_NOTIFICATION_OWNERS.lock();
    match owners.get(&folder_provider) {
        Some(owner) if owner != tab_id && owner_still_pending(owner) => false,
        _ => {
            owners.insert(folder_provider, tab_id.to_string());
            true
        }
    }
}

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
        if kind == codeterm_types::pty::PromptKind::DirectoryTrust {
            let context = {
                let mux = self.mux.read();
                mux.get_tab(tab_id).and_then(|tab| {
                    Some((
                        tab.agent_type.clone()?,
                        tab.cwd.clone().or_else(|| tab.provisioned_cwd.clone())?,
                    ))
                })
            };
            let Some((provider, cwd)) = context else {
                return;
            };
            let Some(folder) = std::fs::canonicalize(&cwd).ok() else {
                return;
            };
            let mut folder = folder.to_string_lossy().into_owned();
            if cfg!(windows) {
                folder.make_ascii_lowercase();
            }
            let key = format!("{provider}:{folder}");
            if !claim_trust_notification(key, tab_id, |owner| {
                self.mux.read().get_tab(owner).is_some_and(|tab| {
                    tab.pending_prompt_kind == Some(kind)
                })
            }) {
                crate::service::launch_readiness::owner_prompt_pending(tab_id, &prompt.question);
                return;
            }
        }
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
        let prompt_is_current = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pending_prompt.as_ref() == Some(prompt) && tab.pending_prompt_kind == Some(kind)
        });
        if prompt_is_current {
            crate::service::launch_readiness::owner_prompt_pending(tab_id, &prompt.question);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::claim_trust_notification;

    #[test]
    fn sibling_codex_trust_prompts_share_one_owner_notification() {
        let folder = format!("notification-test-{}", uuid::Uuid::new_v4());
        assert!(claim_trust_notification(folder.clone(), "first", |_| false));
        assert!(!claim_trust_notification(folder.clone(), "sibling", |_| true));
        assert!(claim_trust_notification(folder, "sibling", |_| false));
    }
}
