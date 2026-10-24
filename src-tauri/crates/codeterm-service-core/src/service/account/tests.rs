use super::*;
use codeterm_account::IdentityObserver;

#[tokio::test]
async fn account_identity_changes_reach_the_frontend_event_stream() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = Arc::new(fixture.service.clone());
    let observer = AccountIdentity(Arc::downgrade(&service));
    let mut events = service.events.subscribe();
    for identity in [
        Some(codeterm_account::Identity {
            uid: "owner".into(),
            display_name: None,
            email: None,
            provider: "test".into(),
            photo_url: None,
        }),
        None,
    ] {
        observer.changed(identity.as_ref()).await;
        let event = events
            .try_recv()
            .expect("identity changes must reach SSE clients");
        assert!(matches!(
            event.event,
            codeterm_events::SystemEvent::Generic { event_type, data }
                if event_type == "account-changed" && data.is_null()
        ));
    }
}
