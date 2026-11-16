use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

fn snapshot(version: u64, present: bool) -> Snapshot {
    Snapshot::parse(json!({
        "schema_version":1,"version":version,"updated_at":100,
        "access":"paid","can_invite":false,"friends":[],"incoming":[],"outgoing":[],
        "own_keys":if present { json!([{"verifier_key_id":"vk_confirmed",
            "enc_key_id":null,"installation_id":"machine","machine_label":"Fixture","created_at":100}]) } else { json!([]) },
        "referral_trial":{"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}
    })).unwrap()
}

async fn fixture() -> crate::service::test_fixture::ServiceFixture {
    let fixture =
        crate::test_support::fixture(crate::multiplexer::state::MultiplexerState::new().0);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    fixture
        .service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    assert!(
        fixture
            .service
            .apply_neighbor_projection("owner", 1, snapshot(2, true))
            .await
    );
    fixture
}

#[tokio::test]
async fn same_version_reconnect_restores_unavailable_inventory_and_dirty_projection() {
    let f = fixture().await;
    let service = &f.service;
    service.neighbor_projection_unavailable();
    service
        .fence_neighbor_key_inventory("owner", 1, "vk_confirmed")
        .unwrap();
    assert!(service.neighbor_key_inventory_pending("owner", 1));
    assert!(
        !service
            .apply_neighbor_projection("owner", 1, snapshot(1, true))
            .await
    );
    assert!(service.neighbor_key_inventory_pending("owner", 1));
    assert!(
        service
            .apply_neighbor_projection("owner", 1, snapshot(2, true))
            .await
    );
    assert!(!service.neighbor_key_inventory_pending("owner", 1));
    let state = service.neighbor_projection_diagnostics().await;
    assert_eq!(state["available"], true);
    assert_eq!(state["authoritative"], true);
    assert_eq!(state["dirty"], false);
    assert_eq!(state["snapshot_version"], 2);
    assert!(service.neighbor_activation_ticket("owner", 1).is_ok());
}

#[tokio::test]
async fn confirmed_idempotent_acknowledgment_does_not_invalidate_the_list() {
    let f = fixture().await;
    f.service
        .fence_neighbor_key_inventory("owner", 1, "vk_confirmed")
        .unwrap();
    assert!(!f.service.neighbor_key_inventory_pending("owner", 1));
    assert_eq!(
        f.service.neighbor_projection_diagnostics().await["dirty"],
        false
    );
    assert!(f.service.neighbor_activation_ticket("owner", 1).is_ok());
    f.service.invalidate_neighbor_list().await.unwrap();
    f.service
        .fence_neighbor_key_inventory("owner", 1, "vk_confirmed")
        .unwrap();
    assert_eq!(
        f.service.neighbor_projection_diagnostics().await["dirty"],
        true
    );
}

#[tokio::test]
async fn fresh_inventory_confirms_only_exact_pending_ids_and_bounds_repeated_reads() {
    let f = fixture().await;
    let service = &f.service;
    service.neighbor_projection_unavailable();
    for key in ["vk_confirmed", "vk_absent"] {
        service
            .fence_neighbor_key_inventory("owner", 1, key)
            .unwrap();
    }
    let calls = AtomicUsize::new(0);
    for _ in 0..3 {
        assert_eq!(
            service
                .recover_neighbor_inventory_with("owner", 1, async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(snapshot(2, true))
                })
                .await,
            Err(Error::KeyInventoryPending)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        service.neighbor_projection_diagnostics().await["pending_verifier_ids"],
        json!(["vk_absent"])
    );
}

#[tokio::test]
async fn concurrent_reads_share_one_fresh_inventory_recovery() {
    let f = fixture().await;
    let service = &f.service;
    service.neighbor_projection_unavailable();
    service
        .fence_neighbor_key_inventory("owner", 1, "vk_confirmed")
        .unwrap();
    let calls = AtomicUsize::new(0);
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let first = service.recover_neighbor_inventory_with("owner", 1, async {
        calls.fetch_add(1, Ordering::SeqCst);
        started.send(()).unwrap();
        held.await.unwrap();
        Ok(snapshot(2, true))
    });
    let second = service.recover_neighbor_inventory_with("owner", 1, async {
        calls.fetch_add(1, Ordering::SeqCst);
        panic!("Concurrent recovery must share the first snapshot")
    });
    let control = async {
        ready.await.unwrap();
        release.send(()).unwrap();
    };
    let (first, second, ()) = tokio::join!(first, second, control);
    assert_eq!(first, Ok(()));
    assert_eq!(second, Ok(()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!service.neighbor_key_inventory_pending("owner", 1));
}

#[tokio::test]
async fn account_switch_cannot_clear_another_sessions_inventory_fence() {
    let f = fixture().await;
    let service = &f.service;
    service
        .fence_neighbor_key_inventory("owner", 1, "vk_absent")
        .unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let recovery = service.recover_neighbor_inventory_with("owner", 1, async {
        started.send(()).unwrap();
        held.await.unwrap();
        Ok(snapshot(3, true))
    });
    let switch = async {
        ready.await.unwrap();
        service
            .select_neighbor_projection("other", 2)
            .await
            .unwrap();
        service
            .fence_neighbor_key_inventory("other", 2, "vk_confirmed")
            .unwrap();
        release.send(()).unwrap();
    };
    let (result, ()) = tokio::join!(recovery, switch);
    assert!(result.is_err());
    assert!(service.neighbor_key_inventory_pending("other", 2));
    assert_eq!(service.neighbor_projection_version("other", 2), None);
    assert!(
        !service
            .apply_neighbor_projection("other", 1, snapshot(4, true))
            .await
    );
    assert!(service.neighbor_key_inventory_pending("other", 2));
}

#[tokio::test(start_paused = true)]
async fn recovery_timeout_is_pending_and_does_not_restart_on_the_next_read() {
    let f = fixture().await;
    let service = &f.service;
    service
        .fence_neighbor_key_inventory("owner", 1, "vk_absent")
        .unwrap();
    assert_eq!(
        service
            .recover_neighbor_inventory_with("owner", 1, std::future::pending())
            .await,
        Err(Error::KeyInventoryPending)
    );
    assert_eq!(
        service
            .recover_neighbor_inventory_with("owner", 1, async {
                panic!("An exhausted recovery cannot start another network read")
            })
            .await,
        Err(Error::KeyInventoryPending)
    );
}
