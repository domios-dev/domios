use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn contradictory_advisory_bypass_is_repaired_once_into_a_fresh_nonfull_session() {
    use codeterm_neighbors::permissions::MachineAccess;
    for access in [MachineAccess::ReadOnly, MachineAccess::Folder] {
        for global in [false, true] {
            let (mut mux, general) = MultiplexerState::new();
            let workspace = mux.find_workspace_for_tab(&general).unwrap();
            let old = mux.create_tab(&workspace).unwrap();
            let tab = mux.get_tab_mut(&old).unwrap();
            tab.agent_type = Some("advisory-provider".into());
            tab.agent_session_id = Some("unsafe-old-session".into());
            tab.session_binding_nonce = Some("old-nonce".into());
            tab.restorable_launch.permission_bypass_requested = true;
            let fixture = ServiceFixture::new(mux);
            fixture
                .service
                .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
            fixture
                .service
                .set_neighbor_friend_receiver_for_test(&"b".repeat(64), Some(true), 1);
            fixture.set_agent_full_access(global);
            let service = &fixture.service;
            service.agent_registry.register_provider(Box::new(
                crate::agent::test_provider::MockProvider::new("advisory-provider")
                    .with_initial_prompt_resume(),
            ));
            let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
            let friend = "a".repeat(64);
            let permission = Permission {
                allowed: true,
                scope: "opaque scope".into(),
                machine_access: access,
                folder: (access == MachineAccess::Folder)
                    .then(|| fixture.temp.path().to_string_lossy().into_owned()),
                ..Default::default()
            };
            let cwd = crate::service::neighbors::confinement::prepare_folder(
                &metadata,
                &friend,
                &permission,
            )
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
                    launch_options: None,
                },
            );
            metadata.save_settings(settings).await.unwrap();
            service
                .restore_neighbor_concierge_with(
                    &metadata,
                    "owner",
                    &friend,
                    |provider, _, cwd, grant, resume, _| async move {
                        assert!(resume.is_none());
                        assert_eq!(grant.machine_access, access);
                        let mut mux = service.mux.write();
                        let id = mux.create_tab(&workspace).unwrap();
                        let tab = mux.get_tab_mut(&id).unwrap();
                        tab.agent_type = provider;
                        tab.agent_session_id = Some("safe-fresh-session".into());
                        tab.session_binding_nonce = Some("fresh-nonce".into());
                        tab.cwd = Some(cwd);
                        Ok(id)
                    },
                )
                .await
                .unwrap();
            let settings = metadata.settings().await.unwrap();
            let current = &settings.bindings[&friend];
            assert_ne!(current, &old);
            assert!(service.mux.read().get_tab(&old).is_none());
            assert_eq!(
                service
                    .mux
                    .read()
                    .get_tab(current)
                    .unwrap()
                    .agent_session_id
                    .as_deref(),
                Some("safe-fresh-session")
            );
            assert!(
                !service
                    .mux
                    .read()
                    .get_tab(current)
                    .unwrap()
                    .restorable_launch
                    .permission_bypass_requested
            );
            for _ in 0..2 {
                service
                    .restore_neighbor_concierge_with(
                        &metadata,
                        "owner",
                        &friend,
                        |_, _, _, _, _, _| async {
                            panic!("corrected unchanged binding must remain stable")
                        },
                    )
                    .await
                    .unwrap();
            }
        }
    }
}

#[tokio::test]
async fn resumable_providers_keep_unchanged_sessions_but_start_fresh_for_changed_context() {
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
        fixture
            .service
            .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
        fixture
            .service
            .set_neighbor_friend_receiver_for_test(&"b".repeat(64), Some(true), 1);
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
                    assert!(resume.is_none());
                    assert_eq!(grant.scope, "opaque-updated-scope");
                    assert_eq!(options.reasoning_effort.as_deref(), Some("opaque-effort"));
                    assert_eq!(options.service_tier.as_deref(), Some("opaque-tier"));
                    let mut mux = service.mux.write();
                    assert!(mux.get_tab(&old).is_some());
                    let new = mux.create_tab(&workspace).unwrap();
                    let tab = mux.get_tab_mut(&new).unwrap();
                    tab.agent_type = actual_provider;
                    tab.agent_session_id = Some("opaque-fresh-session".into());
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
            Some("opaque-fresh-session")
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
        let repair = current.clone();
        service.mux.write().get_tab_mut(&repair).unwrap().cwd = Some("opaque-stale-folder".into());
        let ws = service.mux.read().find_workspace_for_tab(&repair).unwrap();
        service
            .restore_neighbor_concierge_with(
                &metadata,
                "owner",
                &friend,
                |provider, _, cwd, _, resume, _| async move {
                    assert_eq!(resume.as_deref(), Some("opaque-fresh-session"));
                    let mut mux = service.mux.write();
                    assert!(mux.get_tab(&repair).is_none());
                    let id = mux.create_tab(&ws).unwrap();
                    let tab = mux.get_tab_mut(&id).unwrap();
                    tab.agent_type = provider;
                    tab.agent_session_id = resume;
                    tab.cwd = Some(cwd);
                    Ok(id)
                },
            )
            .await
            .unwrap();
        let saved = metadata.settings().await.unwrap();
        assert_eq!(
            service
                .mux
                .read()
                .get_tab(&saved.bindings[&friend])
                .unwrap()
                .agent_session_id
                .as_deref(),
            Some("opaque-fresh-session")
        );
    }
}
