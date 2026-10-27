use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[tokio::test]
async fn oversized_sends_are_typed_before_the_network_operation_queue() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let (owner, mut queued) = tokio::sync::mpsc::channel(1);
    *fixture.service.neighbor_cell().owner.lock() = Some(owner);
    let command = NeighborRuntimeCommand::Send {
        friendship_id: "a".repeat(64),
        kind: Kind::Question,
        body: "x".repeat(8177),
        in_reply_to: None,
    };
    tokio::select! {
        result = fixture.service.neighbor_runtime(command, None, None) => assert_eq!(result.unwrap_err(), "message_too_large: {\"limit\":8176}"),
        _ = queued.recv() => panic!("An oversized body cannot reach network work"),
    }
}

#[tokio::test]
async fn local_reads_bypass_blocked_network_owner_and_return_metadata() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.permissions.insert(
        friendship.clone(),
        Permission {
            allowed: true,
            scope: "opaque scope".into(),
            allow_tab_requests: false,
            allow_interactive_content: true,
        },
    );
    metadata.save_settings(settings).await.unwrap();
    let (sender, mut requests) = tokio::sync::mpsc::channel(1);
    *service.neighbor_cell().owner.lock() = Some(sender);
    for command in [
        NeighborRuntimeCommand::History {
            friendship_id: friendship.clone(),
            before: None,
            limit: None,
        },
        NeighborRuntimeCommand::Permission {
            friendship_id: friendship.clone(),
        },
        NeighborRuntimeCommand::NotificationPreference {
            friendship_id: friendship.clone(),
        },
        NeighborRuntimeCommand::Status,
    ] {
        assert!(command.is_local_read());
        tokio::select! {
            biased;
            result = service.neighbor_runtime(serde_json::from_value(match &command {
                NeighborRuntimeCommand::History { .. } => json!({"operation":"history","friendship_id":friendship}),
                NeighborRuntimeCommand::Permission { .. } => json!({"operation":"permission","friendship_id":friendship}),
                NeighborRuntimeCommand::NotificationPreference { .. } => json!({"operation":"notification_preference","friendship_id":friendship}),
                _ => json!({"operation":"status"}),
            }).unwrap(), None, None) => assert_eq!(result.unwrap_err(), "Sign in with a hosted account to use Neighbors"),
            _ = requests.recv() => panic!("Local reads cannot enter the blocked network queue"),
        }
        let result = service
            .neighbor_local_metadata(&command, &metadata)
            .await
            .unwrap();
        match command {
            NeighborRuntimeCommand::History { .. } => assert_eq!(result["entries"], json!([])),
            NeighborRuntimeCommand::Permission { .. } => {
                assert_eq!(result["allow_interactive_content"], true)
            }
            NeighborRuntimeCommand::NotificationPreference { .. } => {
                assert_eq!(result["notify"], "in_app")
            }
            _ => {
                assert_eq!(
                    result["permissions"][&friendship]["allow_interactive_content"],
                    true
                );
                assert_eq!(result["receiver_status_known"], false);
            }
        }
    }
    assert!(requests.try_recv().is_err());
}
