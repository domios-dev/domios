use super::support::*;
use crate::account_mesh::{wire::RegistryRefusal, *};

#[tokio::test]
async fn account_mesh_changed_local_key_recovers_revoked_slot_without_registry_read() {
    let mut harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.registry.revoke_self("owner");
    *harness.keys.0.lock().unwrap() = Some(InstallationKey::generate().to_secret());
    harness.clock.set(START + 3600);
    harness.restart().await;
    harness.sign_in("owner").await;
    harness.mesh.enrollment_signed_in();
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    assert_eq!(harness.calls("list"), 0);
    assert_eq!(harness.calls("recover"), 1);
    assert_eq!(harness.calls("recover_key"), 1);
    assert_eq!(harness.calls("enroll"), 1);
    assert_eq!(
        harness
            .state()
            .self_installation
            .as_ref()
            .unwrap()
            .key_generation,
        2
    );
    assert!(harness.state().current_membership(START + 3600).is_some());
}

#[tokio::test]
async fn account_mesh_stuck_typed_forbidden_backs_off_for_hours_without_repeated_rotation() {
    let harness = Harness::new().await;
    harness.sign_in("owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    harness.registry.revoke_self("owner");
    harness.clock.set(START + 3600);
    harness.registry.script.lock().unwrap().membership = Some(RegistryRefusal::Forbidden);
    harness.mesh.enrollment_signed_in();
    assert_eq!(
        harness.mesh.refresh(EnrollTrigger::Automatic).await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    let before = count(&harness.log, "registry:");
    for delay in [300, 600, 1200, 2400, 4800, 9600, 14400] {
        harness.clock.set(START + 3600 + delay);
        assert_eq!(
            harness.mesh.refresh(EnrollTrigger::Lease).await,
            Err(AccountMeshRefusal::ReauthRequired)
        );
    }
    assert_eq!(count(&harness.log, "registry:"), before);
    assert_eq!(harness.calls("recover_key"), 1);
}

#[test]
fn account_mesh_renewal_margin_is_one_twelfth_of_each_granted_lifetime() {
    for duration in [1, 119, 120, 121, 600, 1800, 2400, 3600, 86400, 172800] {
        let membership = VerifiedMembership {
            account_id: "account".into(),
            installation_id: INSTALLATION.into(),
            key_id: "a".repeat(64),
            key_generation: 1,
            slot_revision: 1,
            issued_at: START,
            expires_at: START + duration,
        };
        assert_eq!(membership.refresh_after(), START + duration - duration / 12);
    }
}

#[tokio::test]
async fn account_mesh_manual_network_and_signin_rearm_escape_four_hour_permanent_backoff() {
    for trigger in [
        EnrollTrigger::Explicit,
        EnrollTrigger::Network,
        EnrollTrigger::Automatic,
    ] {
        let harness = Harness::new().await;
        harness.registry.script.lock().unwrap().membership_seconds = Some(86400);
        harness.sign_in("owner").await;
        harness
            .mesh
            .refresh(EnrollTrigger::Automatic)
            .await
            .unwrap();
        *harness.endpoint.0.lock().unwrap() = Some(tailnet_endpoint());
        harness.registry.script.lock().unwrap().publication = Some(RegistryRefusal::Invalid);
        for attempt in 0..8 {
            assert_eq!(
                harness.mesh.refresh(EnrollTrigger::Lease).await,
                Err(AccountMeshRefusal::Invalid)
            );
            if attempt < 7 {
                harness.clock.set(harness.state().retry_at.unwrap());
            }
        }
        assert_eq!(
            harness.state().retry_at.unwrap() - harness.mesh.now(),
            14400
        );
        let calls = harness.calls("publish");
        let renewals = harness.calls("renew");
        harness.registry.script.lock().unwrap().publication = None;
        for deferred in [EnrollTrigger::Lease, EnrollTrigger::Automatic] {
            assert_eq!(
                harness.mesh.refresh(deferred).await,
                Err(AccountMeshRefusal::Invalid)
            );
        }
        assert_eq!(harness.calls("publish"), calls);
        if trigger == EnrollTrigger::Automatic {
            harness.mesh.enrollment_signed_in();
        }
        harness.mesh.refresh(trigger).await.unwrap();
        let renewed = trigger == EnrollTrigger::Automatic;
        assert_eq!(harness.calls("publish"), calls + usize::from(!renewed));
        assert_eq!(harness.calls("renew"), renewals + usize::from(renewed));
        assert!(harness.registry.endpoint("owner").is_some());
        assert_eq!(harness.state().publication, Publication::Published);
        assert_eq!(harness.state().retry_at, None);
    }
}
