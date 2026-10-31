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
    let state = std::sync::Arc::new(parking_lot::Mutex::new(std::mem::take(port)));
    let owned = state.clone();
    let installation = installation.to_owned();
    let result = service
        .consume_neighbor_projection(
            "owner",
            1,
            snapshot(version, receiver),
            move |list| async move {
                owned.lock().reconcile(list, &installation);
                Ok(())
            },
        )
        .await
        .unwrap();
    *port = std::mem::take(&mut *state.lock());
    result
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
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
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
        service
            .select_neighbor_projection("owner", 1)
            .await
            .unwrap();
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
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
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

#[tokio::test]
async fn fallback_is_bounded_per_account_generation_and_does_not_reset_on_reconnect() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    assert!(service.begin_neighbor_fallback());
    for _reconnect in 0..5 {
        service
            .select_neighbor_projection("owner", 1)
            .await
            .unwrap();
        assert!(!service.begin_neighbor_fallback());
    }
    service
        .select_neighbor_projection("owner", 2)
        .await
        .unwrap();
    assert!(service.begin_neighbor_fallback());
}

#[tokio::test]
async fn mutation_and_refresh_bypass_old_list_projection_until_a_newer_snapshot() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    let calls = AtomicUsize::new(0);
    assert!(
        service
            .apply_neighbor_projection("owner", 1, snapshot(1, None))
            .await
    );
    for version in 2..=3 {
        if version == 2 {
            service.invalidate_neighbor_list().await.unwrap();
        } else {
            service.refresh_neighbor_list().await.unwrap();
        }
        assert!(
            !service
                .apply_neighbor_projection("owner", 1, snapshot(version - 1, None))
                .await
        );
        for _activity in 0..19 {
            assert_eq!(
                service
                    .neighbor_backend_read("friendsList", "owner", 1, async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(json!({"incoming":[{"request_id":"pending"}]}))
                    })
                    .await
                    .unwrap()["incoming"][0]["request_id"],
                "pending"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), version as usize - 1);
        assert!(service
            .peek_neighbor_projection("owner", 1, "friendsOwnKeys", "")
            .is_some());
        assert!(
            service
                .apply_neighbor_projection("owner", 1, snapshot(version, None))
                .await
        );
        assert_eq!(
            service
                .neighbor_backend_read("friendsList", "owner", 1, async {
                    panic!("An authoritative newer snapshot supersedes cached fallback")
                })
                .await
                .unwrap()["version"],
            version
        );
    }
}

#[tokio::test]
async fn projection_arriving_during_fallback_wins_the_returned_list() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let read = service.neighbor_backend_read("friendsList", "owner", 1, async {
        started.send(()).unwrap();
        held.await.unwrap();
        Ok(json!({"friends":[],"incoming":[],"outgoing":[]}))
    });
    let project = async {
        running.await.unwrap();
        assert!(
            service
                .apply_neighbor_projection("owner", 1, snapshot(2, None))
                .await
        );
        release.send(()).unwrap();
    };
    let (result, _) = tokio::join!(read, project);
    let list = result.unwrap();
    assert_eq!(list["version"], 2);
    assert_eq!(list["friends"][0]["uid"], "peer");
}

#[tokio::test]
async fn fallback_publication_cannot_overwrite_a_projection_or_cross_account_or_epoch() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    let initial = service.neighbor_list_epoch("owner", 1);
    assert!(service.publish_neighbor_fallback("owner", 1, &initial, json!("initial")));
    assert!(
        service
            .apply_neighbor_projection("owner", 1, snapshot(1, None))
            .await
    );
    assert!(!service.publish_neighbor_fallback("owner", 1, &initial, json!("late")));
    service.invalidate_neighbor_list().await.unwrap();
    assert!(!service.publish_neighbor_fallback("owner", 1, &initial, json!("expired")));
    let fresh = service.neighbor_list_epoch("owner", 1);
    assert!(service.publish_neighbor_fallback("owner", 1, &fresh, json!("fresh")));
    service
        .select_neighbor_projection("other", 1)
        .await
        .unwrap();
    assert!(!service.publish_neighbor_fallback("owner", 1, &fresh, json!("foreign")));
    assert!(
        !service
            .apply_neighbor_projection("owner", 1, snapshot(50, None))
            .await
    );
    assert!(service
        .peek_neighbor_projection("other", 1, "friendsList", "")
        .is_none());
    assert!(
        service
            .apply_neighbor_projection("other", 1, snapshot(1, None))
            .await
    );
    service
        .select_neighbor_projection("other", 2)
        .await
        .unwrap();
    assert!(
        !service
            .apply_neighbor_projection("other", 1, snapshot(50, None))
            .await
    );
    assert!(service
        .peek_neighbor_projection("other", 2, "friendsList", "")
        .is_none());
    let events = fixture.ui.0.lock();
    let lists: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "neighbors-changed")
        .map(|(_, body)| body["list"].clone())
        .collect();
    assert_eq!(lists, vec![json!("initial"), json!("fresh")]);
}

#[tokio::test]
async fn explicit_invalidation_rearms_one_fallback_but_reconnect_does_not() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    assert!(service.begin_neighbor_fallback());
    service.invalidate_neighbor_list().await.unwrap();
    assert!(service.begin_neighbor_fallback());
    for _reconnect in 0..19 {
        service
            .select_neighbor_projection("owner", 1)
            .await
            .unwrap();
        assert!(!service.begin_neighbor_fallback());
    }
}

#[tokio::test]
async fn expired_projection_reconciliation_cannot_publish_after_refresh_or_account_switch() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let owner = service.clone();
    let reconcile = service.consume_neighbor_projection(
        "owner",
        1,
        snapshot(1, None),
        move |list| async move {
            started.send(()).unwrap();
            held.await.unwrap();
            assert!(owner.publish_neighbor_projection_list("owner", 1, list));
            Ok(())
        },
    );
    let refresh = async {
        running.await.unwrap();
        let owner = service.clone();
        let retirement = service
            .neighbor_reconciliation()
            .submit(
                super::super::reconciliation::Kind::Retirement,
                move || async move {
                    owner.invalidate_neighbor_projection_list();
                    Ok(())
                },
            )
            .unwrap();
        release.send(()).unwrap();
        retirement.wait().await.unwrap();
    };
    let (result, _) = tokio::join!(reconcile, refresh);
    assert_eq!(result, Ok(true));
    assert_eq!(fixture.ui.0.lock().len(), 1);
    assert!(
        service
            .apply_neighbor_projection("owner", 1, snapshot(2, None))
            .await
    );
    let old = service
        .peek_neighbor_projection("owner", 1, "friendsList", "")
        .unwrap();
    service
        .select_neighbor_projection("other", 1)
        .await
        .unwrap();
    assert!(!service.publish_neighbor_projection_list("owner", 1, old));
    assert_eq!(
        service
            .neighbor_backend_read("friendsList", "owner", 1, async {
                panic!("A retired account cannot load or evict the selected account cache")
            })
            .await,
        Err(Error::Revoked)
    );
    assert_eq!(fixture.ui.0.lock().len(), 1);
}

#[tokio::test]
async fn prepared_registry_completion_cannot_reinstall_after_invalidation_or_account_selection() {
    for transition in 0..3 {
        let fixture = crate::service::test_fixture::ServiceFixture::new(
            crate::multiplexer::state::MultiplexerState::new().0,
        );
        let service = &fixture.service;
        service
            .ext::<crate::config::ConfigState>()
            .lock()
            .neighbors
            .enabled = true;
        service
            .select_neighbor_projection("owner", 1)
            .await
            .unwrap();
        let old_epoch = service.neighbor_projection_read_epoch("owner", 1).unwrap();
        let old_list = json!({"access":"paid","friends":[{"friendship_id":"a".repeat(64),"displayName":"Old"}]});
        service
            .remember_neighbor_list_read("owner", 1, &old_epoch, &old_list)
            .unwrap();
        assert!(service.neighbor_registry_receiving(&"a".repeat(64)));
        let (prepared, ready) = tokio::sync::oneshot::channel();
        let (publish, released) = tokio::sync::oneshot::channel();
        let old = async {
            prepared.send(()).unwrap();
            released.await.unwrap();
            service.remember_neighbor_list_read("owner", 1, &old_epoch, &old_list)
        };
        let retire = async {
            ready.await.unwrap();
            let (uid, generation) = match transition {
                0 => {
                    service.invalidate_neighbor_list().await.unwrap();
                    ("owner", 1)
                }
                1 => {
                    service
                        .select_neighbor_projection("other", 1)
                        .await
                        .unwrap();
                    ("other", 1)
                }
                _ => {
                    service
                        .select_neighbor_projection("owner", 2)
                        .await
                        .unwrap();
                    ("owner", 2)
                }
            };
            assert!(service
                .neighbor_cell()
                .registry_snapshot_for_test()
                .is_none());
            let fresh = service
                .neighbor_projection_read_epoch(uid, generation)
                .unwrap();
            service.remember_neighbor_list_read(uid, generation, &fresh,
                &json!({"access":"paid","friends":[{"friendship_id":"b".repeat(64),"displayName":"Fresh"}]}),
            ).unwrap();
            publish.send(()).unwrap();
        };
        let (old, _) = tokio::join!(old, retire);
        assert_eq!(old, Err(Error::Revoked));
        assert!(!service.neighbor_registry_receiving(&"a".repeat(64)));
        assert!(service.neighbor_registry_receiving(&"b".repeat(64)));
        assert_eq!(
            service
                .neighbor_cell()
                .registry_snapshot_for_test()
                .unwrap()["friends"][0]["displayName"],
            "Fresh"
        );
    }
}

#[tokio::test]
async fn registry_publication_before_invalidation_is_cleared_and_old_projection_cannot_reinstall() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    assert!(
        service
            .apply_neighbor_projection("owner", 1, snapshot(1, None))
            .await
    );
    let old = service
        .peek_neighbor_projection("owner", 1, "friendsList", "")
        .unwrap();
    service
        .remember_neighbor_projection_registry("owner", 1, &old)
        .unwrap();
    assert!(service.neighbor_registry_receiving(&"a".repeat(64)));
    service.invalidate_neighbor_list().await.unwrap();
    assert!(service
        .neighbor_cell()
        .registry_snapshot_for_test()
        .is_none());
    assert_eq!(
        service.remember_neighbor_projection_registry("owner", 1, &old),
        Err(Error::Revoked)
    );
    assert!(service
        .neighbor_cell()
        .registry_snapshot_for_test()
        .is_none());
    assert!(
        service
            .apply_neighbor_projection("owner", 1, snapshot(2, None))
            .await
    );
    let fresh = service
        .peek_neighbor_projection("owner", 1, "friendsList", "")
        .unwrap();
    service
        .remember_neighbor_projection_registry("owner", 1, &fresh)
        .unwrap();
    assert_eq!(
        service.remember_neighbor_projection_registry("owner", 1, &old),
        Err(Error::Revoked)
    );
    assert!(service.neighbor_registry_receiving(&"a".repeat(64)));
    service
        .select_neighbor_projection("other", 1)
        .await
        .unwrap();
    assert_eq!(
        service.remember_neighbor_projection_registry("owner", 1, &fresh),
        Err(Error::Revoked)
    );
    assert!(service
        .neighbor_cell()
        .registry_snapshot_for_test()
        .is_none());
}

#[tokio::test]
async fn registry_commit_keeps_account_selection_locked_through_the_injected_publication() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    let epoch = service.neighbor_projection_read_epoch("owner", 1).unwrap();
    service
        .commit_neighbor_list_read("owner", 1, &epoch, || {
            assert!(service
                .extensions
                .get_or_init(Cell::default)
                .state
                .try_lock()
                .is_none());
            service.remember_neighbor_registry(
                &json!({"access":"paid","friends":[{"friendship_id":"a".repeat(64)}]}),
            );
        })
        .unwrap();
    assert!(service.neighbor_registry_receiving(&"a".repeat(64)));
    service.invalidate_neighbor_list().await.unwrap();
    assert!(service
        .neighbor_cell()
        .registry_snapshot_for_test()
        .is_none());
    assert_eq!(
        service.commit_neighbor_list_read("owner", 1, &epoch, || {
            panic!("A retired preparation cannot enter its publication closure")
        }),
        Err::<(), _>(Error::Revoked)
    );
}

#[tokio::test]
async fn queued_newer_projection_refuses_older_effects_and_activation_until_current_commit() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    assert!(
        service
            .apply_neighbor_projection("owner", 1, snapshot(1, None))
            .await
    );
    assert!(service.queue_neighbor_projection_version("owner", 1, 3));
    assert!(service.neighbor_activation_ticket("owner", 1).is_err());
    assert!(!service
        .consume_neighbor_projection("owner", 1, snapshot(2, None), |_| async {
            panic!("A known older queued projection cannot mutate local state")
        })
        .await
        .unwrap());
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    assert!(service
        .consume_neighbor_projection("owner", 1, snapshot(3, None), move |_| async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(service.neighbor_activation_ticket("owner", 1).is_ok());
}
