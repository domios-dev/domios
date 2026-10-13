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
fn cron_completion_tool_reply_keeps_run_open_until_final_record_is_appended() {
    use super::super::{AtomicBool, AtomicU32, ResultEventScanner, RunState};
    use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
    use crate::plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use parking_lot::Mutex;
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};
    use tauri::{Listener, Manager};

    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab_id).unwrap();
    mux.create_tab(&workspace).unwrap();
    mux.link_pty(&tab_id, "pending-pty".into());
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("pending-fixture".into());
        tab.agent_session_id = Some("own-session".into());
        tab.session_binding_nonce = Some("own-nonce".into());
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let path = fixture
        .temp
        .path()
        .canonicalize()
        .unwrap()
        .join("own-session.jsonl");
    std::fs::write(&path, "").unwrap();
    let source = format!(
        r#"module.exports.default = {{
        sessionFilePath() {{ return {}; }},
        parseSessionDelta(chunk) {{ return {{ messages: chunk.trim().split('\n').filter(Boolean).map(JSON.parse) }}; }}
    }};"#,
        serde_json::to_string(&path.to_string_lossy()).unwrap()
    );
    let manifest_json =
        r#"{"id":"pending-fixture","version":"0.1.0","displayName":"Pending fixture"}"#;
    let manifest: PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm =
        PluginVm::from_source_with("pending-fixture", source, Some(manifest_json.into())).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, Arc::new(vm))));
    let store = CronStore::new_with_path(fixture.temp.path().join("crons.json"));
    let cron: crate::CronState = Arc::new(Mutex::new(store));
    let wake: crate::CronWake = Arc::new(std::sync::Condvar::new());
    fixture._app.manage(cron.clone());
    fixture._app.manage(wake.clone());
    let job = cron
        .lock()
        .create(CronCreateParams {
            name: "pending-completion".into(),
            workspace_id: workspace,
            team_id: None,
            detach_from_team: false,
            schedule: "0 * * * *".into(),
            task: "opaque task".into(),
            description: None,
            agent: Some("pending-fixture".into()),
            model: None,
            reasoning_effort: None,
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: None,
            report_target_tab_id: None,
            enabled: Some(false),
            max_history: None,
            oneshot: Some(false),
            jitter_max_ms: Some(0),
            tz: None,
            timeout_seconds: None,
        })
        .unwrap();
    cron.lock()
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: "pending-run".into(),
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
    cron.lock().register_tab(&tab_id, &job.id, "pending-run");
    let state = Arc::new(RunState {
        pty_id: "pending-pty".into(),
        tab_id: tab_id.clone(),
        kind: AgentRunKind::Cron {
            job_id: job.id.clone(),
            run_id: "pending-run".into(),
        },
        scanner: Mutex::new(ResultEventScanner::new()),
        started_at: Instant::now(),
        last_byte_at: Mutex::new(Instant::now()),
        completed: AtomicBool::new(false),
        armed: AtomicBool::new(true),
        saw_working: AtomicBool::new(true),
        saw_post_session_progress: AtomicBool::new(true),
        restart_count: AtomicU32::new(0),
        last_restart_at: Mutex::new(Instant::now()),
        resume_system_prompt: None,
        cron_completion: Mutex::new(None),
    });
    fixture
        .service
        .run_watcher
        .runs
        .lock()
        .insert("pending-pty".into(), state);
    assert!(fixture
        .service
        .cron_complete(
            &cron,
            &wake,
            "pending-run",
            Some("opaque summary"),
            Some(&tab_id),
            Some("own-nonce"),
            false
        )
        .unwrap()
        .is_none());
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_some());
    assert_eq!(
        cron.lock().get_history(&job.id)[0].status,
        CronRunStatus::Running
    );
    fixture
        .service
        .run_watcher
        .complete_via_stop_report(&fixture.service, &tab_id, "own-nonce");
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_some());
    assert_eq!(
        cron.lock().get_history(&job.id)[0].status,
        CronRunStatus::Running
    );

    let (closed_tx, closed_rx) = mpsc::sync_channel(1);
    let closed_mux = fixture.service.mux.clone();
    let closed_tab = tab_id.clone();
    let persisted = path.clone();
    let listener = fixture._app.listen("cron-run-finished", move |_| {
        if closed_mux.read().get_tab(&closed_tab).is_none() {
            let _ = closed_tx.try_send(std::fs::read_to_string(&persisted).unwrap());
        }
    });
    let answer = serde_json::json!({"role":"assistant","uuid":"final-record", "blocks":[{"kind":"text", "data":{"text":"CRON_OK_WIRING"}}]});
    let mut writer = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(writer, "{answer}").unwrap();
    writer.sync_all().unwrap();
    let recorded = closed_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("transcript append must release pending completion");
    fixture._app.unlisten(listener);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(recorded.trim()).unwrap(),
        answer
    );
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_none());
    assert_eq!(
        cron.lock().get_history(&job.id)[0].status,
        CronRunStatus::Success
    );
}
