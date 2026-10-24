use super::support::*;
use crate::account_mesh::wire::NetworkIdentity;

#[tokio::test]
async fn network_transport_identity_is_published_and_replaced_without_an_address_change() {
    let harness = Harness::new().await;
    harness.sign_in("user-1").await;
    harness
        .mesh
        .refresh(crate::account_mesh::EnrollTrigger::Automatic)
        .await
        .unwrap();
    for network_id in ["a".repeat(64), "b".repeat(64)] {
        let identity = NetworkIdentity {
            kind: "tailscale".into(),
            network_id,
        };
        *harness.endpoint.0.lock().unwrap() =
            Some(tailnet_endpoint().with_transport(Some(identity.clone())));
        harness
            .mesh
            .refresh(crate::account_mesh::EnrollTrigger::Network)
            .await
            .unwrap();
        let endpoint = harness.registry.endpoint("user-1").unwrap();
        assert_eq!(endpoint.transport, Some(identity));
        let wire = serde_json::to_string(&endpoint).unwrap();
        assert!(!wire.contains("LoginName"));
        assert!(!wire.contains("user"));
    }
}

#[test]
fn network_transport_identity_is_omitted_when_unknown() {
    let wire = serde_json::to_value(tailnet_endpoint().wire()).unwrap();
    assert!(wire.get("transport").is_none());
}
