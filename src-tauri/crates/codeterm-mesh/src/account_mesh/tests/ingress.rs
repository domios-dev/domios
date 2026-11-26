use super::support::*;
use crate::account_mesh::link::link_installation_with;
use crate::cloud_ingress::admission::{EnrolledProof, IngressRole};
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[tokio::test]
async fn enrolled_ingress_requires_both_proofs_on_the_same_live_channel() {
    let clock = clock();
    let (a, b) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&a, &b]);
    for m in [&a, &b] {
        m.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    }
    let binding = [17; 32];
    let target = b.mesh.lock().admission_context().unwrap().party;
    let proof = a
        .mesh
        .lock()
        .sign_ingress_proof(&target, IngressRole::Initiator, &binding)
        .unwrap();
    let verify = |proof: &EnrolledProof, binding: &[u8; 32], role| {
        b.mesh
            .lock()
            .verify_ingress_proof(&a.installation_id, role, binding, proof)
    };
    for field in ["mesh_signature", "installation_signature"] {
        let mut damaged = serde_json::to_value(&proof).unwrap();
        damaged[field] = "invalid".into();
        let damaged = serde_json::from_value(damaged).unwrap();
        assert!(verify(&damaged, &binding, IngressRole::Initiator).is_err());
    }
    assert!(verify(&proof, &[18; 32], IngressRole::Initiator).is_err());
    assert!(verify(&proof, &binding, IngressRole::Responder).is_err());
    let evidence = verify(&proof, &binding, IngressRole::Initiator).unwrap();
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &binding)
        .is_err());
    let network = Loopback::new(&[&a, &b]);
    link_installation_with(&a.mesh, &network, &b.installation_id)
        .await
        .unwrap();
    let evidence = verify(&proof, &binding, IngressRole::Initiator).unwrap();
    assert!(evidence.peer_for_dispatch(&b.mesh.lock(), &binding).is_ok());
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &[18; 32])
        .is_err());
    let snapshot = codeterm_entitlements::DetachedGrant {
        payload: "registry".into(),
        signature: "signature".into(),
        key_id: "issuer".into(),
        alg: "Ed25519".into(),
    };
    assert!(evidence
        .receive_registry_snapshot(&b.mesh, &[18; 32], &snapshot)
        .await
        .is_err());
    assert!(b.authority.snapshots.lock().is_empty());
    assert_eq!(
        evidence
            .receive_registry_snapshot(&b.mesh, &binding, &snapshot)
            .await,
        Ok(true)
    );
    assert_eq!(b.authority.snapshots.lock().as_slice(), &[snapshot]);
    clock.store(START + MEMBERSHIP_TTL, Ordering::SeqCst);
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &binding)
        .is_err());
    for m in [&a, &b] {
        m.sign_in(1, ACCOUNT, START + 2 * MEMBERSHIP_TTL);
    }
    let renewed = a
        .mesh
        .lock()
        .sign_ingress_proof(&target, IngressRole::Initiator, &binding)
        .unwrap();
    let token = a.mesh.lock().advertised_membership().unwrap();
    assert!(b.mesh.lock().refresh_peer_membership(&a.id(), &token));
    let evidence = verify(&renewed, &binding, IngressRole::Initiator).unwrap();
    assert!(evidence.peer_for_dispatch(&b.mesh.lock(), &binding).is_ok());
    let mut revoked = b.mesh.lock().account_state().clone();
    revoked
        .revoked_installations
        .insert(a.installation_id.clone());
    b.mesh.lock().apply_account_state(revoked);
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &binding)
        .is_err());
    b.sign_in(2, ACCOUNT, START + 2 * MEMBERSHIP_TTL);
    assert!(evidence
        .peer_for_dispatch(&b.mesh.lock(), &binding)
        .is_err());
}

#[tokio::test]
async fn reverse_stream_drains_fifo_and_never_replays_an_uncertain_receipt() {
    use crate::cloud_ingress::*;
    use crate::cloud_outbox::*;
    use tokio_tungstenite::{tungstenite::protocol::Role, WebSocketStream};
    let clock = clock();
    let (home, cloud) = (machine(1, &clock), machine(2, &clock));
    enroll_all(&[&home, &cloud]);
    for m in [&home, &cloud] {
        m.sign_in(1, ACCOUNT, START + MEMBERSHIP_TTL);
    }
    let root = std::env::temp_dir().join(format!("cloud-duplex-{}", uuid::Uuid::new_v4()));
    let (events, mut settled) = tokio::sync::mpsc::unbounded_channel();
    struct Observer(tokio::sync::mpsc::UnboundedSender<OutboxRecordStatus>);
    impl DrainLifecycleObserver for Observer {
        fn on_settled(&self, _: &OutboxId, status: &OutboxRecordStatus) {
            self.0.send(status.clone()).unwrap();
        }
    }
    struct Application(parking_lot::Mutex<Vec<String>>);
    impl ReverseDispatcher for Application {
        fn enabled(&self) -> bool {
            true
        }
        fn dispatch(
            &self,
            request: OuterWireRequest,
            _token: String,
        ) -> crate::account_mesh::BoxFuture<'_, Result<Option<Vec<u8>>, IngressError>> {
            let title = request.body["body"]["title"].as_str().unwrap().to_owned();
            self.0.lock().push(title.clone());
            Box::pin(async move {
                if title == "uncertain" {
                    Err(IngressError::Unavailable)
                } else {
                    Ok(Some(vec![1]))
                }
            })
        }
    }
    let app = Arc::new(Application(Default::default()));
    let receipts = Arc::new(
        ReceiverDedupe::open_with_system_clock(root.join("receipts"), Default::default())
            .await
            .unwrap(),
    );
    let queue = Arc::new(
        CloudOutbox::open(
            root.join("queue"),
            CloudOutboxConfig {
                observer: Arc::new(Observer(events)),
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    let enqueue = |title: &str| {
        queue.enqueue(OutboundPeerRequest {
            account_id: ACCOUNT.into(),
            source_installation_id: cloud.installation_id.clone(),
            target_installation_id: home.installation_id.clone(),
            operation: OutboundOperation::Notification,
            caller_context: CallerContext {
                origin: crate::crown::AuthenticatedOrigin::Mesh,
                agent: None,
                expires_at_ms: None,
            },
            body: serde_json::to_vec(&serde_json::json!({"title":title})).unwrap(),
        })
    };
    for title in ["first", "second", "uncertain"] {
        enqueue(title).await.unwrap();
    }
    let attach = || {
        let (a, b) = tokio::io::duplex(4096);
        let home = home.mesh.clone();
        let id = cloud.installation_id.clone();
        let cloud = cloud.mesh.clone();
        let (app, receipts, queue) = (app.clone(), receipts.clone(), queue.clone());
        let reader = tokio::spawn(async move {
            let socket = WebSocketStream::from_raw_socket(a, Role::Client, None).await;
            let channel = initiate_test_channel(home, socket, &id).await.unwrap();
            receive_test_stream(channel, &receipts, app.as_ref()).await
        });
        let writer = tokio::spawn(async move {
            let socket = WebSocketStream::from_raw_socket(b, Role::Server, None).await;
            let (channel, _) = accept_test_channel(socket, cloud).await.unwrap();
            serve_test_stream(channel, queue, Arc::new(ExpiryOnlyPolicy)).await
        });
        (reader, writer)
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (reader, writer) = attach();
        for _ in 0..2 {
            assert!(matches!(
                settled.recv().await,
                Some(OutboxRecordStatus::Delivered { .. })
            ));
        }
        assert!(matches!(
            settled.recv().await,
            Some(OutboxRecordStatus::DeliveryUnconfirmed { .. })
        ));
        assert!(reader.await.unwrap().is_err());
        assert!(writer.await.unwrap().is_err());
        enqueue("after-reconnect").await.unwrap();
        let (reader, writer) = attach();
        assert!(matches!(
            settled.recv().await,
            Some(OutboxRecordStatus::Delivered { .. })
        ));
        reader.abort();
        let _ = reader.await;
        let _ = writer.await;
        assert_eq!(
            *app.0.lock(),
            ["first", "second", "uncertain", "after-reconnect"]
        );
    })
    .await
    .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
