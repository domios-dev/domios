use super::*;

struct NoCalls;
#[async_trait::async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NoCalls {
    async fn invoke(&self, _: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        panic!("No callable is needed while startup is cancelled");
    }
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("No authentication is needed while startup is cancelled");
    }
}

#[tokio::test]
async fn off_cancels_watch_start_even_while_an_earlier_connection_holds_the_gate() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let held = service.neighbor_cell().control_start.lock().await;
    let child = service.clone();
    let (started, entered) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        started.send(()).unwrap();
        child
            .watch_neighbor_receiver(
                Arc::new(FirebaseTransport {
                    calls: Arc::new(NoCalls),
                    uid: "owner".into(),
                    project: "invalid/project".into(),
                    installation_id: "machine".into(),
                }),
                child.neighbor_history_generation(),
            )
            .await
    });
    entered.await.unwrap();
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    service.pause_neighbor_reception();
    assert_eq!(
        task.await.unwrap().unwrap_err(),
        Error::Disabled.to_string()
    );
    drop(held);
}

#[tokio::test]
async fn receiver_fence_requires_current_enabled_installation_and_revision() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    assert_eq!(
        service.require_neighbor_receiver(None),
        Err(Error::NotReceiver)
    );
    let mut status = ReceiverStatus {
        receiver_revision: 8,
        receiver_installation_id: Some("machine".into()),
        receiver_key_id: None,
        receiver_machine_label: Some("Desk".into()),
        receiving_enabled: true,
        is_me: true,
        legacy_active: false,
    };
    for (is_me, enabled, revision, allowed) in [
        (true, true, 8, true),
        (false, true, 8, false),
        (true, false, 8, false),
        (true, true, 7, false),
    ] {
        status.is_me = is_me;
        status.receiving_enabled = enabled;
        *service.neighbor_cell().receiver.lock() = Some(status.clone());
        assert_eq!(
            service.require_neighbor_receiver(Some(revision)).is_ok(),
            allowed
        );
    }
    assert_eq!(
        ServiceContext::receiver_view(&status),
        json!({"receiving_here":true,"receiver_machine_label":"Desk","receiving_enabled":true,"revision":8})
    );
    assert!(serde_json::from_value::<NeighborRuntimeCommand>(
        json!({"operation":"enroll","takeover_confirmed":true})
    )
    .is_err());
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
    assert!(observed
        .lock()
        .iter()
        .all(|event| event["status"] == "reconnecting" && !event["error"].is_null()));
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
