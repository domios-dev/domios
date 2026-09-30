use super::ServiceContext;
use crate::multiplexer::types::PendingPrompt;

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
}

#[cfg(test)]
mod tests {
    use super::is_second_rendering;
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
}
