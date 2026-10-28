use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use async_trait::async_trait;

#[test]
fn registry_cache_retains_explicit_receiver_authority_including_malformed_projection() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let friendship = "a".repeat(64);
    let projection = json!({"source":"friend","installation_id":"elsewhere","revision":9});
    fixture.service.remember_neighbor_registry(&json!({"access":"paid","friends":[{"friendship_id":friendship,"my_receiver":projection,"peer_receiver":{"enc_key_id":"opaque-key","revision":2,"receiving":true}}]}));
    let cached = fixture.service.neighbor_cell().registry_snapshot.lock().clone().unwrap();
    assert_eq!(cached["friends"][0]["my_receiver"],projection);
    assert_eq!(cached["friends"][0]["peer_receiver"]["enc_key_id"],"opaque-key");
}

struct NoCalls;
#[async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NoCalls {
    async fn invoke(&self, _: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        panic!("A current receiver snapshot must not refetch the callable");
    }
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("No network authentication needed");
    }
}

#[tokio::test]
async fn receiver_and_registry_snapshots_are_reused_and_invalidated_on_freshness_loss() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let status = codeterm_neighbors::adapter::ReceiverStatus {
        receiver_revision: 7,
        receiver_installation_id: Some("machine".into()),
        receiver_key_id: Some("vk_fixture".into()),
        receiver_machine_label: Some("Here".into()),
        receiving_enabled: true,
        is_me: true,
        legacy_active: false,
    };
    *service.neighbor_cell().receiver.lock() = Some(status.clone());
    let registry = FirebaseRegistry {
        calls: Arc::new(NoCalls),
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Here".into(),
        enrollment: None,
    };
    assert_eq!(
        service
            .refresh_neighbor_receiver(
                &registry,
                &KeyScope::installation("machine", "owner").unwrap(),
                service.neighbor_history_generation()
            )
            .await
            .unwrap(),
        status
    );
    service.remember_neighbor_registry(
        &json!({"access":"paid","friends":[{"friendship_id":"a".repeat(64),"permission":{"scope":"opaque"}},{"friendship_id":"b".repeat(64)}]}),
    );
    assert!(service.neighbor_registry_receiving(&"a".repeat(64)));
    assert!(service.neighbor_registry_receiving(&"b".repeat(64)));
    assert!(service
        .neighbor_cell()
        .registry_snapshot
        .lock()
        .as_ref()
        .unwrap()["friends"][0]
        .get("permission")
        .is_none());
    for task in service.stop_neighbor_friendship_listeners(&"a".repeat(64)) {
        task.abort();
    }
    assert!(!service.neighbor_registry_receiving(&"a".repeat(64)));
    assert!(service.neighbor_registry_receiving(&"b".repeat(64)));
    service.remember_neighbor_registry(
        &json!({"access":"locked","friends":[{"friendship_id":"a".repeat(64)}]}),
    );
    assert!(!service.neighbor_registry_receiving(&"a".repeat(64)));
    service.pause_neighbor_reception();
    assert!(service.neighbor_cell().receiver.lock().is_none());
    assert!(service.neighbor_cell().registry_snapshot.lock().is_none());
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    service.remember_neighbor_registry(&json!({"access":"paid","friends":[]}));
    assert!(service.neighbor_cell().registry_snapshot.lock().is_none());
}
