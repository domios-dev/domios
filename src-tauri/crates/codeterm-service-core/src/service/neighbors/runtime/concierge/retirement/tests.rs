use super::*;
use crate::{
    multiplexer::{state::MultiplexerState, types::NeighborAuthority},
    service::test_fixture::ServiceFixture,
};
use std::sync::Arc;

struct NoCalls;
#[async_trait::async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NoCalls {
    async fn invoke(&self, _: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        panic!("nonreceiver concierge mutation contacted the relay")
    }
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("nonreceiver concierge mutation requested a relay token")
    }
}

async fn fixture(
    data: &std::path::Path,
) -> (ServiceFixture, Metadata, String, String, String, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let root = mux.create_tab(&workspace).unwrap();
    let child = mux.create_tab(&workspace).unwrap();
    let other_friend = mux.create_tab(&workspace).unwrap();
    let other_account = mux.create_tab(&workspace).unwrap();
    let friend = "a".repeat(64);
    for (id, uid, friendship, parent, nonce) in [
        (&root, "owner", friend.clone(), None, "root-nonce"),
        (
            &child,
            "owner",
            friend.clone(),
            Some(root.clone()),
            "child-nonce",
        ),
        (&other_friend, "owner", "c".repeat(64), None, "other-nonce"),
        (
            &other_account,
            "other-owner",
            "d".repeat(64),
            None,
            "foreign-nonce",
        ),
    ] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("opaque-pty".into());
        tab.agent_session_id = Some(format!("old-session-{id}"));
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
        tab.neighbor_authority = Some(NeighborAuthority {
            uid: uid.into(),
            friendship_id: friendship,
            root_tab_id: parent,
        });
    }
    let fixture = ServiceFixture::new_at(mux, data);
    fixture.register_neighbor_advisory_provider("opaque-pty");
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&friend, Some(true), 1);
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&"c".repeat(64), Some(true), 1);
    let metadata = Metadata::new(fixture.service.account_data_root().unwrap(), "owner");
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.enabled = true;
    settings.bindings.insert(friend.clone(), root.clone());
    settings
        .bindings
        .insert("c".repeat(64), other_friend.clone());
    settings.permissions.insert(
        friend.clone(),
        Permission {
            allowed: true,
            scope: "opaque local grant".into(),
            allow_memory: false,
            ..Default::default()
        },
    );
    settings.permissions.insert(
        "c".repeat(64),
        Permission {
            allowed: true,
            scope: "opaque other grant".into(),
            ..Default::default()
        },
    );
    settings.initial_contexts.insert(
        friend.clone(),
        crate::service::neighbors::storage::InitialContext {
            gateway_tab: root.clone(),
            digest: "opaque-context".into(),
            launch_options: Some(ConciergeOptions {
                reasoning_effort: Some("opaque-effort".into()),
                service_tier: None,
            }),
        },
    );
    settings.concierge_receivers.insert(
        friend.clone(),
        ConciergeReceiver {
            revision: 1,
            binding: fixture
                .service
                .neighbor_friend_receiver_binding(&friend)
                .unwrap(),
        },
    );
    metadata.save_settings(settings.clone()).await.unwrap();
    fixture
        .service
        .remember_neighbor_concierge_accounts("owner", &settings);
    let history = crate::service::neighbors::history::History {
        entries: vec![serde_json::from_value(serde_json::json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque retained conversation","at":1,"message_id":"b".repeat(32),"kind":"question","handling_state":"no_reply"})).unwrap()],
        ..Default::default()
    };
    metadata
        .write(format!("history-{friend}.json"), history)
        .await
        .unwrap();
    fixture.install_seeded_storage_desk();
    (fixture, metadata, friend, root, child, other_friend)
}

async fn persist_desk(service: &ServiceContext) {
    let service = service.clone();
    tokio::task::spawn_blocking(move || {
        crate::service::session_persist::write_session_snapshot(&service, &service.mux).unwrap();
    })
    .await
    .unwrap();
}

async fn regain(
    fixture: &ServiceFixture,
    metadata: &Metadata,
    friend: &str,
    revision: u64,
    other: &str,
) -> String {
    let service = &fixture.service;
    if !service.neighbor_friend_receiving_here(friend) {
        service.set_neighbor_friend_receiver_for_test(friend, Some(true), revision);
    }
    service
        .reconcile_neighbor_concierge_receiver_with(
            metadata,
            "owner",
            friend,
            true,
            revision,
            service.neighbor_history_generation(),
            || {
                service.restore_neighbor_concierge_with(
                    metadata,
                    "owner",
                    friend,
                    |provider, model, cwd, permission, resume, options| async move {
                        assert_eq!(provider.as_deref(), Some("opaque-pty"));
                        assert!(model.is_none());
                        assert!(!permission.allow_memory);
                        assert!(resume.is_none());
                        assert_eq!(options.reasoning_effort.as_deref(), Some("opaque-effort"));
                        let mut mux = service.mux.write();
                        let workspace = mux.find_workspace_for_tab(other).unwrap();
                        let id = mux.create_tab(&workspace).unwrap();
                        let tab = mux.get_tab_mut(&id).unwrap();
                        tab.agent_type = provider;
                        tab.agent_session_id = Some(format!("fresh-session-{id}"));
                        tab.session_binding_nonce = Some(format!("fresh-nonce-{id}"));
                        tab.cwd = Some(cwd);
                        drop(mux);
                        persist_desk(service).await;
                        Ok(id)
                    },
                )
            },
        )
        .await
        .unwrap();
    metadata.settings().await.unwrap().bindings[friend].clone()
}

#[tokio::test]
async fn per_friend_loss_retires_only_its_root_and_descendants_and_revokes_credentials() {
    let data = tempfile::tempdir().unwrap();
    let (fixture, metadata, friend, root, child, other) = fixture(data.path()).await;
    let service = &fixture.service;
    let root_token = service.mint_tab_credential(&root, "root-nonce").unwrap();
    let child_token = service.mint_tab_credential(&child, "child-nonce").unwrap();
    let other_token = service.mint_tab_credential(&other, "other-nonce").unwrap();
    let before = metadata.history_page(&friend, None, 50).await.unwrap();
    let grants = serde_json::to_value(metadata.settings().await.unwrap().permissions).unwrap();
    service.set_neighbor_friend_receiver_for_test(&friend, Some(false), 9);
    for _ in 0..2 {
        service
            .reconcile_neighbor_concierge_receiver(
                &metadata,
                "owner",
                &friend,
                false,
                9,
                service.neighbor_history_generation(),
            )
            .await
            .unwrap();
    }
    let saved = metadata.settings().await.unwrap();
    assert!(!saved.bindings.contains_key(&friend));
    assert_eq!(saved.bindings[&"c".repeat(64)], other);
    assert!(!saved.initial_contexts.contains_key(&friend));
    assert_eq!(
        saved.dormant_concierges[&friend].provider.as_deref(),
        Some("opaque-pty")
    );
    assert_eq!(
        saved.dormant_concierges[&friend]
            .options
            .reasoning_effort
            .as_deref(),
        Some("opaque-effort")
    );
    assert_eq!(serde_json::to_value(&saved.permissions).unwrap(), grants);
    assert_eq!(
        metadata.history_page(&friend, None, 50).await.unwrap(),
        before
    );
    assert!(service.identify_tab_credential(&root_token).is_none());
    assert!(service.identify_tab_credential(&child_token).is_none());
    assert!(service.identify_tab_credential(&other_token).is_some());
    assert!(service
        .require_neighbor_restore_grant(&other, false)
        .is_ok());
    let mux = service.mux.read();
    assert!(mux.get_tab(&root).is_none());
    assert!(mux.get_tab(&child).is_none());
    assert!(mux.get_tab(&other).is_some());
    let workspace = mux.find_workspace_for_tab(&other).unwrap();
    let closed = mux.list_recently_closed(&workspace);
    assert_eq!(
        closed.iter().filter(|entry| entry.tab_id == root).count(),
        1
    );
    assert_eq!(
        closed.iter().filter(|entry| entry.tab_id == child).count(),
        1
    );
}

#[tokio::test]
async fn global_stop_and_signout_retire_all_local_friends_without_touching_other_accounts() {
    for signout in [false, true] {
        let data = tempfile::tempdir().unwrap();
        let (fixture, metadata, friend, root, child, other) = fixture(data.path()).await;
        let service = &fixture.service;
        if signout {
            let session = codeterm_account::Session {
                login_generation: 0,
                identity: codeterm_account::Identity {
                    uid: "owner".into(),
                    provider: "hosted-fixture".into(),
                    display_name: None,
                    email: None,
                    photo_url: None,
                },
                id_token: codeterm_account::Secret::new("opaque"),
                expires_at: None,
            };
            service.queue_neighbor_logout(&session).await.unwrap();
        } else {
            service
                .retire_local_neighbor_concierges(
                    &metadata,
                    "owner",
                    service.neighbor_history_generation(),
                )
                .await
                .unwrap();
        }
        let saved = metadata.settings().await.unwrap();
        assert!(saved.bindings.is_empty());
        assert!(saved.dormant_concierges.contains_key(&friend));
        assert!(saved.dormant_concierges.contains_key(&"c".repeat(64)));
        for id in [&root, &child, &other] {
            assert!(service.mux.read().get_tab(id).is_none());
        }
        assert!(service.mux.read().snapshot().tabs.values().any(|tab| tab
            .neighbor_authority
            .as_ref()
            .is_some_and(|authority| authority.uid == "other-owner")));
        if signout {
            assert!(saved.cleanup_pending);
            assert!(!saved.enabled);
        }
    }
}

#[tokio::test]
async fn nonreceiver_bind_apply_create_and_ensure_refuse_before_network_or_new_tabs() {
    for receiver in [None, Some(false)] {
        let data = tempfile::tempdir().unwrap();
        let (fixture, metadata, friend, root, _, _) = fixture(data.path()).await;
        let service = &fixture.service;
        service.set_neighbor_friend_receiver_for_test(&friend, receiver, 2);
        service
            .retire_neighbor_concierge(
                &metadata,
                "owner",
                &friend,
                service.neighbor_history_generation(),
            )
            .await
            .unwrap();
        let before = service.mux.read().snapshot().tabs.len();
        let transport = FirebaseTransport {
            calls: Arc::new(NoCalls),
            project: "fixture".into(),
            uid: "owner".into(),
            installation_id: "machine".into(),
        };
        assert_eq!(
            service
                .bind_neighbor_concierge(&metadata, &transport, &friend, &root)
                .await
                .unwrap_err(),
            Error::ReceivingElsewhere
        );
        assert_eq!(
            service
                .set_neighbor_permission(&metadata, &transport, &friend, Permission::default())
                .await
                .unwrap_err(),
            Error::ReceivingElsewhere
        );
        assert_eq!(
            service
                .create_neighbor_concierge(
                    &friend,
                    None,
                    None,
                    data.path().to_string_lossy().into_owned(),
                    Permission::default(),
                    None,
                    ConciergeOptions::default()
                )
                .await
                .unwrap_err(),
            Error::ReceivingElsewhere.to_string()
        );
        assert_eq!(
            service
                .restore_neighbor_concierge_with(
                    &metadata,
                    "owner",
                    &friend,
                    |_, _, _, _, _, _| async { panic!("nonreceiver ensure created a root") }
                )
                .await
                .unwrap_err(),
            Error::ReceivingElsewhere.to_string()
        );
        assert_eq!(service.mux.read().snapshot().tabs.len(), before);
        assert!(!metadata
            .settings()
            .await
            .unwrap()
            .bindings
            .contains_key(&friend));
    }
}

#[tokio::test]
async fn cold_nonreceiver_boot_cannot_materialize_and_retirement_survives_another_restart() {
    let data = tempfile::tempdir().unwrap();
    let (original, original_metadata, friend, root, child, _) = fixture(data.path()).await;
    let settings = original_metadata.settings().await.unwrap();
    let history = original_metadata.history(&friend).await.unwrap();
    let snapshot = serde_json::to_vec(&original.service.mux.read().to_session_snapshot()).unwrap();
    drop(original);
    let snapshot = serde_json::from_slice(&snapshot).unwrap();
    let restored_data = tempfile::tempdir().unwrap();
    let restored = ServiceFixture::new_at(
        MultiplexerState::from_session_snapshot(&snapshot),
        restored_data.path(),
    );
    restored.install_seeded_storage_desk();
    let service = &restored.service;
    let metadata = Metadata::new(service.account_data_root().unwrap(), "owner");
    metadata.save_settings(settings).await.unwrap();
    metadata
        .write(format!("history-{friend}.json"), history)
        .await
        .unwrap();
    for id in [&root, &child] {
        service.mux.write().get_tab_mut(id).unwrap().frozen = true;
        let _boundary = crate::service::lazy_restore::test_boundary::Boundary::install(
            id,
            Arc::new(|| panic!("unknown/nonreceiver boot materialized a concierge")),
        );
        assert_eq!(
            service.wake_frozen_tab(id).unwrap_err(),
            Error::ReceivingElsewhere.to_string()
        );
    }
    service.set_neighbor_friend_receiver_for_test(&friend, Some(false), 9);
    service
        .reconcile_neighbor_concierge_receiver(
            &metadata,
            "owner",
            &friend,
            false,
            9,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let snapshot = service.mux.read().to_session_snapshot();
    let settings = metadata.settings().await.unwrap();
    let history = metadata.history(&friend).await.unwrap();
    drop(restored);
    let again_data = tempfile::tempdir().unwrap();
    let again = ServiceFixture::new_at(
        MultiplexerState::from_session_snapshot(&snapshot),
        again_data.path(),
    );
    again.install_seeded_storage_desk();
    let again_metadata = Metadata::new(again.service.account_data_root().unwrap(), "owner");
    again_metadata.save_settings(settings).await.unwrap();
    again_metadata
        .write(format!("history-{friend}.json"), history)
        .await
        .unwrap();
    assert!(!again
        .service
        .mux
        .read()
        .snapshot()
        .tabs
        .values()
        .any(|tab| tab
            .neighbor_authority
            .as_ref()
            .is_some_and(
                |authority| authority.uid == "owner" && authority.friendship_id == friend
            )));
    let saved = again_metadata.settings().await.unwrap();
    assert!(!saved.bindings.contains_key(&friend));
    assert!(saved.dormant_concierges.contains_key(&friend));
}

#[tokio::test]
async fn return_reset_to_lower_revision_and_equal_revision_binding_change_create_fresh_once() {
    let data = tempfile::tempdir().unwrap();
    let (fixture, metadata, friend, old, _, other) = fixture(data.path()).await;
    let service = &fixture.service;
    service.set_neighbor_friend_receiver_for_test(&friend, Some(false), 9);
    service
        .reconcile_neighbor_concierge_receiver(
            &metadata,
            "owner",
            &friend,
            false,
            9,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let first = regain(&fixture, &metadata, &friend, 2, &other).await;
    assert_ne!(first, old);
    assert_eq!(
        metadata.settings().await.unwrap().concierge_receivers[&friend].revision,
        2
    );
    assert!(metadata
        .settings()
        .await
        .unwrap()
        .dormant_concierges
        .is_empty());
    assert_eq!(regain(&fixture, &metadata, &friend, 2, &other).await, first);
    service.set_neighbor_friend_receiver_binding_for_test(
        &friend,
        "opaque-new-binding-with-same-revision",
    );
    let second = regain(&fixture, &metadata, &friend, 2, &other).await;
    assert_ne!(first, second);
    assert!(service.mux.read().get_tab(&first).is_none());
    assert_eq!(
        regain(&fixture, &metadata, &friend, 2, &other).await,
        second
    );
}

#[tokio::test]
async fn receiving_on_a_new_machine_waits_for_its_own_local_setup() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let service = &fixture.service;
    let friendship = "a".repeat(64);
    let metadata = Metadata::new(service.account_data_root().unwrap(), "owner");
    service.set_neighbor_friend_receiver_for_test(&friendship, Some(true), 1);
    let before = service.mux.read().snapshot().tabs.len();
    service
        .reconcile_neighbor_concierge_receiver(
            &metadata,
            "owner",
            &friendship,
            true,
            1,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let settings = metadata.settings().await.unwrap();
    assert!(settings.bindings.is_empty());
    assert!(settings.permissions.is_empty());
    assert!(settings.dormant_concierges.is_empty());
    assert_eq!(service.mux.read().snapshot().tabs.len(), before);
}

#[tokio::test]
async fn receiver_loss_during_replacement_cannot_publish_or_keep_a_live_concierge() {
    let data = tempfile::tempdir().unwrap();
    let (fixture, metadata, friend, old, _, other) = fixture(data.path()).await;
    let service = &fixture.service;
    let result = service
        .replace_neighbor_concierge_with(
            &metadata,
            &friend,
            false,
            ConciergeOptions::default(),
            |cwd, _| async {
                let id = {
                    let mut mux = service.mux.write();
                    let workspace = mux.find_workspace_for_tab(&other).unwrap();
                    let id = mux.create_tab(&workspace).unwrap();
                    let tab = mux.get_tab_mut(&id).unwrap();
                    tab.agent_type = Some("opaque-pty".into());
                    tab.agent_session_id = Some("opaque-new-session".into());
                    tab.session_binding_nonce = Some("new-nonce".into());
                    tab.neighbor_authority = Some(NeighborAuthority {
                        uid: "owner".into(),
                        friendship_id: friend.clone(),
                        root_tab_id: None,
                    });
                    tab.cwd = Some(cwd);
                    id
                };
                persist_desk(service).await;
                service.set_neighbor_friend_receiver_for_test(&friend, Some(false), 2);
                Ok(id)
            },
        )
        .await;
    assert_eq!(result.unwrap_err(), Error::ReceivingElsewhere.to_string());
    let settings = metadata.settings().await.unwrap();
    assert!(!settings.bindings.contains_key(&friend));
    assert!(settings.dormant_concierges.contains_key(&friend));
    assert!(service.mux.read().get_tab(&old).is_none());
    assert!(!service.mux.read().snapshot().tabs.values().any(|tab| tab
        .neighbor_authority
        .as_ref()
        .is_some_and(|authority| authority.uid == "owner" && authority.friendship_id == friend)));
    assert!(service.mux.read().get_tab(&other).is_some());
}

#[tokio::test]
async fn retirement_finishes_active_handling_without_erasing_the_incoming_message_or_replying() {
    use crate::service::neighbors::history::handling::{Owner, State};
    let data = tempfile::tempdir().unwrap();
    let (fixture, metadata, friend, root, _, _) = fixture(data.path()).await;
    let service = &fixture.service;
    service
        .mutate_neighbor_handling(
            &metadata,
            &friend,
            service.neighbor_history_generation(),
            |history| {
                history.entries[0].handling.handling_state = Some(State::Working);
                history.entries[0].handling.handling_owner = Some(Owner {
                    tab: root.clone(),
                    session: format!("old-session-{root}"),
                    nonce: "root-nonce".into(),
                    runtime: false,
                    turn: None,
                    edge: 0,
                });
                true
            },
        )
        .await
        .unwrap();
    service.set_neighbor_friend_receiver_for_test(&friend, Some(false), 2);
    service
        .reconcile_neighbor_concierge_receiver(
            &metadata,
            "owner",
            &friend,
            false,
            2,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let page = metadata.history_page(&friend, None, 50).await.unwrap();
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["body"], "opaque retained conversation");
    assert_eq!(page["entries"][0]["handling_state"], "failed");
}

#[tokio::test]
async fn signout_metadata_failures_still_retire_current_account_agents_and_credentials() {
    for fail_save in [false, true] {
        let data = tempfile::tempdir().unwrap();
        let (fixture, metadata, _, root, child, other_friend) = fixture(data.path()).await;
        let service = &fixture.service;
        let foreign = service
            .mux
            .read()
            .snapshot()
            .tabs
            .iter()
            .find_map(|(id, tab)| {
                tab.neighbor_authority
                    .as_ref()
                    .filter(|authority| authority.uid == "other-owner")
                    .map(|_| id.clone())
            })
            .unwrap();
        let root_token = service.mint_tab_credential(&root, "root-nonce").unwrap();
        let child_token = service.mint_tab_credential(&child, "child-nonce").unwrap();
        let foreign_token = service
            .mint_tab_credential(&foreign, "foreign-nonce")
            .unwrap();
        let original = serde_json::to_value(metadata.settings().await.unwrap()).unwrap();
        let session = codeterm_account::Session {
            login_generation: 0,
            identity: codeterm_account::Identity {
                uid: "owner".into(),
                provider: "hosted-fixture".into(),
                display_name: None,
                email: None,
                photo_url: None,
            },
            id_token: codeterm_account::Secret::new("opaque"),
            expires_at: None,
        };
        let error = if fail_save {
            let failing = metadata.clone().with_failed_settings_save_for_test();
            service
                .queue_neighbor_logout_with_metadata(&session, &failing)
                .await
                .unwrap_err()
        } else {
            metadata
                .write("settings.json".into(), json!({"invalid_settings":true}))
                .await
                .unwrap();
            service.queue_neighbor_logout(&session).await.unwrap_err()
        };
        assert!(error.contains(&Error::CursorStore.to_string()));
        assert!(service.identify_tab_credential(&root_token).is_none());
        assert!(service.identify_tab_credential(&child_token).is_none());
        assert!(service.identify_tab_credential(&foreign_token).is_some());
        let mux = service.mux.read();
        assert!(mux.get_tab(&foreign).is_some());
        let workspace = mux.find_workspace_for_tab(&foreign).unwrap();
        let closed = mux.list_recently_closed(&workspace);
        for id in [&root, &child, &other_friend] {
            assert!(mux.get_tab(id).is_none());
            assert_eq!(closed.iter().filter(|entry| &entry.tab_id == id).count(), 1);
        }
        drop(mux);
        if fail_save {
            assert_eq!(
                serde_json::to_value(metadata.settings().await.unwrap()).unwrap(),
                original
            );
        } else {
            assert_eq!(
                metadata
                    .read::<Value>("settings.json".into())
                    .await
                    .unwrap()
                    .unwrap(),
                json!({"invalid_settings":true})
            );
        }
    }
}
