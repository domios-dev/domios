use super::*;
use std::{sync::atomic::AtomicUsize, time::Duration};
use serde_json::json;

#[tokio::test]
async fn concurrent_ports_share_one_backend_request() {
    let gate = ReadGate::default();
    let calls = AtomicUsize::new(0);
    let release = tokio::sync::Semaphore::new(0);
    let load = || async { calls.fetch_add(1, Ordering::SeqCst); release.acquire().await.unwrap().forget(); Ok(json!(7)) };
    let first = gate.read(("owner".into(), 1), false, Instant::now, || 500, load());
    let second = gate.read(("owner".into(), 1), false, Instant::now, || 500, load());
    let release_port = async { release.add_permits(1); };
    let (first, second, _) = tokio::join!(first, second, release_port);
    assert_eq!(first, Ok(json!(7)));
    assert_eq!(second, first);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failure_deadlines_cap_reset_on_success_and_permanent_refusals_wait_for_rearm() {
    let gate = ReadGate::default();
    let start = Instant::now();
    let mut now = start;
    let calls = AtomicUsize::new(0);
    for ceiling in [500, 1000, 2000, 4000, 8000, 16000, 32000, 64000, 128000, 256000, 300000, 300000] {
        let failure = async { calls.fetch_add(1, Ordering::SeqCst); Err(Error::Transport) };
        assert_eq!(gate.read(("owner".into(), 1), false, || now, || ceiling, failure).await, Err(Error::Transport));
        assert_eq!(gate.state.lock().await.retry_at, Some(now + Duration::from_millis(ceiling)));
        let blocked = async { panic!("A deadline must suppress the port") };
        assert_eq!(gate.read(("owner".into(), 1), false, || now, || 0, blocked).await, Err(Error::Transport));
        now += Duration::from_millis(ceiling);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 12);
    assert_eq!(gate.read(("owner".into(), 1), false, || now, || 0, async { Ok(json!(1)) }).await, Ok(json!(1)));
    assert_eq!(gate.state.lock().await.failures, 0);
    assert_eq!(gate.read(("owner".into(), 1), false, || now, || 500, async { Err(Error::Transport) }).await, Err(Error::Transport));
    assert_eq!(gate.state.lock().await.retry_at, Some(now + Duration::from_millis(500)));
    now += Duration::from_millis(500);
    for refusal in [Error::Locked, Error::PermissionDenied, Error::SessionRevoked, Error::Firestore { code: 7, operation: "listen" }] {
        gate.rearm();
        assert_eq!(gate.read(("owner".into(), 1), false, || now, || 0, async { Err(refusal) }).await, Err(refusal));
        now += Duration::from_secs(86400);
        assert_eq!(gate.read(("owner".into(), 1), false, || now, || 0, async { panic!("Permanent refusal needs an event") }).await, Err(refusal));
        gate.rearm();
        assert_eq!(gate.read(("owner".into(), 1), false, || now, || 0, async { Ok(json!(2)) }).await, Ok(json!(2)));
    }
}

#[tokio::test]
async fn locked_entitlement_makes_zero_calls_and_change_resumes_reads() {
    let gate = ReadGate::default();
    let calls = AtomicUsize::new(0);
    for locked in [true, true, false] {
        let result = gate.read(("owner".into(), 1), locked, Instant::now, || 0, async {
            calls.fetch_add(1, Ordering::SeqCst); Ok(json!(1))
        }).await;
        assert_eq!(result, if locked { Err(Error::Locked) } else { Ok(json!(1)) });
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(!locked));
    }
}

#[tokio::test]
async fn sign_in_or_account_switch_does_not_inherit_an_old_permanent_refusal() {
    let gate = ReadGate::default();
    assert_eq!(gate.read(("owner".into(), 1), false, Instant::now, || 0, async { Err(Error::Locked) }).await, Err(Error::Locked));
    for key in [("owner".into(), 2), ("other".into(), 1)] {
        assert_eq!(gate.read(key, false, Instant::now, || 0, async { Ok(json!(1)) }).await, Ok(json!(1)));
    }
}

#[tokio::test]
async fn concurrent_own_key_consumers_share_one_backend_request() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let calls = AtomicUsize::new(0);
    let release = tokio::sync::Semaphore::new(0);
    let load = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        release.acquire().await.unwrap().forget();
        Ok(json!({"keys":[]}))
    };
    let first = fixture.service.neighbor_backend_read("friendsOwnKeys", "owner", 1, load());
    let second = fixture.service.neighbor_backend_read("friendsOwnKeys", "owner", 1, load());
    let release = async { release.add_permits(1); };
    let (first, second, _) = tokio::join!(first, second, release);
    assert_eq!(first, Ok(json!({"keys":[]})));
    assert_eq!(first, second);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn referral_claim_is_cached_for_the_login_session_even_across_refresh_and_duplicate_account_events() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let calls = AtomicUsize::new(0);
    for generation in [1, 1, 1, 2] {
        let value = fixture.service.neighbor_backend_read("claimReferralTrial", "owner", generation, async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"outcome":"not_invited"}))
        }).await;
        assert_eq!(value, Ok(json!({"outcome":"not_invited"})));
        fixture.service.rearm_neighbor_reads();
        assert_eq!(calls.load(Ordering::SeqCst), generation as usize);
    }
}
