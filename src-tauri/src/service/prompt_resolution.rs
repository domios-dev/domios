use crate::notification::store::NotificationStore;
use codeterm_types::pty::PromptIdentity;

pub(crate) fn clear_tab_prompt(
    tab_id: &str,
    tab: Option<&mut crate::multiplexer::types::Tab>,
) -> Option<PromptIdentity> {
    let identity = tab.map(|tab| {
        let identity = tab.prompt_identity();
        tab.pending_prompt = None;
        tab.pending_prompt_kind = None;
        identity
    });
    identity.flatten()
}

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
    use crate::api::routes::AppState;
    use crate::notification::store::{Ask, NotificationProvenance, NotificationSeverity};
    use codeterm_types::notification::NotificationAction;
    use codeterm_types::pty::{PendingPrompt, PromptKind, PromptOption};
    use std::sync::Arc;

    fn prompt(label: &str, value: &str) -> PendingPrompt {
        PendingPrompt {
            question: "opaque".into(),
            options: vec![PromptOption {
                label: label.into(),
                value: value.into(),
            }],
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        }
    }

    fn identity(prompt: &PendingPrompt) -> PromptIdentity {
        PromptIdentity::new(PromptKind::Provider, prompt)
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

    fn pending_prompt_fixture() -> (crate::service::test_fixture::ServiceFixture, String, String) {
        let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let previous = prompt("previous", "1");
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).expect("fixture tab");
            tab.agent_type = Some("provider".into());
            tab.pending_prompt = Some(previous.clone());
            tab.pending_prompt_kind = Some(PromptKind::Provider);
        }
        let previous_id = add_prompt_notification(&fixture, &tab_id, &previous);
        assert_open_prompt_notification(&fixture, &previous_id, &identity(&previous));
        (fixture, tab_id, previous_id)
    }

    fn add_prompt_notification(
        fixture: &crate::service::test_fixture::ServiceFixture,
        tab_id: &str,
        prompt: &PendingPrompt,
    ) -> String {
        let identity = identity(prompt);
        let actions = prompt
            .options
            .iter()
            .map(|option| NotificationAction::RespondPromptOption {
                label: option.label.clone(),
                value: option.value.clone(),
                identity: identity.clone(),
            })
            .collect();
        fixture.service.create_prompt_notification_ext(
            "opaque title".into(),
            Some("opaque body".into()),
            Some("warning"),
            Some(actions),
            Some(true),
            crate::service::notification::NotificationOrigin::local_tab(tab_id.into()),
            crate::service::notification::PushIntent {
                level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
                event_key: Some(format!(
                    "prompt-resolution-test:{tab_id}:{}",
                    identity.options[0].value
                )),
            },
        )
    }

    fn assert_open_prompt_notification(
        fixture: &crate::service::test_fixture::ServiceFixture,
        id: &str,
        expected_identity: &PromptIdentity,
    ) {
        let notifications = fixture.service.notif.lock();
        let record = notifications.notification(id).expect("prompt notification");
        assert_eq!(
            record.level,
            Some(crate::notification::push::NotifyLevel::ActionNeeded)
        );
        assert!(!record.resolved);
        let actions = record.actions.as_ref().expect("prompt answer actions");
        assert!(actions.iter().any(|action| matches!(
            action,
            NotificationAction::RespondPromptOption { identity, .. }
                if identity == expected_identity
        )));
    }

    fn assert_resolved_and_new_prompt_notifies(
        fixture: &crate::service::test_fixture::ServiceFixture,
        tab_id: &str,
        previous_id: &str,
    ) {
        {
            let notifications = fixture.service.notif.lock();
            let record = notifications
                .notification(previous_id)
                .expect("resolved prompt notification");
            assert!(record.resolved);
            assert!(record.actions.is_none());
        }
        {
            let mux = fixture.service.mux.read();
            let tab = mux.get_tab(tab_id).expect("fixture tab");
            assert!(tab.pending_prompt.is_none());
            assert!(tab.pending_prompt_kind.is_none());
        }

        let current = prompt("current", "2");
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(tab_id).expect("fixture tab");
            tab.agent_type.get_or_insert_with(|| "provider".into());
            tab.pending_prompt = Some(current.clone());
            tab.pending_prompt_kind = Some(PromptKind::Provider);
        }
        let current_id = add_prompt_notification(fixture, tab_id, &current);
        assert_ne!(current_id, previous_id);
        assert_open_prompt_notification(fixture, &current_id, &identity(&current));
    }

    fn app_state(fixture: &crate::service::test_fixture::ServiceFixture) -> AppState {
        let config = crate::config::schema::AppConfig::default();
        AppState {
            svc: Arc::new(fixture.service.clone()),
            cron: Arc::new(parking_lot::Mutex::new(
                crate::cron::store::CronStore::new_with_path(
                    fixture.temp.path().join("prompt-resolution-crons.json"),
                ),
            )),
            cron_wake: Arc::new(std::sync::Condvar::new()),
            api_port: 0,
            update_state: Arc::new(parking_lot::Mutex::new(crate::update::UpdateState {
                install_method: crate::update::InstallMethod::Unknown,
                last_check: None,
                last_check_time: None,
                channel: config.updater.channel,
                phase: crate::update::UpdatePhase::Idle,
                last_error: None,
                phase_changed_at: None,
                downloaded_bytes: 0,
                download_total_bytes: None,
            })),
            config: Arc::new(parking_lot::Mutex::new(config)),
            mesh: Arc::new(parking_lot::Mutex::new(crate::mesh::MeshManager::new(
                "prompt-resolution-test".into(),
                0,
            ))),
            keepawake: Arc::new(parking_lot::Mutex::new(None)),
            resize_owner: Arc::new(parking_lot::Mutex::new(
                crate::resize_owner::ResizeOwnerRegistry::default(),
            )),
            rest_api: Arc::new(crate::startup::RestApiControl::new()),
            test_clock: None,
            events: fixture.service.events.clone(),
            web_dist: None,
            snapshot_bound: crate::api::snapshot_bound::SnapshotBound::default(),
        }
    }

    #[test]
    fn replacement_resolves_the_previous_prompt_only() {
        let mut store = NotificationStore::new(10);
        let previous = identity(&prompt("old", "1"));
        let current = identity(&prompt("new", "1"));
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
    fn idle_transition_resolves_the_prompt_through_agent_interruption() {
        let (fixture, tab_id, previous_id) = pending_prompt_fixture();
        let pty_id = format!("idle-transition-{tab_id}");
        fixture
            .service
            .mux
            .write()
            .link_pty(&tab_id, pty_id.clone());

        fixture.service.mark_agent_interrupted(&pty_id);

        assert_resolved_and_new_prompt_notifies(&fixture, &tab_id, &previous_id);
    }

    #[tokio::test]
    async fn stop_transition_resolves_the_prompt_through_hook() {
        let (fixture, tab_id, previous_id) = pending_prompt_fixture();
        let state = app_state(&fixture);

        let _ = crate::api::hook_routes::hook_stop(
            axum::extract::State(state),
            axum::extract::Path(tab_id.clone()),
            axum::http::HeaderMap::new(),
            crate::api::optional_json::OptionalJson(None),
        )
        .await
        .expect("stop hook accepted");

        assert_resolved_and_new_prompt_notifies(&fixture, &tab_id, &previous_id);
    }

    #[test]
    fn exit_transition_resolves_the_prompt_through_lifecycle_clear() {
        let (fixture, tab_id, previous_id) = pending_prompt_fixture();

        assert!(fixture
            .service
            .clear_tab_agent_identity_for_lifecycle_exit_with_cause(
                &tab_id,
                "test lifecycle exit",
            ));

        assert_resolved_and_new_prompt_notifies(&fixture, &tab_id, &previous_id);
    }

    #[test]
    fn unchanged_prompt_identity_stays_open() {
        let mut store = NotificationStore::new(10);
        let identity = identity(&prompt("choice", "1"));
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
