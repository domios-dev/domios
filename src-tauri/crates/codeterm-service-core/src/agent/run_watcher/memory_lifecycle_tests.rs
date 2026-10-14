use super::*;
use codeterm_mem::dream_status::ExecutionEnd;
use codeterm_mem::workers::{make_info, WorkerStatus};

#[test]
fn extraction_hard_deadline_finalizes_without_restart() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = format!("deadline-{}", uuid::Uuid::new_v4());
    mux.link_pty(&tab_id, pty_id.clone());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    let run = svc.run_watcher.insert_run_for_test(&pty_id, &tab_id);
    let AgentRunKind::MemWorker { worker_id } = &run.kind else {
        unreachable!()
    };
    svc.workers.register(make_info(
        worker_id.clone(),
        tab_id.clone(),
        "extract:item".into(),
        "auto:save".into(),
        "red".into(),
    ));
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(
            Duration::from_secs(3),
            watchdog_loop(svc.clone(), run.clone(), 0),
        )
        .await
        .unwrap();
    });
    let ended = svc.workers.get(worker_id).unwrap();
    assert_eq!(ended.status, WorkerStatus::Error);
    assert!(matches!(
        ended.execution_end,
        Some(ExecutionEnd::TimedOut { .. })
    ));
    assert!(ended.done_at.is_some());
    assert_eq!(run.restart_count.load(Ordering::Acquire), 0);
    assert!(svc.mux.read().get_tab(&tab_id).is_none());
    assert!(!svc.run_watcher.runs.lock().contains_key(&pty_id));
}

#[test]
fn memory_process_exit_records_failure_and_releases_run() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = format!("exit-{}", uuid::Uuid::new_v4());
    mux.link_pty(&tab_id, pty_id.clone());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    let run = svc.run_watcher.insert_run_for_test(&pty_id, &tab_id);
    let AgentRunKind::MemWorker { worker_id } = &run.kind else {
        unreachable!()
    };
    svc.workers.register(make_info(
        worker_id.clone(),
        tab_id.clone(),
        "dreaming:rem".into(),
        "system:dreaming".into(),
        "red".into(),
    ));
    svc.run_watcher
        .fail_now(svc, &pty_id, "process exited".into());
    let ended = svc.workers.get(worker_id).unwrap();
    assert_eq!(ended.status, WorkerStatus::Error);
    assert_eq!(ended.report.as_deref(), Some("process exited"));
    assert!(ended.done_at.is_some());
    assert!(run.completed.load(Ordering::Acquire));
    assert!(!svc.run_watcher.runs.lock().contains_key(&pty_id));
    assert!(svc.mux.read().get_tab(&tab_id).is_none());
}
