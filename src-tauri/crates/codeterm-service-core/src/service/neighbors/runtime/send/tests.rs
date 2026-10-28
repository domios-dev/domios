use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[tokio::test]
async fn refusal_preflight_accepts_cyrillic_and_rejects_oversize_without_writing() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let target = "b".repeat(32);
    let incoming = serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque incoming","at":1,"message_id":target,"kind":"question"})).unwrap();
    fixture
        .service
        .append_neighbor_history(
            &metadata,
            &friendship,
            incoming,
            fixture.service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let before = metadata.history_page(&friendship, None, 50).await.unwrap();
    let permission = Permission::default();
    let cyrillic = "\u{044f}".repeat(300);
    assert_eq!(cyrillic.len(), 600);
    let (accepted, paths) = prepare_outgoing_body(
        &metadata,
        &friendship,
        &permission,
        Kind::Refusal,
        OutboundBody::Granted(Zeroizing::new(cyrillic.clone())),
        Some(&target),
    )
    .await
    .unwrap();
    assert_eq!(accepted.as_str(), cyrillic);
    assert!(paths.is_empty());
    for size in [1024, 1025] {
        let result = prepare_outgoing_body(
            &metadata,
            &friendship,
            &permission,
            Kind::Refusal,
            OutboundBody::Granted(Zeroizing::new("x".repeat(size))),
            Some(&target),
        )
        .await;
        if size == 1024 {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result.unwrap_err(),
                Error::RefusalTooLong {
                    limit: 1024,
                    actual: 1025
                }
            );
        }
    }
    for target in [None, Some("cccccccccccccccccccccccccccccccc")] {
        assert_eq!(
            prepare_outgoing_body(
                &metadata,
                &friendship,
                &permission,
                Kind::Refusal,
                OutboundBody::Granted(Zeroizing::new("opaque refusal".into())),
                target
            )
            .await
            .unwrap_err(),
            Error::ReplyTargetUnknown
        );
    }
    assert_eq!(
        metadata.history_page(&friendship, None, 50).await.unwrap(),
        before
    );
}
