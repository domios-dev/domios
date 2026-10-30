use super::*;
use serde_json::json;
use std::sync::atomic::AtomicUsize;

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
async fn cached_failures_wait_for_an_event_and_do_not_retry_on_a_read() {
    let gate = ReadGate::default();
    assert_eq!(gate.read(("owner".into(), 1), false, async { Err(Error::Transport) }).await, Err(Error::Transport));
    for _consumer in 0..15 {
        assert_eq!(gate.read(("owner".into(), 1), false, async { panic!("UI activity cannot retry a failed read") }).await, Err(Error::Transport));
    }
    gate.rearm();
    assert_eq!(gate.read(("owner".into(), 1), false, async { Ok(json!(1)) }).await, Ok(json!(1)));
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
        gate.read(("owner".into(), 1), false, async {
            Err(Error::Locked)
        })
        .await,
        Err(Error::Locked)
    );
    for key in [("owner".into(), 2), ("other".into(), 1)] {
        assert_eq!(
            gate.read(key, false, async { Ok(json!(1)) })
                .await,
            Ok(json!(1))
        );
    }
}

#[tokio::test]
async fn request_count_oracle_fallback_is_shared_across_four_consumers_and_ui_activity() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.ext::<crate::config::ConfigState>().lock().neighbors.enabled = true;
    let calls = AtomicUsize::new(0);
    for _consumer_focus_or_view in 0..(4 + 10 + 5) {
            fixture.service.neighbor_backend_read("friendsList", "owner", 1, async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!([]))
            }).await.unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fixture.service.invalidate_neighbor_list();
    for generation in [1, 2] {
        fixture.service.neighbor_backend_read("friendsList", "owner", generation, async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!([]))
        }).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), generation as usize + 1);
    }
}
