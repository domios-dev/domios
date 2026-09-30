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

/// A permission request reaches the tab twice: rendered from its hook payload
/// and parsed from the screen. Once one rendering is surfaced, the other is the
/// same prompt and must neither displace it nor notify again.
pub(super) fn is_second_rendering(
    existing: Option<(&PendingPrompt, codeterm_types::pty::PromptKind)>,
    new: (&PendingPrompt, codeterm_types::pty::PromptKind),
    request_rendering: Option<&PendingPrompt>,
) -> bool {
    let (Some((existing, existing_kind)), Some(request)) = (existing, request_rendering) else {
        return false;
    };
    existing_kind == new.1 && existing != new.0 && (new.0 == request || existing == request)
}

impl ServiceContext {
    pub(super) fn is_second_prompt_rendering(
        &self,
        tab_id: &str,
        prompt: &PendingPrompt,
        kind: codeterm_types::pty::PromptKind,
    ) -> bool {
        let request = self
            .prompt_detector
            .lock()
            .permission_request_rendering(tab_id);
        let mux = self.mux.read();
        let existing = mux
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_prompt.as_ref().zip(tab.pending_prompt_kind));
        is_second_rendering(existing, (prompt, kind), request.as_ref())
    }

    fn trust_notification_key(&self, tab_id: &str) -> Option<String> {
        let (provider, cwd) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            (
                tab.agent_type.clone()?,
                tab.cwd.clone().or_else(|| tab.provisioned_cwd.clone())?,
            )
        };
        let mut folder = std::fs::canonicalize(&cwd)
            .ok()?
            .to_string_lossy()
            .into_owned();
        if cfg!(windows) {
            folder.make_ascii_lowercase();
        }
        Some(format!("{provider}:{folder}"))
    }

    /// Raise the tab's pending-prompt condition to whoever owns the tab. `None`
    /// means a sibling tab's notice for the same folder trust already stands.
    pub(crate) fn notify_pending_prompt(
        &self,
        tab_id: &str,
        prompt: &PendingPrompt,
        kind: codeterm_types::pty::PromptKind,
    ) -> Option<crate::service::tab_conditions::ConditionRoute> {
        // Only a user-facing tab may own the shared trust notice, or a hidden sibling would silence it.
        if kind == codeterm_types::pty::PromptKind::DirectoryTrust
            && self.tab_is_user_facing(tab_id)
        {
            // Without a resolvable folder there is no sibling to share with, so the tab notifies alone.
            let key = self.trust_notification_key(tab_id);
            if let Some(key) = key {
                if !claim_trust_notification(key, tab_id, |owner| {
                    self.mux
                        .read()
                        .get_tab(owner)
                        .is_some_and(|tab| tab.pending_prompt_kind == Some(kind))
                }) {
                    crate::service::launch_readiness::owner_prompt_pending(
                        tab_id,
                        &prompt.question,
                    );
                    return None;
                }
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
        let route = self.raise_tab_condition(
            tab_id,
            crate::notification::dedupe::TabCondition::PendingPrompt,
            crate::service::tab_conditions::ConditionNotice {
                title: "Agent needs your response".to_string(),
                body,
                actions,
                hint: Some(format!(
                    "Answer it with `codeterm tab respond <option> --tab {tab_id}`."
                )),
            },
        );
        let prompt_is_current = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pending_prompt.as_ref() == Some(prompt) && tab.pending_prompt_kind == Some(kind)
        });
        if prompt_is_current {
            crate::service::launch_readiness::owner_prompt_pending(tab_id, &prompt.question);
        }
        Some(route)
    }
}

#[cfg(test)]
mod tests {
    use super::{claim_trust_notification, is_second_rendering};
    use codeterm_types::pty::{PendingPrompt, PromptKind, PromptOption};

    fn rendering(labels: &[&str]) -> PendingPrompt {
        PendingPrompt {
            question: "opaque".into(),
            options: labels
                .iter()
                .enumerate()
                .map(|(index, label)| PromptOption {
                    label: (*label).into(),
                    value: (index + 1).to_string(),
                })
                .collect(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        }
    }

    #[test]
    fn one_permission_request_surfaces_once_whichever_rendering_comes_first() {
        let kind = PromptKind::Provider;
        let hook = rendering(&["hook-a"]);
        let screen = rendering(&["screen-a", "screen-b"]);
        let later = rendering(&["later-a", "later-b"]);

        assert!(is_second_rendering(
            Some((&screen, kind)),
            (&hook, kind),
            Some(&hook)
        ));
        assert!(is_second_rendering(
            Some((&hook, kind)),
            (&screen, kind),
            Some(&hook)
        ));
        assert!(!is_second_rendering(None, (&hook, kind), Some(&hook)));
        assert!(!is_second_rendering(
            Some((&screen, kind)),
            (&later, kind),
            Some(&hook)
        ));
        assert!(!is_second_rendering(
            Some((&screen, kind)),
            (&hook, kind),
            None
        ));
        assert!(!is_second_rendering(
            Some((&screen, PromptKind::DirectoryTrust)),
            (&hook, kind),
            Some(&hook)
        ));
    }

    #[test]
    fn sibling_codex_trust_prompts_share_one_owner_notification() {
        let folder = format!("notification-test-{}", uuid::Uuid::new_v4());
        assert!(claim_trust_notification(folder.clone(), "first", |_| false));
        assert!(!claim_trust_notification(folder.clone(), "sibling", |_| {
            true
        }));
        assert!(claim_trust_notification(folder, "sibling", |_| false));
    }
}
