use super::*;
use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

#[test]
fn nonceless_duplicate_does_not_take_the_acknowledged_shortcut() {
    let claim = SessionStartClaim {
        prev: Some("session".into()),
        changed: false,
        nonce_verified: false,
        nonce_already_acknowledged: true,
        identity_evidence: crate::agent::provider_identity::ProviderIdentityEvidence {
            cwd: Some("/tmp/project".into()),
            session_id: Some("session".into()),
            configured_nonce: None,
            reported_nonce: None,
        },
    };

    assert!(!claim.is_unchanged_acknowledged_report());
    assert!(!can_short_circuit_session_start(&claim, true, true));
}

#[test]
fn hook_session_offruntime_keeps_runtime_responsive_while_cron_writer_is_held() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .cwd = Some("/project".into());

    let mut store = CronStore::new_with_path(fixture.temp.path().join("crons.json"));
    let job = store
        .create(CronCreateParams {
            name: "session".into(),
            workspace_id: "ws".into(),
            team_id: None,
            detach_from_team: false,
            schedule: "0 * * * *".into(),
            task: "task".into(),
            description: None,
            agent: Some("codex".into()),
            model: None,
            reasoning_effort: None,
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: None,
            report_target_tab_id: None,
            enabled: None,
            max_history: None,
            oneshot: None,
            jitter_max_ms: Some(0),
            tz: None,
            timeout_seconds: None,
        })
        .unwrap();
    store
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: "run-1".into(),
                started_at: 1,
                finished_at: None,
                status: CronRunStatus::Running,
                tab_id: Some(tab_id.clone()),
                session_id: None,
                cwd: None,
                summary: None,
                report_delivery: None,
            },
        )
        .unwrap();
    store.register_tab(&tab_id, &job.id, "run-1");
    let cron = Arc::new(parking_lot::Mutex::new(store));
    assert!(fixture._app.manage(cron.clone()));

    let (locked_tx, locked_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let writer_cron = cron.clone();
    let writer_holding = Arc::new(AtomicBool::new(false));
    let writer_holding_thread = writer_holding.clone();
    let writer = std::thread::spawn(move || {
        let _guard = writer_cron.lock();
        writer_holding_thread.store(true, Ordering::SeqCst);
        locked_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
        writer_holding_thread.store(false, Ordering::SeqCst);
    });
    locked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("cron writer lock acquired");

    let claim = SessionStartClaim {
        prev: None,
        changed: true,
        nonce_verified: false,
        nonce_already_acknowledged: false,
        identity_evidence: crate::agent::provider_identity::ProviderIdentityEvidence {
            cwd: Some("/project".into()),
            session_id: Some("sid-offruntime".into()),
            configured_nonce: None,
            reported_nonce: None,
        },
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let empty_payload = serde_json::json!({});
    let response = runtime.block_on(async {
        let mut finish = Box::pin(finish_session_binding(
            &fixture.service,
            &tab_id,
            "codex",
            "sid-offruntime",
            None,
            None,
            "session-start",
            &empty_payload,
            claim,
            crate::session::lifecycle::BindingSource::Hook,
            true,
        ));
        let first_poll =
            std::future::poll_fn(|cx| std::task::Poll::Ready(finish.as_mut().poll(cx))).await;
        assert!(first_poll.is_pending());
        assert!(writer_holding.load(Ordering::SeqCst));

        let (progress_tx, progress_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move { progress_tx.send(()).unwrap() });
        progress_rx.await.unwrap();
        assert!(writer_holding.load(Ordering::SeqCst));

        release_tx.send(()).unwrap();
        finish.await
    });
    writer.join().unwrap();

    let response = response.unwrap();
    assert_eq!(response.0["changed"], true);
    let store = cron.lock();
    let run = store
        .get_history(&job.id)
        .into_iter()
        .find(|run| run.run_id == "run-1")
        .unwrap();
    assert_eq!(run.session_id.as_deref(), Some("sid-offruntime"));
    assert_eq!(run.cwd.as_deref(), Some("/project"));
}
