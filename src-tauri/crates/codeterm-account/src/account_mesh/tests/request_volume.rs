use super::support::*;
use crate::account_mesh::wire::RegistryRefusal;
use crate::account_mesh::*;

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

fn immediate<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match std::future::Future::poll(future.as_mut(), &mut context) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("cached mesh operation must complete without waiting"),
    }
}

#[tokio::test]
async fn account_mesh_notice_and_cached_list_never_start_a_registry_flight() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    wait_for_event(harness.mesh.refresh(EnrollTrigger::Automatic))
        .await
        .unwrap();
    let expected = harness.state().registry.clone().unwrap();
    let before = count(&harness.log, "registry:");
    {
        let mut script = harness.registry.script.lock().unwrap();
        script.list = Some(RegistryRefusal::Offline);
        script.membership = Some(RegistryRefusal::Offline);
        script.publication = Some(RegistryRefusal::Offline);
    }
    assert_eq!(
        immediate(harness.mesh.presence_notice("peer", "changed", 1)),
        Ok(false)
    );
    assert_eq!(immediate(harness.mesh.list()).unwrap(), expected);
    for trigger in [
        EnrollTrigger::Automatic,
        EnrollTrigger::Lease,
        EnrollTrigger::Network,
    ] {
        assert_eq!(immediate(harness.mesh.refresh(trigger)), Ok(()));
    }
    assert_eq!(harness.calls("list"), 0);
    assert_eq!(harness.calls("sync"), 0);
    assert_eq!(harness.calls("renew"), 0);
    assert_eq!(count(&harness.log, "registry:"), before);
}
