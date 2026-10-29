use super::*;
use crate::service::neighbors::history::Author;
use std::sync::{atomic::Ordering, Arc};

fn authenticated_general(mux: &mut MultiplexerState, general: &str) {
    let tab = mux.get_tab_mut(general).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("general-session".into());
    tab.session_binding_nonce = Some("general-nonce".into());
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
    tab.role_profile = Some(crate::service::general_agent::GENERAL_AGENT_KIND.into());
}

async fn assert_preflight(
    fixture: &ServiceFixture,
    metadata: &Metadata,
    authority: SendAuthority,
    author: Author,
    kind: Kind,
    file: bool,
    expected: Error,
) {
    let calls = Arc::new(NoNetwork);
    let transport = FirebaseTransport {
        calls: calls.clone(),
        project: "fixture".into(),
        uid: "owner".into(),
        installation_id: "machine".into(),
    };
    let registry = FirebaseRegistry {
        calls,
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Fixture".into(),
        enrollment: None,
    };
    let store = KeyFence::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    let friend = "a".repeat(64);
    let before = metadata.history_page(&friend, None, 50).await.unwrap();
    let body = if file {
        OutboundBody::Files {
            text: Zeroizing::new(String::new()),
            paths: vec!["opaque-file".into()],
        }
    } else {
        OutboundBody::Granted(Zeroizing::new("opaque outgoing text".into()))
    };
    let error = fixture
        .service
        .send_neighbor_once(
            &transport,
            &lifecycle,
            metadata,
            Outgoing {
                authority,
                friendship: friend.clone(),
                kind,
                body,
                in_reply_to: None,
                author,
            },
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error, expected);
    assert_eq!(
        store.0.load(Ordering::SeqCst),
        usize::from(expected == Error::SecretStore)
    );
    assert_eq!(
        metadata.history_page(&friend, None, 50).await.unwrap(),
        before
    );
}

#[tokio::test]
async fn owner_and_authenticated_general_send_text_answers_and_files_without_inbound_grants() {
    let (mut mux, general) = MultiplexerState::new();
    authenticated_general(&mut mux, &general);
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    for denied in [false, true] {
        let mut settings = crate::service::neighbors::storage::Settings::default();
        if denied {
            settings
                .permissions
                .insert(friend.clone(), Permission::default());
        }
        metadata.save_settings(settings).await.unwrap();
        assert!(metadata.settings().await.unwrap().bindings.is_empty());
        for caller in [None, Some(general.as_str())] {
            let nonce = caller.map(|_| "general-nonce");
            for (kind, file) in [
                (Kind::Question, false),
                (Kind::Answer, false),
                (Kind::Question, true),
                (Kind::Answer, true),
            ] {
                let command = NeighborRuntimeCommand::Send {
                    friendship_id: friend.clone(),
                    kind,
                    body: "opaque".into(),
                    in_reply_to: None,
                };
                fixture
                    .service
                    .authorize_neighbor_metadata(&command, caller, nonce, &metadata)
                    .await
                    .unwrap();
                let authority = fixture
                    .service
                    .neighbor_send_authority(&command, caller, nonce)
                    .unwrap();
                assert_eq!(authority, SendAuthority::Unrestricted);
                let author = fixture.service.neighbor_author(caller).unwrap();
                if caller.is_some() {
                    assert!(matches!(&author, Author::Agent { .. }));
                }
                assert_preflight(
                    &fixture,
                    &metadata,
                    authority,
                    author,
                    kind,
                    file,
                    Error::SecretStore,
                )
                .await;
            }
            let command = NeighborRuntimeCommand::Send {
                friendship_id: friend.clone(),
                kind: Kind::TabRequest,
                body: "opaque".into(),
                in_reply_to: None,
            };
            let authority = fixture
                .service
                .neighbor_send_authority(&command, caller, nonce)
                .unwrap();
            assert_preflight(
                &fixture,
                &metadata,
                authority,
                fixture.service.neighbor_author(caller).unwrap(),
                Kind::TabRequest,
                false,
                Error::TabRequestsNotAllowed,
            )
            .await;
        }
        let command = NeighborRuntimeCommand::Send {
            friendship_id: friend.clone(),
            kind: Kind::Question,
            body: "opaque".into(),
            in_reply_to: None,
        };
        assert!(fixture
            .service
            .neighbor_send_authority(&command, Some(&general), Some("stale-nonce"))
            .is_err());
    }
}

#[tokio::test]
async fn bound_concierge_and_descendant_sends_stay_gated_by_the_exact_friend_grant() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let child = mux.create_tab(&workspace).unwrap();
    let friend = "a".repeat(64);
    for (id, root) in [(&concierge, None), (&child, Some(concierge.clone()))] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        tab.neighbor_authority = Some(crate::multiplexer::types::NeighborAuthority {
            uid: "owner".into(),
            friendship_id: friend.clone(),
            root_tab_id: root,
        });
    }
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&friend, Some(true), 1);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    for grant in [None, Some(false), Some(true)] {
        let mut settings = crate::service::neighbors::storage::Settings::default();
        settings.bindings.insert(friend.clone(), concierge.clone());
        if let Some(allowed) = grant {
            settings.permissions.insert(
                friend.clone(),
                Permission {
                    allowed,
                    ..Default::default()
                },
            );
        }
        metadata.save_settings(settings).await.unwrap();
        for caller in [&concierge, &child] {
            let file_error = fixture
                .service
                .neighbor_files(
                    json!({"operation":"send","friendship_id":friend,
                "kind":"question","body":"","paths":["opaque-file"]}),
                    Some(caller),
                    Some(&format!("nonce-{caller}")),
                )
                .await
                .unwrap_err();
            assert!(file_error.starts_with("neighbor_forbidden:"));
            for (kind, file) in [
                (Kind::Question, false),
                (Kind::Answer, false),
                (Kind::Question, true),
                (Kind::Answer, true),
            ] {
                let command = NeighborRuntimeCommand::Send {
                    friendship_id: friend.clone(),
                    kind,
                    body: "opaque".into(),
                    in_reply_to: None,
                };
                let nonce = format!("nonce-{caller}");
                fixture
                    .service
                    .authorize_neighbor_metadata(&command, Some(caller), Some(&nonce), &metadata)
                    .await
                    .unwrap();
                let authority = fixture
                    .service
                    .neighbor_send_authority(&command, Some(caller), Some(&nonce))
                    .unwrap();
                assert_eq!(authority, SendAuthority::FriendshipGrant);
                assert_preflight(
                    &fixture,
                    &metadata,
                    authority,
                    fixture.service.neighbor_author(Some(caller)).unwrap(),
                    kind,
                    file,
                    if grant == Some(true) {
                        Error::SecretStore
                    } else {
                        Error::PermissionDenied
                    },
                )
                .await;
            }
            let foreign = NeighborRuntimeCommand::Send {
                friendship_id: "b".repeat(64),
                kind: Kind::Question,
                body: "opaque".into(),
                in_reply_to: None,
            };
            assert!(fixture
                .service
                .authorize_neighbor_metadata(
                    &foreign,
                    Some(caller),
                    Some(&format!("nonce-{caller}")),
                    &metadata
                )
                .await
                .is_err());
        }
    }
}
