use super::*;
use crate::account_mesh::{link::link_installation_with, tests::support::*, BoxFuture};
use crate::cloud_ingress::{accept_test_channel, initiate_test_channel};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::DuplexStream;
use tokio_tungstenite::tungstenite::protocol::Role;

async fn sockets() -> (WebSocketStream<DuplexStream>, WebSocketStream<DuplexStream>) {
    let (a, b) = tokio::io::duplex(8192);
    tokio::join!(
        WebSocketStream::from_raw_socket(a, Role::Client, None),
        WebSocketStream::from_raw_socket(b, Role::Server, None)
    )
}

struct Receiver<'a> {
    cloud: &'a Machine,
    grants: (DetachedGrant, DetachedGrant),
    calls: AtomicUsize,
}
impl RenewalReceiver for Receiver<'_> {
    fn enrolled(&self) -> bool {
        true
    }
    fn apply<'a>(
        &'a self,
        member: &'a DetachedGrant,
        registry: &'a DetachedGrant,
    ) -> BoxFuture<'a, Result<(), IngressError>> {
        Box::pin(async move {
            assert_eq!((&self.grants.0, &self.grants.1), (member, registry));
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.cloud.sign_in(1, ACCOUNT, START + 3 * MEMBERSHIP_TTL);
            Ok(())
        })
    }
}

#[tokio::test]
async fn sleeping_past_expiry_recovers_one_renewal_before_fresh_admission() {
    let clock = clock();
    let (home, cloud) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&home, &cloud]);
    home.sign_in(1, ACCOUNT, START + 4 * MEMBERSHIP_TTL);
    cloud.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    link_installation_with(
        &home.mesh,
        &Loopback::new(&[&home, &cloud]),
        &cloud.installation_id,
    )
    .await
    .unwrap();
    cloud.mesh.lock().inbound_only = true;
    clock.store(START + 2 * MEMBERSHIP_TTL, Ordering::SeqCst);
    let normal = || async {
        let (client, server) = sockets().await;
        tokio::join!(
            initiate_test_channel(home.mesh.clone(), client, &cloud.installation_id),
            accept_test_channel(server, cloud.mesh.clone())
        )
    };
    let (client, server) = normal().await;
    assert!(client.is_err() && server.is_err());
    let make_grant = |payload: &str| DetachedGrant {
        payload: payload.into(),
        signature: "test".into(),
        key_id: "issuer".into(),
        alg: "Ed25519".into(),
    };
    let grants = (make_grant("renewed"), make_grant("registry"));
    *home.authority.renewal.lock() = Some(grants.clone());
    let receiver = Receiver {
        cloud: &cloud,
        grants,
        calls: AtomicUsize::new(0),
    };
    let (recovery, _, old) =
        Recovery::pinned(&home.mesh.lock(), &cloud.installation_id, true).unwrap();
    let binding = [7; 32];
    for installation in [false, true] {
        let mut forged = recovery.proof(&home.mesh.lock(), true, &binding).unwrap();
        if installation {
            forged.installation.clear();
        } else {
            forged.mesh.clear();
        }
        assert!(recovery
            .verify(
                &recovery.home,
                transcript::INITIATOR_ROLE,
                &binding,
                &forged
            )
            .is_err());
    }
    let state = cloud.mesh.lock().account_state().clone();
    let mut revoked = state.clone();
    revoked
        .revoked_installations
        .insert(home.installation_id.clone());
    cloud.mesh.lock().apply_account_state(revoked);
    assert!(Recovery::pinned(&cloud.mesh.lock(), &home.installation_id, false).is_err());
    let mut changed = state.clone();
    changed
        .registry_members
        .as_mut()
        .unwrap()
        .get_mut(&cloud.installation_id)
        .unwrap()
        .2 += 1;
    cloud.mesh.lock().apply_account_state(changed);
    assert!(Recovery::pinned(&cloud.mesh.lock(), &home.installation_id, false).is_err());
    cloud.mesh.lock().apply_account_state(state);
    let expired_home = Operation::Renewal {
        home_membership: membership_token(ACCOUNT, &home.installation_id, START + MEMBERSHIP_TTL),
        membership: receiver.grants.0.clone(),
        signed_registry: receiver.grants.1.clone(),
    };
    for forbidden in [
        serde_json::json!({"relay":{}}),
        serde_json::json!("endpoint"),
        serde_json::json!("attach"),
        serde_json::json!({"registry":{"signed_registry":receiver.grants.1}}),
        serde_json::to_value(expired_home).unwrap(),
    ] {
        let (client, server) = sockets().await;
        let send_forbidden = async {
            let mut stream = EncryptedStream::handshake_with_domain(client, true, DOMAIN)
                .await
                .unwrap();
            let proof = recovery
                .proof(&home.mesh.lock(), true, &stream.binding)
                .unwrap();
            stream
                .write(&Hello {
                    home: home.installation_id.clone(),
                    proof,
                })
                .await
                .unwrap();
            let proof = stream.read().await.unwrap();
            recovery
                .verify(
                    &recovery.cloud,
                    transcript::RESPONDER_ROLE,
                    &stream.binding,
                    &proof,
                )
                .unwrap();
            stream.write(&forbidden).await.unwrap();
            assert!(stream.read::<()>().await.is_err());
        };
        let (_, result) = tokio::join!(
            send_forbidden,
            respond(server, &cloud.mesh, &receiver, || true)
        );
        assert!(result.is_err());
    }
    assert_eq!(receiver.calls.load(Ordering::SeqCst), 0);
    let (client, server) = sockets().await;
    let (_, closed) = tokio::join!(
        async {
            let mut stream = EncryptedStream::handshake_with_domain(client, true, DOMAIN)
                .await
                .unwrap();
            initiate(&mut stream, &home.mesh, recovery, &old.verified)
                .await
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_secs(5), stream.read::<()>())
                    .await
                    .unwrap()
                    .is_err()
            );
        },
        respond(server, &cloud.mesh, &receiver, || true)
    );
    closed.unwrap();
    assert_eq!(receiver.calls.load(Ordering::SeqCst), 1);
    assert!(Recovery::pinned(&cloud.mesh.lock(), &home.installation_id, false).is_err());
    let (client, server) = normal().await;
    assert_eq!(
        client.unwrap().peer_membership().unwrap().expires_at,
        START + 3 * MEMBERSHIP_TTL
    );
    assert!(server.is_ok());
}
