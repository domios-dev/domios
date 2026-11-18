use super::{close, started};
use crate::cron::clock;
use crate::cron::store::{CronCreateParams, CronRunStatus, CronStorageOptions, CronStore};
use crate::plugin::js_runtime::{JsProvider, PluginVm};
use crate::service::spawn::TabHandle;
use crate::service::ServiceContext;
use crate::storage::Storage;
use parking_lot::Mutex;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const AGENT: &str = "round-trip-agent";
const SESSION: &str = "round-trip-session";

fn cron_for(
    service: &ServiceContext,
    storage: &Storage,
    root: &std::path::Path,
) -> (crate::CronState, crate::CronWake) {
    service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_reasoning_efforts(
            AGENT,
            &["low", "medium", "high"],
        )
        .with_launch_binary(crate::agent::test_provider::test_shell_binary()),
    ));
    let store = CronStore::open_storage(
        storage,
        root.join("crons"),
        clock::system_clock(),
        CronStorageOptions::default(),
        crate::config::schema::AppConfig::default()
            .general_agent
            .agent_provider,
    )
    .expect("startup hydrates cron from Storage");
    let cron = Arc::new(Mutex::new(store));
    let wake = Arc::new(std::sync::Condvar::new());
    assert!(service.set_ext(cron.clone()).is_ok());
    assert!(service.set_ext(wake.clone()).is_ok());
    (cron, wake)
}

fn cron_params(root: &std::path::Path, workspace_id: String) -> CronCreateParams {
    serde_json::from_value(serde_json::json!({
        "name": "round-trip schedule",
        "workspace_id": workspace_id,
        "schedule": "0 * * * *",
        "task": "round-trip task",
        "agent": AGENT,
        "cwd": root,
        "enabled": false,
        "jitter_max_ms": 0
    }))
    .expect("valid cron parameters")
}

#[test]
fn cron_create_update_delete_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let (cron, wake) = cron_for(&instance.fixture.service, &instance.storage, root.path());
    let created = instance
        .fixture
        .service
        .cron_create(&cron, &wake, cron_params(root.path(), workspace_id))
        .expect("create cron through ServiceContext");
    let updated = instance
        .fixture
        .service
        .cron_update(
            &cron,
            &wake,
            &created.id,
            serde_json::from_value(serde_json::json!({
                "name": "updated schedule",
                "task": "updated task"
            }))
            .unwrap(),
        )
        .expect("update cron through ServiceContext");
    assert_eq!(updated.task, "updated task");
    drop(cron);
    drop(wake);

    let restarted = instance.restart(root.path());
    let (cron, wake) = cron_for(&restarted.fixture.service, &restarted.storage, root.path());
    let restored = restarted
        .fixture
        .service
        .cron_get(&cron, &created.id)
        .expect("updated cron definition survives restart");
    assert_eq!(restored.name, "updated schedule");
    assert_eq!(restored.task, "updated task");
    restarted
        .fixture
        .service
        .cron_delete(&cron, &wake, &created.id)
        .expect("delete cron through ServiceContext");
    drop(cron);
    drop(wake);

    let restarted = restarted.restart(root.path());
    let (cron, wake) = cron_for(&restarted.fixture.service, &restarted.storage, root.path());
    assert!(restarted
        .fixture
        .service
        .cron_get(&cron, &created.id)
        .is_err());
    drop(cron);
    drop(wake);
    close(restarted);
}

fn install_cron_transcript_provider(service: &ServiceContext, transcript: &Path) {
    let manifest = serde_json::from_value(serde_json::json!({
        "id": AGENT,
        "version": "0.1.0",
        "displayName": "Cron transcript fixture",
        "capabilities": {"recordSources": {"sessionTranscript": {
            "format": "jsonl",
            "filter": {"type": {"path": ["type"], "in": ["assistant"]}}
        }}}
    }))
    .unwrap();
    let source = format!(
        r#"module.exports = {{
            sessionFilePath(cwd, session) {{ return session === {} ? {} : null; }},
            parseSessionRecords(records, meta) {{
                return {{
                    new_offset: meta.newOffset,
                    messages: records.map(record => ({{
                        role: record.value.type,
                        uuid: record.value.uuid,
                        recordIndex: record.row,
                        blocks: [{{kind: "text", data: {{text: record.value.content}}}}]
                    }}))
                }};
            }}
        }};"#,
        serde_json::json!(SESSION),
        serde_json::json!(transcript.to_string_lossy())
    );
    let vm = Arc::new(PluginVm::from_source(AGENT, source).unwrap());
    service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, vm)));
}

#[test]
fn cron_run_start_finish_and_history_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let service = &instance.fixture.service;
    let workspace_id = service.mux.read().list_workspaces()[0].id.clone();
    let (cron, wake) = cron_for(service, &instance.storage, root.path());
    let created = service
        .cron_create(&cron, &wake, cron_params(root.path(), workspace_id.clone()))
        .expect("create cron through ServiceContext");
    let transcript = root.path().join("cron-session.jsonl");
    let mut writer = std::fs::File::create(&transcript).unwrap();
    install_cron_transcript_provider(service, &transcript);
    let run = service
        .cron_trigger_with_spawn(&cron, &created.id, |spec| {
            let history = service.cron_history(&cron, &created.id)?;
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].status, CronRunStatus::Running);
            assert!(history[0].tab_id.is_none());

            let mut mux = service.mux.write();
            let tab_id = mux.create_tab(&workspace_id)?;
            let pty_id = format!("fixture-{tab_id}");
            mux.link_pty(&tab_id, pty_id.clone());
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.hidden = true;
            tab.agent_type = Some(AGENT.into());
            tab.cwd = spec.cwd;
            Ok(TabHandle {
                command_submission: None,
                tab_id,
                pty_id: Some(pty_id),
                workspace_id,
                description: spec.description,
                worker_id: None,
                warning: None,
                model_fallback: None,
                budget_notice: None,
            })
        })
        .expect("trigger cron through ServiceContext");
    let tab_id = run
        .tab_id
        .as_deref()
        .expect("trigger returned the owning tab");
    let nonce = service.ensure_session_binding_nonce(tab_id);
    assert_eq!(
        service
            .cron_complete(&cron, &wake, &run.run_id, Some("early"), None, None, true)
            .err()
            .as_deref(),
        Some("cron session missing")
    );
    service.set_agent_session_id(tab_id, Some(SESSION.into()));

    let complete = |summary, nonce| {
        service.cron_complete(
            &cron,
            &wake,
            &run.run_id,
            Some(summary),
            Some(tab_id),
            Some(nonce),
            false,
        )
    };
    assert!(complete("foreign completion", "foreign nonce").is_err());
    assert!(complete("finished", &nonce)
        .expect("request completion")
        .is_none());
    assert!(complete("duplicate", &nonce)
        .expect("repeat completion request")
        .is_none());

    writeln!(
        writer,
        "{}",
        serde_json::json!({"type": "assistant", "uuid": "answer", "content": "final answer"})
    )
    .unwrap();
    writer.sync_all().unwrap();
    drop(writer);
    let pending = service.cron_history(&cron, &created.id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].run_id, run.run_id);
    assert_eq!(pending[0].status, CronRunStatus::Running);
    assert!(pending[0].finished_at.is_none());
    assert!(service.mux.read().get_tab(tab_id).is_some());

    let (changed_tx, changed_rx) = std::sync::mpsc::channel();
    let listener = instance.fixture.ui.listen("cron-run-finished", move |_| {
        let _ = changed_tx.send(());
    });
    service
        .run_watcher
        .observe_provider_turn_end(service, tab_id, &nonce, true);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let history = service.cron_history(&cron, &created.id).unwrap();
        if history[0].report_delivery.is_some() {
            break;
        }
        changed_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("provider stop and durable answer settle the run");
    }
    instance.fixture.ui.unlisten(listener);
    assert!(service.mux.read().get_tab(tab_id).is_none());
    assert!(complete("late duplicate", &nonce)
        .expect("repeat settled completion")
        .is_none());

    let spawn_error = "fixture spawn failed";
    assert_eq!(
        service
            .cron_trigger_with_spawn(&cron, &created.id, |_| Err(spawn_error.into()))
            .err()
            .as_deref(),
        Some(spawn_error)
    );
    let history = service.cron_history(&cron, &created.id).unwrap();
    let failed_run_id = history
        .iter()
        .find(|row| row.status == CronRunStatus::Failed)
        .expect("failed trigger records its run")
        .run_id
        .clone();
    drop(cron);
    drop(wake);

    let restarted = instance.restart(root.path());
    let (cron, wake) = cron_for(&restarted.fixture.service, &restarted.storage, root.path());
    let history = restarted
        .fixture
        .service
        .cron_history(&cron, &created.id)
        .expect("cron history survives restart");
    assert_eq!(history.len(), 2);
    let saved = history
        .iter()
        .find(|row| row.run_id == run.run_id)
        .expect("successful run survived restart");
    assert_eq!(saved.tab_id.as_deref(), Some(tab_id));
    assert_eq!(saved.status, CronRunStatus::Success);
    assert!(saved.finished_at.is_some());
    assert_eq!(saved.session_id.as_deref(), Some(SESSION));
    assert_eq!(saved.summary.as_deref(), Some("finished"));
    let failed = history
        .iter()
        .find(|row| row.run_id == failed_run_id)
        .expect("failed run survived restart");
    assert_eq!(failed.status, CronRunStatus::Failed);
    assert!(failed.finished_at.is_some());
    drop(cron);
    drop(wake);
    close(restarted);
}
