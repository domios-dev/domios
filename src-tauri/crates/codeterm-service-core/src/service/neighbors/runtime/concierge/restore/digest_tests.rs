use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn resumable_providers_reconcile_stable_digest_and_resume_once_per_real_change() {
    for provider in ["opaque-provider-one", "opaque-provider-two"] {
        let (mut mux, general) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&general).unwrap();
        let old = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&old).unwrap();
        tab.agent_type = Some(provider.into());
        tab.agent_session_id = Some("opaque-same-session".into());
        tab.session_binding_nonce = Some("opaque-old-nonce".into());
        tab.restorable_launch.identity_instructions =
            Some("opaque-role-onboarding-without-assignment".into());
        let fixture = ServiceFixture::new(mux);
        let service = &fixture.service;
        service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new(provider).with_initial_prompt_resume(),
        ));
        let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
        let friend = "a".repeat(64);
        let mut permission = Permission {
            allowed: true,
            scope: "opaque-original-scope".into(),
            ..Default::default()
        };
        let cwd =
            crate::service::neighbors::confinement::prepare_folder(&metadata, &friend, &permission)
                .await
                .unwrap();
        service.mux.write().get_tab_mut(&old).unwrap().cwd = Some(cwd);
        let mut settings = crate::service::neighbors::storage::Settings::default();
        settings.bindings.insert(friend.clone(), old.clone());
        settings
            .permissions
            .insert(friend.clone(), permission.clone());
        settings.initial_contexts.insert(
            friend.clone(),
            crate::service::neighbors::storage::InitialContext {
                gateway_tab: old.clone(),
                digest: initial_prompt::digest(&friend, &permission),
                launch_options: Some(ConciergeOptions {
                    reasoning_effort: Some("opaque-effort".into()),
                    service_tier: Some("opaque-tier".into()),
                }),
            },
        );
        metadata.save_settings(settings).await.unwrap();
        for _ in 0..3 {
            service
                .restore_neighbor_concierge_with(
                    &metadata,
                    "owner",
                    &friend,
                    |_, _, _, _, _, _| async {
                        panic!("unchanged context must not replace a concierge")
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(
            service
                .mux
                .read()
                .get_tab(&old)
                .unwrap()
                .agent_session_id
                .as_deref(),
            Some("opaque-same-session")
        );
        permission.scope = "opaque-updated-scope".into();
        let mut settings = metadata.settings().await.unwrap();
        settings
            .permissions
            .insert(friend.clone(), permission.clone());
        metadata.save_settings(settings).await.unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let invoked = calls.clone();
        service
            .restore_neighbor_concierge_with(
                &metadata,
                "owner",
                &friend,
                |actual_provider, _, cwd, grant, resume, options| async move {
                    invoked.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(actual_provider.as_deref(), Some(provider));
                    assert_eq!(resume.as_deref(), Some("opaque-same-session"));
                    assert_eq!(grant.scope, "opaque-updated-scope");
                    assert_eq!(options.reasoning_effort.as_deref(), Some("opaque-effort"));
                    assert_eq!(options.service_tier.as_deref(), Some("opaque-tier"));
                    let mut mux = service.mux.write();
                    assert!(mux.get_tab(&old).is_none());
                    let new = mux.create_tab(&workspace).unwrap();
                    let tab = mux.get_tab_mut(&new).unwrap();
                    tab.agent_type = actual_provider;
                    tab.agent_session_id = resume;
                    tab.cwd = Some(cwd);
                    // A provider's saved prompt representation is not the binding identity.
                    tab.restorable_launch.identity_instructions =
                        Some("opaque-normalized-provider-context".into());
                    Ok(new)
                },
            )
            .await
            .unwrap();
        for _ in 0..3 {
            service
                .restore_neighbor_concierge_with(
                    &metadata,
                    "owner",
                    &friend,
                    |_, _, _, _, _, _| async {
                        panic!("one real change must replace at most once")
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let saved = metadata.settings().await.unwrap();
        let current = &saved.bindings[&friend];
        assert_eq!(
            service
                .mux
                .read()
                .get_tab(current)
                .unwrap()
                .agent_session_id
                .as_deref(),
            Some("opaque-same-session")
        );
        assert_eq!(
            saved.initial_context_digest(&friend),
            Some(initial_prompt::digest(&friend, &permission).as_str())
        );
        let receiver = codeterm_neighbors::adapter::ReceiverStatus {
            receiver_revision: 1,
            receiver_installation_id: Some("machine".into()),
            receiver_key_id: None,
            receiver_machine_label: Some("Here".into()),
            receiving_enabled: true,
            is_me: true,
            legacy_active: false,
        };
        let status = service.neighbor_concierge_view(&friend, &saved, &receiver);
        assert_eq!(
            status["initial_context_digest"],
            initial_prompt::digest(&friend, &permission)
        );
        assert!(!status.to_string().contains("opaque-updated-scope"));
    }
}
