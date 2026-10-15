use super::*;
use crate::agent::chat_types::RichMessage;
use crate::multiplexer::state::MultiplexerState;
use codeterm_provider_signals::{AuthStatus, SessionObservations};
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn fixture() -> (MuxState, String, Option<String>) {
    let (mut state, id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own".into());
    tab.session_binding_nonce = Some("nonce".into());
    (
        Arc::new(TimedRwLock::new("mux", state)),
        id,
        Some("fixture".into()),
    )
}

fn record(error: Option<ApiErrorKind>, clears: bool) -> RichMessage {
    RichMessage {
        uuid: Some("turn".into()),
        provider_session_id: Some("own".into()),
        provider_observations: Some(SessionObservations {
            api_error: error,
            clears_api_error: clears,
            permission_mode: Some(PermissionMode::Plan),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn compute_signals_incidents_and_recovery_without_mutating_snapshot() {
    let (mux, id, _) = fixture();
    let mut snapshot = SignalSnapshot::capture(mux.read().get_tab(&id).unwrap());
    snapshot.task = Some("task".into());
    snapshot.signals.auth = Some(AuthStatus {
        logged_in: false,
        auth_method: None,
        subscription_type: None,
        config_directory: None,
    });
    snapshot.signals.api_error = Some(ApiErrorKind::Authentication);
    let result = compute(
        &snapshot,
        "own",
        &[
            record(None, true),
            record(Some(ApiErrorKind::UsageLimit), false),
        ],
        true,
    );
    assert!(result.changed);
    assert!(result.refresh_needed);
    assert_eq!(result.changed_error, Some(ApiErrorKind::UsageLimit));
    assert_eq!(result.signals.permission_mode, Some(PermissionMode::Plan));
    assert_eq!(result.transition, IncidentTransition::Recover);
    assert_eq!(result.fresh.len(), 1);
    assert_eq!(result.fresh[0].worker, id);
    assert_eq!(result.fresh[0].task.as_deref(), Some("task"));
    assert!(result.fresh[0].pauses_for_exhaustion);
    assert!(snapshot.signals.admitted_provider_incidents.is_empty());
    snapshot.signals = result.signals;
    let repeated = compute(
        &snapshot,
        "own",
        &[record(Some(ApiErrorKind::UsageLimit), false)],
        false,
    );
    assert!(repeated.fresh.is_empty());
    let foreign = RichMessage {
        provider_session_id: Some("foreign".into()),
        ..record(None, true)
    };
    assert!(!compute(&snapshot, "own", &[foreign], true).refresh_needed);
}

#[test]
fn stale_signals_recompute_and_preserve_concurrent_incident_admission() {
    let (mux, id, provider) = fixture();
    let records = [record(Some(ApiErrorKind::RateLimit), false)];
    let mut calls = 0;
    let result = update_with(&mux, &id, "own", "nonce", &provider, |snapshot| {
        calls += 1;
        let computed = compute(snapshot, "own", &records, true);
        if calls == 1 {
            mux.write().get_tab_mut(&id).unwrap().provider_signals = computed.signals.clone();
        }
        computed
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert!(result.fresh.is_empty());
    assert!(!result.changed);
    assert_eq!(
        mux.read()
            .get_tab(&id)
            .unwrap()
            .provider_signals
            .admitted_provider_incidents
            .len(),
        1
    );
}

#[test]
fn contention_is_bounded_and_stale_binding_cannot_commit() {
    let (mux, id, provider) = fixture();
    let mut calls = 0;
    assert!(
        update_with(&mux, &id, "own", "nonce", &provider, |snapshot| {
            calls += 1;
            let computed = compute(snapshot, "own", &[], true);
            mux.write()
                .get_tab_mut(&id)
                .unwrap()
                .provider_signals
                .auth_probe_pending = calls == 1;
            computed
        })
        .is_none()
    );
    assert_eq!(calls, 2);
    assert!(
        update_with(&mux, &id, "own", "nonce", &provider, |snapshot| {
            mux.write().get_tab_mut(&id).unwrap().session_binding_nonce = Some("new".into());
            compute(
                snapshot,
                "own",
                &[record(Some(ApiErrorKind::Authentication), false)],
                true,
            )
        })
        .is_none()
    );
    assert_eq!(
        mux.read().get_tab(&id).unwrap().provider_signals.api_error,
        None
    );
}

#[test]
fn twenty_thousand_records_compute_with_mux_available_to_another_reader() {
    let (mux, id, provider) = fixture();
    let worker_mux = mux.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let records = vec![record(Some(ApiErrorKind::RateLimit), false); 20_000];
        update_with(&worker_mux, &id, "own", "nonce", &provider, |snapshot| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            compute(snapshot, "own", &records, true)
        })
        .is_some()
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let reader = mux.try_read_until(std::time::Instant::now() + Duration::from_millis(50));
    let acquired = reader.is_some();
    drop(reader);
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap());
    assert!(acquired, "transcript computation must not own mux.write");
}
