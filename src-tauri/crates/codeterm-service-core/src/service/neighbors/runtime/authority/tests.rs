use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[tokio::test]
async fn paused_concierge_cannot_send_or_unpause_but_owner_and_general_can_still_write() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    for id in [&general, &concierge] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    let tab = mux.get_tab_mut(&general).unwrap();
    tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
    tab.role_profile = Some(crate::service::general_agent::GENERAL_AGENT_KIND.into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let friend = "a".repeat(64);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.bindings.insert(friend.clone(), concierge.clone());
    settings.paused.insert(friend.clone());
    metadata.save_settings(settings).await.unwrap();
    for command in [
        NeighborRuntimeCommand::Send {
            friendship_id: friend.clone(),
            kind: Kind::Question,
            body: "opaque".into(),
            in_reply_to: None,
        },
        NeighborRuntimeCommand::ResumeFriend {
            friendship_id: friend,
        },
    ] {
        assert!(service
            .authorize_neighbor_metadata(&command, None, None, &metadata)
            .await
            .is_ok());
        assert!(service
            .authorize_neighbor_metadata(
                &command,
                Some(&general),
                Some(&format!("nonce-{general}")),
                &metadata
            )
            .await
            .is_ok());
        assert!(service
            .authorize_neighbor_metadata(
                &command,
                Some(&concierge),
                Some(&format!("nonce-{concierge}")),
                &metadata
            )
            .await
            .is_err());
    }
}

#[tokio::test]
async fn runtime_authority_is_owner_general_or_the_exact_authenticated_bound_concierge() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let a = mux.create_tab(&workspace).unwrap();
    let b = mux.create_tab(&workspace).unwrap();
    let unrelated = mux.create_tab(&workspace).unwrap();
    for id in [&general, &a, &b, &unrelated] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    mux.get_tab_mut(&general).unwrap().role = Some(crate::multiplexer::types::AgentRole::Manager);
    mux.get_tab_mut(&general).unwrap().role_profile =
        Some(crate::service::general_agent::GENERAL_AGENT_KIND.into());
    mux.get_tab_mut(&unrelated).unwrap().role = Some(crate::multiplexer::types::AgentRole::Worker);
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend_a = "a".repeat(64);
    let friend_b = "b".repeat(64);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.bindings.insert(friend_a.clone(), a.clone());
    settings.bindings.insert(friend_b.clone(), b.clone());
    metadata.save_settings(settings).await.unwrap();
    for (friendship, message) in [(&friend_a, "1".repeat(32)), (&friend_b, "2".repeat(32))] {
        let entry = serde_json::from_value(json!({"direction":"out","author":"me","sender_uid":"owner","body":"opaque","at":1,"message_id":message,"kind":"question","delivery_status":"pending"})).unwrap();
        service
            .append_neighbor_history(
                &metadata,
                friendship,
                entry,
                service.neighbor_history_generation(),
            )
            .await
            .unwrap();
        for command in [
            NeighborRuntimeCommand::History {
                friendship_id: friendship.clone(),
                before: None,
                limit: None,
            },
            NeighborRuntimeCommand::Send {
                friendship_id: friendship.clone(),
                kind: Kind::Question,
                body: "opaque".into(),
                in_reply_to: None,
            },
            NeighborRuntimeCommand::Retry {
                friendship_id: friendship.clone(),
            },
            NeighborRuntimeCommand::DeliveryStatus {
                message_id: message,
            },
        ] {
            for caller in [None, Some(&general), Some(&a), Some(&b), Some(&unrelated)] {
                let nonce = caller.map(|id| format!("nonce-{id}"));
                let result = service
                    .authorize_neighbor_metadata(
                        &command,
                        caller.map(String::as_str),
                        nonce.as_deref(),
                        &metadata,
                    )
                    .await;
                let allowed = caller.is_none()
                    || caller == Some(&general)
                    || (friendship == &friend_a && caller == Some(&a))
                    || (friendship == &friend_b && caller == Some(&b));
                assert_eq!(result.is_ok(), allowed);
            }
        }
    }
    for command in [
        NeighborRuntimeCommand::Status,
        NeighborRuntimeCommand::PauseFriend {
            friendship_id: friend_a.clone(),
        },
        NeighborRuntimeCommand::ResumeFriend {
            friendship_id: friend_a.clone(),
        },
        NeighborRuntimeCommand::OwnKeys,
        NeighborRuntimeCommand::Permission {
            friendship_id: friend_a.clone(),
        },
    ] {
        assert!(service
            .neighbor_runtime_access(&command, None, None)
            .is_ok());
        assert!(service
            .neighbor_runtime_access(&command, Some(&general), Some(&format!("nonce-{general}")))
            .is_ok());
        for caller in [&a, &b, &unrelated] {
            assert!(service
                .neighbor_runtime_access(&command, Some(caller), Some(&format!("nonce-{caller}")))
                .is_err());
        }
    }
    for command in [
        NeighborRuntimeCommand::Enroll {
            takeover_confirmed: true,
            expected_receiver_revision: 1,
        },
        NeighborRuntimeCommand::Rotate,
        NeighborRuntimeCommand::Revoke,
    ] {
        assert!(service
            .neighbor_runtime_access(&command, None, None)
            .is_ok());
        for caller in [&general, &a, &b, &unrelated] {
            assert!(service
                .neighbor_runtime_access(&command, Some(caller), Some(&format!("nonce-{caller}")))
                .is_err());
        }
    }
    let command = NeighborRuntimeCommand::History {
        friendship_id: friend_a,
        before: None,
        limit: None,
    };
    assert!(service
        .authorize_neighbor_metadata(&command, Some(&a), Some("wrong"), &metadata)
        .await
        .is_err());
    assert!(service
        .authorize_neighbor_metadata(
            &NeighborRuntimeCommand::DeliveryStatus {
                message_id: "3".repeat(32)
            },
            Some(&a),
            Some(&format!("nonce-{a}")),
            &metadata
        )
        .await
        .is_err());
    let mut settings = metadata.settings().await.unwrap();
    settings.bindings.clear();
    metadata.save_settings(settings).await.unwrap();
    assert!(service
        .authorize_neighbor_metadata(&command, Some(&a), Some(&format!("nonce-{a}")), &metadata)
        .await
        .is_err());
}
