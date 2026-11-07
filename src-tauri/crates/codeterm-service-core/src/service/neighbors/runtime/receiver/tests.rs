use super::*;

#[tokio::test]
async fn friend_receiver_fence_requires_current_enabled_installation_and_revision() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    assert_eq!(
        service.require_neighbor_friend_receiver("friend", None),
        Err(Error::NotReceiver)
    );
    let mut status = ReceiverStatus {
        receiver_revision: 8,
        receiver_installation_id: Some("machine".into()),
        receiver_encryption_key_id: None,
        receiver_machine_label: Some("Desk".into()),
        receiving_enabled: true,
        is_me: true,
    };
    for (is_me, enabled, revision, allowed) in [
        (true, true, 8, true),
        (false, true, 8, false),
        (true, false, 8, false),
        (true, true, 7, false),
    ] {
        status.is_me = is_me;
        status.receiving_enabled = enabled;
        let generation = service.neighbor_cell().generation.load(Ordering::Acquire);
        service
            .remember_neighbor_friend_receiver(
                "friend",
                friends::FriendReceiver::from_status(&status),
                generation,
            )
            .unwrap();
        assert_eq!(
            service
                .require_neighbor_friend_receiver("friend", Some(revision))
                .is_ok(),
            allowed
        );
    }
    assert_eq!(
        ServiceContext::receiver_view(&status, &["machine".to_string()].into()),
        json!({"receiving_here":true,"receiver_machine_label":"Desk","receiving_enabled":true,"revision":8,"is_me":true})
    );
    assert!(serde_json::from_value::<NeighborRuntimeCommand>(
        json!({"operation":"enroll","takeover_confirmed":true})
    )
    .is_err());
}

#[test]
fn an_installation_with_expired_membership_is_never_presented_as_the_receiver() {
    let row = |id: &str, expires: Option<u64>, active: bool, signed_out: bool| {
        codeterm_mesh::types::AccountInstallation {
            installation_id: id.into(),
            name: id.into(),
            active,
            signed_out,
            membership_expires_at: expires,
            endpoint_expires_at: None,
            instance_id: None,
            online: false,
        }
    };
    let lapsed = lapsed_installations(
        &[
            row("expired", Some(100), true, false),
            row("boundary", Some(200), true, false),
            row("current", Some(201), true, false),
            row("unknown", None, true, false),
            row("removed", Some(100), false, false),
            row("signed-out", Some(100), true, true),
        ],
        200,
    );
    assert_eq!(
        lapsed,
        ["boundary".to_string(), "expired".to_string()].into()
    );
    let elsewhere = |installation: &str| ReceiverStatus {
        receiver_revision: 4,
        receiver_installation_id: Some(installation.into()),
        receiver_encryption_key_id: Some("key".into()),
        receiver_machine_label: Some("Other desk".into()),
        receiving_enabled: true,
        is_me: false,
    };
    assert_eq!(
        ServiceContext::receiver_view(&elsewhere("expired"), &lapsed),
        json!({"receiving_here":false,"receiver_machine_label":"Other desk","receiving_enabled":false,"revision":4,"is_me":false,"not_receiving_reason":"membership_expired"})
    );
    assert_eq!(
        ServiceContext::receiver_view(&elsewhere("current"), &lapsed),
        json!({"receiving_here":false,"receiver_machine_label":"Other desk","receiving_enabled":true,"revision":4,"is_me":false})
    );
}

#[tokio::test]
async fn failed_receiver_watch_keeps_reconnecting_cause_and_visible_exhaustion() {
    use codeterm_neighbors::supervisor::ListenerState;
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let friend = "a".repeat(64);
    service
        .neighbor_cell()
        .states
        .lock()
        .insert(friend.clone(), ListenerState::Receiving);
    let (shutdown, mut stopped) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move {
        let _ = stopped.changed().await;
    });
    service.neighbor_cell().listeners.lock().active.insert(
        friend.clone(),
        listener::Handle {
            gateway_tab: None,
            task,
            retry: Arc::new(tokio::sync::Notify::new()),
            shutdown,
        },
    );
    let observed = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = observed.clone();
    fixture.ui.listen("neighbor-runtime-status", move |value| {
        captured.lock().push(value.clone())
    });
    service.neighbor_watch_failure(
        "control",
        Error::Firestore {
            code: 13,
            operation: "control.listen.read",
        },
    );
    let retired = service
        .neighbor_cell()
        .listeners
        .lock()
        .retired
        .remove(&friend)
        .unwrap();
    retired.await.unwrap();
    assert_eq!(
        service.neighbor_cell().states.lock()[&friend],
        ListenerState::Reconnecting
    );
    assert_eq!(service.neighbor_cell().failures.lock()[&friend]["code"], 13);
    {
        let events = observed.lock();
        assert_eq!(
            events[0],
            json!({"receiver_unavailable":true,"friend_receivers_version":1})
        );
        assert!(!events[1..].is_empty());
        assert!(events[1..]
            .iter()
            .all(|event| event["status"] == "reconnecting" && !event["error"].is_null()));
    }
    service.neighbor_watch_failure(
        "control",
        Error::ListenProtocol {
            kind: "listen_retries_exhausted",
        },
    );
    service.neighbor_watch_state("control", ListenerState::Stopped, None);
    let watches = service.neighbor_cell().watch_states.lock();
    assert_eq!(watches["control"]["status"], "stopped");
    assert_eq!(
        watches["control"]["error"]["kind"],
        "listen_retries_exhausted"
    );
    assert_eq!(watches["control"]["error"]["last_error"]["code"], 13);
    drop(watches);
    assert_eq!(
        service.neighbor_cell().failures.lock()[&friend]["kind"],
        "listen_retries_exhausted"
    );
}
