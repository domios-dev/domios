use super::*;

fn tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("workers-{}-{}.json", name, uuid::Uuid::new_v4()))
}

fn fresh_info(id: &str) -> GraphWorkerInfo {
    make_info(
        id.into(),
        "tab1".into(),
        "test intent".into(),
        "user:test".into(),
        "hsl(0,0%,0%)".into(),
    )
}

#[test]
fn worker_orphan_duration_uses_observed_activity_not_restart_time() {
    let path = tmp_path("heartbeat");
    let reg = GraphWorkerRegistry::load(path.clone());
    let mut worker = fresh_info("gw_heartbeat");
    worker.spawned_at = 1000;
    worker.last_observed_at = Some(4000);
    reg.register(worker);
    let loaded = GraphWorkerRegistry::load(path);
    let orphan = loaded.get("gw_heartbeat").unwrap();
    assert_eq!(orphan.status, WorkerStatus::Orphaned);
    assert_eq!(orphan.done_at, Some(4000));
    assert_eq!(orphan.duration_ms(90_000_000), Some(3000));
    let mut historical = orphan;
    historical.last_observed_at = None;
    historical.done_at = Some(90_000_000);
    assert_eq!(historical.duration_ms(100_000_000), None);
    historical.status = WorkerStatus::Error;
    historical.done_at = None;
    assert_eq!(historical.duration_ms(100_000_000), None);
}

#[tokio::test]
async fn worker_terminal_event_records_error_and_preserves_first_end() {
    let reg = GraphWorkerRegistry::load(tmp_path("end-event"));
    reg.register(fresh_info("gw_end"));
    let end = ExecutionEnd::ProcessEnded {
        detail: "launch refused".into(),
    };
    let first = reg.finish_execution("gw_end", None, end.clone()).unwrap();
    reg.wait_terminal("gw_end").await;
    assert_eq!(first.report.as_deref(), Some("launch refused"));
    assert!(first.done_at.is_some());
    let second = reg
        .finish_execution(
            "gw_end",
            None,
            ExecutionEnd::TimedOut {
                detail: "late watchdog".into(),
            },
        )
        .unwrap();
    assert_eq!(second.execution_end, Some(end));
    assert_eq!(second.done_at, first.done_at);
}

#[test]
fn register_list_get() {
    let reg = GraphWorkerRegistry::load(tmp_path("rlg"));
    reg.register(fresh_info("gw_aaaa"));
    reg.register(fresh_info("gw_bbbb"));
    assert_eq!(reg.list().len(), 2);
    assert!(reg.get("gw_aaaa").is_some());
    assert!(reg.get("gw_zzzz").is_none());
}

#[test]
fn progress_and_complete() {
    let reg = GraphWorkerRegistry::load(tmp_path("pc"));
    reg.register(fresh_info("gw_x"));
    reg.update_progress("gw_x", "scanning 5/10".into()).unwrap();
    reg.touch("gw_x", "skill_a");
    reg.touch("gw_x", "fact_b");
    let info = reg.complete("gw_x", "all done".into()).unwrap();
    assert_eq!(info.status, WorkerStatus::Done);
    assert_eq!(info.report.as_deref(), Some("all done"));
    assert_eq!(info.progress_text.as_deref(), Some("scanning 5/10"));
    assert_eq!(info.touched_nodes.len(), 2);
}

#[test]
fn persist_marks_orphaned_on_reload() {
    let path = tmp_path("orph");
    let reg = GraphWorkerRegistry::load(path.clone());
    let mut alive = fresh_info("gw_alive");
    alive.status = WorkerStatus::Reading;
    let mut done = fresh_info("gw_done");
    done.status = WorkerStatus::Done;
    done.done_at = Some(123);
    reg.register(alive);
    reg.register(done);
    drop(reg);

    assert!(codeterm_util::private_file::owner_only(&path).unwrap());
    let reg2 = GraphWorkerRegistry::load(path);
    let alive_after = reg2.get("gw_alive").unwrap();
    let done_after = reg2.get("gw_done").unwrap();
    assert_eq!(alive_after.status, WorkerStatus::Orphaned);
    assert_eq!(alive_after.done_at, None);
    assert_eq!(done_after.status, WorkerStatus::Done);
    assert_eq!(done_after.done_at, Some(123));
}

#[test]
fn load_isolates_malformed_worker_records_without_rewriting_the_store() {
    let path = tmp_path("malformed-record");
    let valid = fresh_info("gw_valid");
    let fixture = serde_json::json!([
        valid,
        {
            "worker_id": "gw_malformed",
            "status": ["not", "a", "status"]
        }
    ])
    .to_string();
    std::fs::write(&path, &fixture).unwrap();

    let registry = GraphWorkerRegistry::load(path.clone());
    let workers = registry.list();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].worker_id, "gw_valid");
    assert_eq!(workers[0].status, WorkerStatus::Orphaned);
    assert_eq!(std::fs::read_to_string(path).unwrap(), fixture);
}

#[test]
fn build_task_substitutes_fields() {
    let task = build_worker_task("dedup db", "gw_test", "merge dups");
    assert!(task.contains("Your intent: dedup db"));
    assert!(task.contains("Your worker id: gw_test"));
    assert!(task.contains("merge dups"));
    assert!(task.contains("domios mem worker report"));
}

#[test]
fn set_run_session_persists_session_and_cwd() {
    let reg = GraphWorkerRegistry::load(tmp_path("sess"));
    reg.register(fresh_info("gw_s"));
    reg.set_run_session("gw_s", "sid-123".into(), Some("/work/dir".into()));
    let w = reg.get("gw_s").unwrap();
    assert_eq!(w.session_id.as_deref(), Some("sid-123"));
    assert_eq!(w.cwd.as_deref(), Some("/work/dir"));
    // Missing worker is a no-op, not a panic.
    reg.set_run_session("gw_missing", "x".into(), None);
}

#[test]
fn worker_id_format() {
    let id = new_worker_id();
    assert!(id.starts_with("gw_"));
    assert_eq!(id.len(), 3 + 8);
}

#[test]
fn load_retains_pre_upgrade_record_carrying_pane_id_instead_of_tab_id() {
    let path = tmp_path("pre-upgrade-pane-id");
    let mut legacy_record = serde_json::to_value(fresh_info("gw_legacy")).unwrap();
    let legacy_tab_id = legacy_record
        .as_object_mut()
        .unwrap()
        .remove("tab_id")
        .unwrap();
    legacy_record["pane_id"] = legacy_tab_id;
    let fixture = serde_json::json!([legacy_record]).to_string();
    std::fs::write(&path, &fixture).unwrap();

    let registry = GraphWorkerRegistry::load(path.clone());
    let workers = registry.list();
    assert_eq!(
        workers.len(),
        1,
        "pre-upgrade record must be retained, not quarantined"
    );
    assert_eq!(workers[0].worker_id, "gw_legacy");
    assert_eq!(workers[0].tab_id, "tab1");
    assert_eq!(std::fs::read_to_string(path).unwrap(), fixture);
}

#[test]
fn active_by_intent_skips_terminal_workers() {
    let reg = GraphWorkerRegistry::load(tmp_path("intent"));

    let mut w1 = fresh_info("gw_run1");
    w1.intent = "dreaming:rem".into();
    w1.status = WorkerStatus::Reading;
    reg.register(w1);

    assert_eq!(
        reg.active_by_intent("dreaming:rem").as_deref(),
        Some("gw_run1")
    );
    assert!(reg.active_by_intent("extract:note_a").is_none());

    let _ = reg.complete("gw_run1", "done".into());
    assert!(reg.active_by_intent("dreaming:rem").is_none());

    let mut w2 = fresh_info("gw_run2");
    w2.intent = "extract:note_a".into();
    w2.status = WorkerStatus::Analyzing;
    reg.register(w2);
    assert_eq!(
        reg.active_by_intent("extract:note_a").as_deref(),
        Some("gw_run2")
    );
}
