use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn concurrent_ports_share_one_backend_request() {
    let gate = ReadGate::default();
    let calls = AtomicUsize::new(0);
    let release = tokio::sync::Semaphore::new(0);
    let load = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        release.acquire().await.unwrap().forget();
        Ok(json!(7))
    };
    let first = gate.read(("owner".into(), 1), false, load());
    let second = gate.read(("owner".into(), 1), false, load());
    let release_port = async {
        release.add_permits(1);
    };
    let (first, second, _) = tokio::join!(first, second, release_port);
    assert_eq!(first, Ok(json!(7)));
    assert_eq!(second, first);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn bounded_fallback_failure_waits_for_a_new_account_generation() {
    let gate = ReadGate::default();
    assert_eq!(
        gate.read(("owner".into(), 1), false, async { Err(Error::Transport) })
            .await,
        Err(Error::Transport)
    );
    for _consumer in 0..15 {
        assert_eq!(
            gate.read(("owner".into(), 1), false, async {
                panic!("UI activity cannot retry a failed read")
            })
            .await,
            Err(Error::Transport)
        );
    }
    assert_eq!(
        gate.read(("owner".into(), 2), false, async { Ok(json!(1)) })
            .await,
        Ok(json!(1))
    );
}

#[tokio::test]
async fn locked_entitlement_makes_zero_calls_and_change_resumes_reads() {
    let gate = ReadGate::default();
    let calls = AtomicUsize::new(0);
    for locked in [true, true, false] {
        let result = gate
            .read(("owner".into(), 1), locked, async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!(1))
            })
            .await;
        assert_eq!(
            result,
            if locked {
                Err(Error::Locked)
            } else {
                Ok(json!(1))
            }
        );
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(!locked));
    }
}

#[tokio::test]
async fn sign_in_or_account_switch_does_not_inherit_an_old_permanent_refusal() {
    let gate = ReadGate::default();
    assert_eq!(
        gate.read(("owner".into(), 1), false, async { Err(Error::Locked) })
            .await,
        Err(Error::Locked)
    );
    for key in [("owner".into(), 2), ("other".into(), 1)] {
        assert_eq!(
            gate.read(key, false, async { Ok(json!(1)) }).await,
            Ok(json!(1))
        );
    }
}

#[tokio::test]
async fn request_count_oracle_fallback_is_shared_across_four_consumers_and_ui_activity() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    let calls = AtomicUsize::new(0);
    for _consumer_focus_or_view in 0..(4 + 10 + 5) {
        fixture
            .service
            .neighbor_backend_read("friendsList", "owner", 1, async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!([]))
            })
            .await
            .unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fixture.service.invalidate_neighbor_list();
    for generation in [1, 2] {
        fixture
            .service
            .neighbor_backend_read("friendsList", "owner", generation, async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!([]))
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), generation as usize + 1);
    }
}

#[tokio::test]
async fn invalidation_fences_active_and_queued_readers_without_blocking_the_fresh_epoch() {
    let gate = ReadGate::default();
    let calls = AtomicUsize::new(0);
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let old = gate.read(("owner".into(), 1), false, async {
        calls.fetch_add(1, Ordering::SeqCst);
        started.send(()).unwrap();
        held.await.unwrap();
        Ok(json!("old"))
    });
    let queued = gate.read(("owner".into(), 1), false, async {
        panic!("A queued reader from an expired epoch cannot load")
    });
    let control = async {
        running.await.unwrap();
        gate.invalidate();
        let load = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!("fresh"))
        };
        let (first, second) = tokio::join!(
            gate.read(("owner".into(), 1), false, load()),
            gate.read(("owner".into(), 1), false, load()),
        );
        assert_eq!(first, Ok(json!("fresh")));
        assert_eq!(second, first);
        release.send(()).unwrap();
    };
    let (old, queued, _) = tokio::join!(biased; old, queued, control);
    assert_eq!(old, Err(Error::Revoked));
    assert_eq!(queued, Err(Error::Revoked));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(gate.read(("owner".into(), 1), false, async {
        panic!("The expired completion cannot overwrite the fresh cache")
    }).await, Ok(json!("fresh")));
}

#[tokio::test]
async fn account_and_login_switch_fence_in_flight_completions() {
    for next in [("other".to_owned(), 1), ("owner".to_owned(), 2)] {
        let gate = ReadGate::default();
        let (started, running) = tokio::sync::oneshot::channel();
        let (release, held) = tokio::sync::oneshot::channel();
        let old = gate.read(("owner".into(), 1), false, async {
            started.send(()).unwrap();
            held.await.unwrap();
            Ok(json!("old-account"))
        });
        let control = async {
            running.await.unwrap();
            assert_eq!(gate.read(next.clone(), false, async {
                Ok(json!("current-account"))
            }).await, Ok(json!("current-account")));
            release.send(()).unwrap();
        };
        let (old, _) = tokio::join!(old, control);
        assert_eq!(old, Err(Error::Revoked));
        assert_eq!(gate.read(next, false, async {
            panic!("The previous account cannot replace the current cache")
        }).await, Ok(json!("current-account")));
    }
}

#[tokio::test]
async fn mutation_and_refresh_recover_cached_failures_once_per_explicit_epoch() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    service.ext::<crate::config::ConfigState>().lock().neighbors.enabled = true;
    let calls = AtomicUsize::new(0);
    for epoch in 0..3 {
        match epoch {
            1 => service.invalidate_neighbor_list(),
            2 => service.refresh_neighbor_list(),
            _ => (),
        }
        for _activity in 0..19 {
            let result = service.neighbor_backend_read("friendsList", "owner", 1, async {
                calls.fetch_add(1, Ordering::SeqCst);
                if epoch == 0 { Err(Error::Transport) } else { Ok(json!(epoch)) }
            }).await;
            assert_eq!(result, if epoch == 0 { Err(Error::Transport) } else { Ok(json!(epoch)) });
        }
        assert_eq!(calls.load(Ordering::SeqCst), epoch + 1);
    }
}

#[tokio::test]
async fn mutation_and_refresh_retire_an_active_service_list_read() {
    for refresh in [false, true] {
        let fixture = crate::service::test_fixture::ServiceFixture::new(
            crate::multiplexer::state::MultiplexerState::new().0,
        );
        let service = &fixture.service;
        service.ext::<crate::config::ConfigState>().lock().neighbors.enabled = true;
        let (started, running) = tokio::sync::oneshot::channel();
        let (release, held) = tokio::sync::oneshot::channel();
        let old = service.neighbor_backend_read("friendsList", "owner", 1, async {
            started.send(()).unwrap();
            held.await.unwrap();
            Ok(json!("old"))
        });
        let control = async {
            running.await.unwrap();
            if refresh { service.refresh_neighbor_list(); } else { service.invalidate_neighbor_list(); }
            assert_eq!(service.neighbor_backend_read("friendsList", "owner", 1, async {
                Ok(json!("fresh"))
            }).await, Ok(json!("fresh")));
            release.send(()).unwrap();
        };
        let (old, _) = tokio::join!(old, control);
        assert_eq!(old, Err(Error::Revoked));
        assert_eq!(service.neighbor_backend_read("friendsList", "owner", 1, async {
            panic!("An expired service read cannot replace the refreshed value")
        }).await, Ok(json!("fresh")));
    }
}

#[test]
fn registry_commit_keeps_the_epoch_locked_through_the_injected_publication() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let epoch = service.neighbor_list_epoch("owner", 1);
    let mut published = false;
    service.commit_neighbor_list_epoch(&epoch, || {
        assert!(service.extensions.get_or_init(Reads::default).list.state.try_lock().is_none());
        published = true;
    }).unwrap();
    assert!(published);
    service.retire_neighbor_list_reads();
    assert_eq!(service.commit_neighbor_list_epoch(&epoch, || {
        panic!("An expired epoch cannot enter its publication closure")
    }), Err::<(), _>(Error::Revoked));
}
