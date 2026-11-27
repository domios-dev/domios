use super::*;

use codeterm_service_core::{
    multiplexer::state::MultiplexerState,
    service::{
        cloud_machines::{CloudMachineController, CloudMachineStore, SystemClock},
        test_fixture::ServiceFixture,
    },
};
use std::sync::Arc;

const ACCOUNT: &str = "billing-owner";

fn billing_fixture() -> (tempfile::TempDir, ServiceFixture) {
    let root = tempfile::tempdir().unwrap();
    let fixture = ServiceFixture::new_at(MultiplexerState::new().0, root.path());
    fixture
        .service
        .install_cloud_machines_for_test(Arc::new(CloudMachineController::new(
            CloudMachineStore::new(root.path().join("cloud_machines.json")),
            "test-controller".into(),
            Arc::new(SystemClock),
        )));
    (root, fixture)
}

fn stamp() -> WireCloudCreditWarnings {
    WireCloudCreditWarnings {
        epoch: 3,
        pct80_at: Some(1_795_700_060),
        ..Default::default()
    }
}

fn lease() -> LeaseHours {
    LeaseHours {
        account: ACCOUNT.into(),
        machine_id: "test-machine".into(),
        warnings: stamp(),
    }
}

fn usage(warnings: Option<WireCloudCreditWarnings>) -> (String, CloudUsageReply) {
    (
        ACCOUNT.into(),
        CloudUsageReply {
            remaining: 4.0,
            warnings,
            ..Default::default()
        },
    )
}

#[tokio::test]
async fn wake_lease_stamp_reaches_the_bell_before_readiness_and_failure_refresh_dedupes() {
    let (_root, fixture) = billing_fixture();
    let svc = &Arc::new(fixture.service.clone());
    let mut reads = 0;
    refresh_hours(svc, Some(lease()), async {
        reads += 1;
        Ok(usage(None))
    })
    .await;
    let first_id = {
        let bell = svc.notif.lock();
        let notes = bell.list(false);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "80% of cloud hours used");
        assert_eq!(
            notes[0].body.as_deref(),
            Some("4.00 cloud hours left. Affected machines: test-machine.")
        );
        notes[0].id.clone()
    };

    let readiness_error = CloudError::WakeDeadlineExceeded {
        machine_id: "test-machine".into(),
    };
    assert!(needs_usage_refresh(&readiness_error));
    refresh_hours(svc, None, async {
        reads += 1;
        Ok(usage(Some(stamp())))
    })
    .await;
    refresh_hours(svc, Some(lease()), async {
        panic!("an already-observed lease stamp must not read usage again");
    })
    .await;
    assert_eq!(reads, 2);
    let bell = svc.notif.lock();
    let notes = bell.list(false);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, first_id);
}

#[tokio::test]
async fn failed_wake_refreshes_usage_once_and_delivers_the_first_stamp() {
    let (_root, fixture) = billing_fixture();
    let svc = &Arc::new(fixture.service.clone());
    assert!(needs_usage_refresh(&CloudError::ProviderUnconfirmed(
        "reply lost".into()
    )));
    assert!(needs_usage_refresh(&CloudError::CreditsExhausted));
    assert!(!needs_usage_refresh(&CloudError::Conflict(
        "not a billing failure".into()
    )));
    let mut reads = 0;

    refresh_hours(svc, None, async {
        reads += 1;
        Ok(usage(Some(stamp())))
    })
    .await;

    assert_eq!(reads, 1);
    let bell = svc.notif.lock();
    let notes = bell.list(false);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].title, "80% of cloud hours used");
}

#[tokio::test]
async fn paid_lease_stamp_survives_usage_lookup_failure() {
    let (_root, fixture) = billing_fixture();
    let svc = &Arc::new(fixture.service.clone());
    refresh_hours(svc, Some(lease()), async {
        Err(CloudError::ProviderUnavailable("usage unavailable".into()))
    })
    .await;
    let bell = svc.notif.lock();
    let notes = bell.list(false);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].title, "80% of cloud hours used");
}
