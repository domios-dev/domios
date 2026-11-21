use super::*;
use std::io::Write;

fn answer(id: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        kind: "assistant".into(),
        content: content.into(),
        ..Default::default()
    }
}

#[test]
fn cron_completion_persists_final_answer_before_close_in_both_event_orders() {
    for stop_first in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let transcript = directory.path().join("own-session.jsonl");
        let previous = answer("previous", "previous turn");
        std::fs::write(
            &transcript,
            format!("{}\n", serde_json::to_string(&previous).unwrap()),
        )
        .unwrap();
        let mut barrier = CompletionBarrier {
            stopped: stop_first,
            assistant_ids: HashSet::from([previous.id.clone()]),
        };
        let read = || -> Vec<ChatMsg> {
            std::fs::read_to_string(&transcript)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        };
        assert!(!barrier.ready(&read()));
        let final_answer = answer("final", "CRON_OK_TEST");
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        writeln!(writer, "{}", serde_json::to_string(&final_answer).unwrap()).unwrap();
        writer.sync_all().unwrap();
        assert_eq!(barrier.ready(&read()), stop_first);
        barrier.stopped = true;
        let mut closed = false;
        if barrier.ready(&read()) {
            assert_eq!(read().last().unwrap().content, final_answer.content);
            closed = true;
        }
        assert!(closed);
    }
}

#[test]
fn cron_completion_cannot_close_for_tools_empty_answers_or_previous_turns() {
    let barrier = CompletionBarrier {
        stopped: true,
        assistant_ids: HashSet::from(["old".into()]),
    };
    assert!(!barrier.ready(&[answer("old", "previous answer")]));
    assert!(!barrier.ready(&[answer("new", " ")]));
    assert!(!barrier.ready(&[ChatMsg {
        kind: "tool_result".into(),
        content: "ok".into(),
        ..Default::default()
    }]));
    assert!(!barrier.ready(&[
        answer("new", "commentary"),
        ChatMsg {
            kind: "tool_use".into(),
            ..Default::default()
        }
    ]));
}

#[test]
fn cron_completion_cannot_attach_to_a_foreign_run() {
    let (mut mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab, "visible-pty".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut store =
        crate::cron::store::CronStore::new_with_path(fixture.temp.path().join("cron.json"));
    store.register_tab(&tab, "own-job", "own-run");
    let cron: crate::CronState = std::sync::Arc::new(parking_lot::Mutex::new(store));
    assert!(fixture.service.set_ext(cron).is_ok());
    let error = fixture
        .service
        .run_watcher
        .cron_completion_state(&fixture.service, &tab, "foreign-run")
        .err()
        .unwrap();
    assert_eq!(error, "cron run ownership changed");
    assert!(fixture.service.run_watcher.runs.lock().is_empty());
}

#[test]
fn completion_lookup_requires_original_admission_and_preserves_a_replacement_watcher() {
    for cancel in [true, false] {
    let (mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let nonce = fixture.service.ensure_session_binding_nonce(&tab);
    let mut store = crate::cron::store::CronStore::new_with_path(fixture.temp.path().join("cron.json"));
    let job = store.create(serde_json::from_value(serde_json::json!({
        "name":"Completion authority", "workspace_id":"fixture", "schedule":"0 * * * *", "task":"opaque", "jitter_max_ms":0,
    })).unwrap()).unwrap();
    store.record_run(&job.id, serde_json::from_value(serde_json::json!({
        "run_id":"owned-run", "started_at":1, "status":"Running", "tab_id":tab,
    })).unwrap()).unwrap();
    store.register_tab(&tab, &job.id, "owned-run");
    let cron: crate::CronState = std::sync::Arc::new(parking_lot::Mutex::new(store));
    assert!(fixture.service.set_ext(cron.clone()).is_ok());
    let registry = &fixture.service.run_watcher;
    assert!(registry.cron_completion_state(&fixture.service, &tab, "owned-run").is_err());
    assert!(registry.runs.lock().is_empty());
    let linkage = crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id: job.id.clone(), run_id:"owned-run".into() };
    registry.reserve_cron_slot(&fixture.service, &cron, &job.id, "owned-run").unwrap();
    let sealed = registry.capture_cron_launch(&fixture.service, &linkage, &tab, &nonce, None).unwrap();
    let pty = fixture.service.link_hidden_agent_cron_pty_with_builder(&tab, Some(&sealed), |gate| {
        crate::pty::manager::build_session(gate, None, None, 120, 40, Default::default())
    }).unwrap();
    let original = registry.cron_binding(&tab, &job.id, "owned-run").unwrap();
    fixture.service.bind_cron_spawn(&cron, &tab, &job.id, "owned-run", Some(nonce.clone()), Some(pty)).unwrap();
    let admitted = registry.cron_completion_state(&fixture.service, &tab, "owned-run").unwrap();
    assert_eq!(admitted.binding_nonce.as_deref(), Some(nonce.as_str()));
    {
        let mut mux = fixture.service.mux.write();
        let replacement = mux.get_tab_mut(&tab).unwrap();
        replacement.session_binding_nonce = Some("replacement-nonce".into());
        replacement.pty_id = Some("replacement-pty".into());
        replacement.agent_session_id = Some("replacement-session".into());
    }
    registry.register(fixture.service.clone(), "replacement-pty".into(), tab.clone(), AgentRunKind::MemWorker { worker_id:"replacement-worker".into() }, 1200, None);
    assert!(registry.cron_completion_state(&fixture.service, &tab, "owned-run").is_err());
    assert!(registry.runs.lock().contains_key("replacement-pty"));
    assert!(registry.register_cron(fixture.service.clone(), &original, 1200, None).is_err());
    if cancel {
        assert_eq!(fixture.service.cron_cancel_running(&cron, &job.id, crate::cron::store::CronCancelReason::UserCancel).unwrap(), 1);
    } else {
        let (ended, receipt) = std::sync::mpsc::channel();
        fixture.ui.listen("cron-run-finished", move |_| { let _ = ended.send(()); });
        super::super::finish_run(&fixture.service, &admitted, false, "delayed original PTY end".into());
        receipt.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let store = cron.lock();
        let old = store.run_receipt(&job.id, "owned-run").unwrap();
        assert_eq!(old.status, crate::cron::store::CronRunStatus::Failed);
        assert!(old.session_id.is_none(), "replacement session cannot bind the old receipt");
        assert_eq!(old.report_delivery.as_deref(), Some("no report target"));
    }
    assert!(registry.runs.lock().contains_key("replacement-pty"));
    assert_eq!(fixture.service.mux.read().get_tab(&tab).unwrap().pty_id.as_deref(), Some("replacement-pty"));
    registry.mark_completed_externally("replacement-pty");
    assert!(admitted.completed.load(std::sync::atomic::Ordering::Acquire));
}
}
