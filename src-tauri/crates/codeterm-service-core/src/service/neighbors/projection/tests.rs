use super::*;
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
};

fn snapshot(version: u64, installation: Option<&str>) -> Snapshot {
    Snapshot::parse(json!({"schema_version":1,"version":version,"updated_at":100,"access":"paid","can_invite":false,
        "friends":[{"friendship_id":"a".repeat(64),"uid":"peer","displayName":"Peer","email":null,"photoURL":null,"since":1,
            "inbound_per_hour":10,"outbound_per_hour":20,"receiving":false,"my_paused":false,"their_paused":false,
            "my_daily_limit":null,"their_daily_limit":null,"my_file_daily_limit_bytes":null,"their_file_daily_limit_bytes":null,
            "my_receiver":{"installation_id":installation,"machine_label":null,"enc_key_id":installation.map(|_| "ek_active"),"revision":version,"receiving":installation.is_some()},
            "peer_receiver":{"enc_key_id":null,"revision":0,"receiving":false}}],
        "incoming":[],"outgoing":[],"own_keys":[],"referral_trial":{"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}})).unwrap()
}
#[derive(Default)]
struct Inboxes {
    invalidations: usize,
    starts: usize,
    active: BTreeSet<String>,
}
impl Inboxes {
    fn reconcile(&mut self, list: Value, installation: &str) {
        self.invalidations += 1;
        let receiving: BTreeSet<String> = list["friends"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|friend| {
                friend["my_receiver"]["receiving"] == true
                    && friend["my_receiver"]["installation_id"] == installation
            })
            .map(|friend| friend["friendship_id"].as_str().unwrap().to_owned())
            .collect();
        for _friend in receiving.difference(&self.active) {
            self.starts += 1;
        }
        self.active = receiving;
    }
}
async fn deliver(
    service: &ServiceContext,
    port: &mut Inboxes,
    installation: &str,
    version: u64,
    receiver: Option<&str>,
) -> bool {
    service
        .consume_neighbor_projection("owner", 1, snapshot(version, receiver), |list| async move {
            port.reconcile(list, installation);
            Ok(())
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn projection_count_oracle_idle_focus_views_reconnects_and_remote_receiver_assignment() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let installation = service.installation_id().await.unwrap().as_str().to_owned();
    service.select_neighbor_projection("owner", 1);
    let mut inboxes = Inboxes::default();
    let calls = AtomicUsize::new(0);
    assert!(deliver(service, &mut inboxes, &installation, 1, None).await);
    assert_eq!((inboxes.invalidations, inboxes.starts), (1, 0));
    for _consumer_focus_or_view in 0..(4 + 10 + 5) {
        for route in ["friendsList", "friendsOwnKeys", "friendsReceiverStatus"] {
            service
                .neighbor_backend_read(route, "owner", 1, async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(json!([]))
                })
                .await
                .unwrap();
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for _reconnect in 0..3 {
        service.select_neighbor_projection("owner", 1);
        assert!(!deliver(service, &mut inboxes, &installation, 1, None).await);
    }
    assert!(!deliver(service, &mut inboxes, &installation, 0, None).await);
    assert_eq!((inboxes.invalidations, inboxes.starts), (1, 0));
    assert!(deliver(service, &mut inboxes, &installation, 2, Some(&installation)).await);
    assert_eq!(
        (inboxes.invalidations, inboxes.starts, inboxes.active.len()),
        (2, 1, 1)
    );
    assert!(deliver(service, &mut inboxes, &installation, 3, None).await);
    assert_eq!(
        (inboxes.invalidations, inboxes.starts, inboxes.active.len()),
        (3, 1, 0)
    );
    assert!(deliver(service, &mut inboxes, &installation, 4, Some(&installation)).await);
    assert_eq!((inboxes.invalidations, inboxes.starts), (4, 2));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_local_inbox_reconciliation_can_recover_on_equal_snapshot_without_accepting_old_data(
) {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service.select_neighbor_projection("owner", 1);
    assert_eq!(
        service
            .consume_neighbor_projection("owner", 1, snapshot(2, None), |_| async {
                Err(Error::LocalKeyUnavailable)
            })
            .await,
        Err(Error::LocalKeyUnavailable)
    );
    assert!(service
        .consume_neighbor_projection(
            "owner",
            1,
            snapshot(2, Some("foreign-assignment")),
            |list| async move {
                assert!(list["friends"][0]["my_receiver"]["installation_id"].is_null());
                Ok(())
            }
        )
        .await
        .unwrap());
    assert!(!service
        .consume_neighbor_projection("owner", 1, snapshot(1, None), |_| async {
            panic!("Stale data cannot reconcile")
        })
        .await
        .unwrap());
}

#[test]
fn fallback_is_bounded_per_account_generation_and_does_not_reset_on_reconnect() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service.select_neighbor_projection("owner", 1);
    assert!(service.begin_neighbor_fallback());
    for _reconnect in 0..5 {
        service.select_neighbor_projection("owner", 1);
        assert!(!service.begin_neighbor_fallback());
    }
    service.select_neighbor_projection("owner", 2);
    assert!(service.begin_neighbor_fallback());
}
