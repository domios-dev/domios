use crate::notification::store::NotificationStore;
use codeterm_types::pty::PromptIdentity;

pub(crate) fn resolve_displaced_prompt(
    store: &mut NotificationStore,
    tab_id: &str,
    previous: Option<PromptIdentity>,
    current: Option<&PromptIdentity>,
) -> Vec<String> {
    let Some(previous) = previous.filter(|previous| Some(previous) != current) else {
        return Vec::new();
    };
    store.resolve_prompt_notifications(tab_id, &previous)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::store::{Ask, NotificationProvenance, NotificationSeverity};
    use codeterm_types::notification::NotificationAction;
    use codeterm_types::pty::{PendingPrompt, PromptKind, PromptOption};

    fn prompt(label: &str, value: &str) -> PromptIdentity {
        PromptIdentity::new(
            PromptKind::Provider,
            &PendingPrompt {
                question: "opaque".into(),
                options: vec![PromptOption {
                    label: label.into(),
                    value: value.into(),
                }],
                sections: None,
                multi_select: None,
                submit_available: None,
                selected_values: None,
            },
        )
    }

    fn notification(store: &mut NotificationStore, identity: &PromptIdentity) -> String {
        store.add_ext(
            "opaque title".into(),
            Some("opaque body".into()),
            NotificationSeverity::Warning,
            NotificationProvenance {
                tab_id: Some("tab".into()),
                ..Default::default()
            },
            Ask::offering(Some(vec![NotificationAction::RespondPromptOption {
                label: identity.options[0].label.clone(),
                value: identity.options[0].value.clone(),
                identity: identity.clone(),
            }])),
            Some(true),
            Some(crate::notification::push::NotifyLevel::ActionNeeded),
        )
    }

    fn assert_transition_resolves(previous: PromptIdentity, current: Option<&PromptIdentity>) {
        let mut store = NotificationStore::new(10);
        let id = notification(&mut store, &previous);
        let resolved = resolve_displaced_prompt(&mut store, "tab", Some(previous), current);
        assert_eq!(resolved, vec![id.clone()]);
        let record = store.notification(&id).expect("resolved prompt record");
        assert!(record.resolved);
        assert!(record.read);
        assert!(record.actions.is_none());
    }

    #[test]
    fn replacement_resolves_the_previous_prompt_only() {
        let mut store = NotificationStore::new(10);
        let previous = prompt("old", "1");
        let current = prompt("new", "1");
        let old_id = notification(&mut store, &previous);
        let new_id = notification(&mut store, &current);

        let resolved = resolve_displaced_prompt(&mut store, "tab", Some(previous), Some(&current));

        assert_eq!(resolved, vec![old_id.clone()]);
        assert!(store.notification(&old_id).unwrap().resolved);
        let current_record = store.notification(&new_id).unwrap();
        assert!(!current_record.resolved);
        assert!(current_record.actions.is_some());
    }

    #[test]
    fn idle_transition_resolves_the_prompt() {
        assert_transition_resolves(prompt("choice", "1"), None);
    }

    #[test]
    fn stop_transition_resolves_the_prompt() {
        assert_transition_resolves(prompt("choice", "1"), None);
    }

    #[test]
    fn exit_transition_resolves_the_prompt() {
        assert_transition_resolves(prompt("choice", "1"), None);
    }

    #[test]
    fn unchanged_prompt_identity_stays_open() {
        let mut store = NotificationStore::new(10);
        let identity = prompt("choice", "1");
        let id = notification(&mut store, &identity);

        assert!(resolve_displaced_prompt(
            &mut store,
            "tab",
            Some(identity.clone()),
            Some(&identity),
        )
        .is_empty());
        assert!(!store.notification(&id).unwrap().resolved);
    }
}
