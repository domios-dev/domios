use super::support::*;
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::*;
use std::sync::Arc;
use tokio::sync::Notify;

#[tokio::test]
async fn account_mesh_exhausted_retries_wait_for_explicit_or_network_trigger() {
    for trigger in [EnrollTrigger::Explicit, EnrollTrigger::Network] {
        let harness = Harness::new().await;
        harness.sign_in("owner").await;
        harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Unavailable);
        for attempt in 0..5 {
            assert_eq!(
                harness.mesh.refresh(EnrollTrigger::Lease).await,
                Err(AccountMeshRefusal::Unavailable)
            );
            if attempt < 4 {
                harness.clock.set(harness.state().retry_at.unwrap());
            }
        }
        assert_eq!(harness.calls("enroll"), 5);
        assert_eq!(harness.state().retry_at, None);
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Automatic).await,
            Err(AccountMeshRefusal::Unavailable)
        );
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Lease).await,
            Err(AccountMeshRefusal::Unavailable)
        );
        assert_eq!(harness.calls("enroll"), 5);
        harness.registry.script.lock().unwrap().membership = None;
        assert_eq!(harness.mesh.refresh(trigger).await, Ok(()));
        assert_eq!(harness.calls("enroll"), 6);
        assert_eq!(harness.state().last_refusal, None);
    }
}

#[tokio::test]
async fn account_mesh_notice_and_online_list_share_one_registry_flight() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let before = harness.calls("list");
    let gate = Arc::new(Notify::new());
    harness.registry.script.lock().unwrap().list_gate = Some(gate.clone());
    let notice = harness.mesh.presence_notice("peer", "changed", 1);
    let list = harness.mesh.list();
    let release = async {
        harness.registry.list_entered.notified().await;
        gate.notify_waiters();
    };
    let (notice, list, ()) = tokio::join!(biased; notice, list, release);
    assert_eq!(notice, Ok(true));
    let list = list.unwrap();
    assert_eq!(harness.state().registry.as_ref(), Some(&list));
    assert_eq!(harness.calls("list"), before + 1);
}
