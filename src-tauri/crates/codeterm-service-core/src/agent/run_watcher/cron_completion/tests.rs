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
#[cfg_attr(
    target_os = "macos",
    ignore = "FSEvents drops appends right after stream start; exact-file watcher in task_1vtxz9"
)]
fn cron_completion_tool_reply_keeps_run_open_until_final_record_is_appended() {
    use crate::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
    use crate::plan::{CuratorRun, Plan};
    use crate::plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use crate::service::{curator::CuratorTabs, AuthenticatedPlanCaller};
    use parking_lot::Mutex;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab_id).unwrap();
    mux.create_tab(&workspace).unwrap();
    mux.link_pty(&tab_id, "pending-pty".into());
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("pending-fixture".into());
        tab.agent_session_id = Some("own-session".into());
        tab.session_binding_nonce = Some("own-nonce".into());
        tab.reported_session_binding_nonce = Some("own-nonce".into());
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
    assert!(!crate::service::agent_runtime::requested_for_tab(
        &fixture.service,
        &tab_id
    ));
    let store = CronStore::new_with_path(fixture.temp.path().join("crons.json"));
    let cron: crate::CronState = Arc::new(Mutex::new(store));
    let wake: crate::CronWake = Arc::new(std::sync::Condvar::new());
    assert!(fixture.service.set_ext(cron.clone()).is_ok());
    assert!(fixture.service.set_ext(wake.clone()).is_ok());
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
                cancel_reason: None,
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
    let mut plan = Plan::new(
        "Manual round".into(),
        fixture.temp.path().display().to_string(),
        Default::default(),
    );
    plan.control_loop.running = Some(CuratorRun {
        number: 1,
        started_at: 1,
        run_id: Some("pending-run".into()),
        tab_id: Some(tab_id.clone()),
        ..Default::default()
    });
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    assert!(fixture.service.set_ext(CuratorTabs::default()).is_ok());
    fixture
        .service
        .ext::<CuratorTabs>()
        .0
        .write()
        .insert(tab_id.clone(), plan.id.clone());
    assert!(!fixture
        .service
        .agent_run_pty_map
        .lock()
        .contains_key(&tab_id));
    assert!(fixture.service.run_watcher.runs.lock().is_empty());
    let round = fixture.service.curator_finish(&AuthenticatedPlanCaller {
        tab_id: Some(tab_id.clone()), session_id: Some("own-session".into()), binding_nonce: Some("own-nonce".into()),
    }, "NEXT: 2h | quiet window\nMOVE: Dispatch remaining work [pl_fixture]\nRETRO:\n- No task moved [pl_fixture]\nNEXT-CHECK:\n- Task dispatch [pl_fixture]\nASKED: none", 7200, 2).unwrap();
    assert_eq!(round.summary, "Dispatch remaining work [pl_fixture]");
    let persisted = fixture.service.plan_store.get(&plan.id).unwrap();
    assert!(persisted.control_loop.running.is_none());
    assert_eq!(persisted.control_loop.last_round.as_ref(), Some(&round));
    assert_eq!(
        persisted
            .strides
            .iter()
            .filter(|stride| stride.kind == "curator_round")
            .count(),
        1
    );
    assert_eq!(
        persisted
            .heartbeat
            .learnings
            .iter()
            .map(|learning| learning.text.as_str())
            .collect::<Vec<_>>(),
        vec![
            "- No task moved [pl_fixture]",
            "- Task dispatch [pl_fixture]"
        ]
    );
    let diagnostic_state = Arc::downgrade(
        fixture
            .service
            .run_watcher
            .runs
            .lock()
            .get("pending-pty")
            .unwrap(),
    );
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_some());
    assert_eq!(
        cron.lock().get_history(&job.id)[0].status,
        CronRunStatus::Running
    );
    for (nonce, success) in [("own-nonce", false), ("foreign-nonce", true)] {
        fixture.service.run_watcher.observe_provider_turn_end(
            &fixture.service,
            &tab_id,
            nonce,
            success,
        );
        assert!(
            !diagnostic_state
                .upgrade()
                .unwrap()
                .cron_completion
                .lock()
                .as_ref()
                .unwrap()
                .barrier
                .stopped
        );
    }
    fixture.service.run_watcher.observe_provider_turn_end(
        &fixture.service,
        &tab_id,
        "own-nonce",
        true,
    );
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_some());
    assert_eq!(
        cron.lock().get_history(&job.id)[0].status,
        CronRunStatus::Running
    );

    let (closed_tx, closed_rx) = mpsc::sync_channel(1);
    let closed_mux = fixture.service.mux.clone();
    let closed_tab = tab_id.clone();
    let persisted = path.clone();
    let listener = fixture.ui.listen("cron-run-finished", move |_| {
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
        .unwrap_or_else(|error| {
            let state = diagnostic_state.upgrade();
            let completion = state.as_ref().and_then(|state| state.cron_completion.try_lock()).map(|pending| {
                pending.as_ref().map(|pending| (
                    pending.barrier.stopped,
                    pending.barrier.assistant_ids.clone(),
                    pending.events.lock().clone(),
                    pending._watcher.is_some(),
                    pending.source_path.clone(),
                    pending.source_path.as_deref().and_then(std::path::Path::parent).map(std::path::Path::to_path_buf),
                ))
            });
            let completed = state.as_ref().map(|state| state.completed.load(Ordering::Acquire));
            let tab_open = fixture.service.mux.try_read().map(|mux| mux.get_tab(&tab_id).is_some());
            let run_status = cron.try_lock().map(|store| store.get_history(&job.id)[0].status.clone());
            let snapshot = crate::service::tab_chat::session_chat_snapshot(&fixture.service, "pending-fixture", "", "own-session", 0);
            panic!("transcript append must release pending completion: {error}; completion(stopped,prior_ids,events,watcher_present,session_path,watch_dir)={completion:?}; completed={completed:?}; tab_open={tab_open:?}; run_status={run_status:?}; snapshot={snapshot:?}");
        });
    fixture.ui.unlisten(listener);
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
