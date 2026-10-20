use super::*;

#[test]
fn persists_and_rehydrates_active_workers_across_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote_workers.json");
    let now = now_ms_epoch();
    {
        let reg = RemoteWorkerRegistry::load(path.clone());
        reg.record_spawn("orch", "eight", "w1", "Canvas", Some("opus".into()), now);
        reg.record_report(
            "orch",
            "eight",
            "w2",
            "Renderer",
            None,
            "STATUS: working",
            now,
        );
    } // drop — simulates daemon shutdown
    assert!(path.exists(), "registry persisted to disk");

    // Fresh registry from the same path = post-restart rehydrate.
    let reloaded = RemoteWorkerRegistry::load(path);
    let list = reloaded.for_orchestrator("orch");
    assert_eq!(
        list.len(),
        2,
        "active remote workers survive restart without a new report"
    );
    let canvas = list.iter().find(|w| w.remote_tab == "w1").unwrap();
    assert_eq!(canvas.peer, "eight");
    assert_eq!(canvas.status, "working");
    assert_eq!(canvas.model.as_deref(), Some("opus"));
    let renderer = list.iter().find(|w| w.remote_tab == "w2").unwrap();
    assert_eq!(renderer.status, "working");
}

#[test]
fn load_drops_stale_entries_so_restart_restores_only_recent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote_workers.json");
    let now = now_ms_epoch();
    {
        let reg = RemoteWorkerRegistry::load(path.clone());
        // One fresh, one already older than STALE_MS.
        reg.record_report("orch", "eight", "fresh", "F", None, "working", now);
        reg.record_report(
            "orch",
            "eight",
            "old",
            "O",
            None,
            "working",
            now.saturating_sub(STALE_MS + 1),
        );
    }
    let reloaded = RemoteWorkerRegistry::load(path);
    let list = reloaded.for_orchestrator("orch");
    assert_eq!(list.len(), 1, "stale ghost evicted on rehydrate");
    assert_eq!(list[0].remote_tab, "fresh");
}

#[test]
fn load_removes_completed_entries_in_memory_without_rewriting_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote_workers.json");
    let completed = RemoteWorker {
        peer: "eight".into(),
        remote_tab: "done".into(),
        title: "Done".into(),
        model: None,
        status: "done".into(),
        last_message: "STATUS: done".into(),
        last_seen_ms: now_ms_epoch(),
    };
    let fixture =
        serde_json::to_string(&HashMap::from([("orch".to_string(), vec![completed])])).unwrap();
    std::fs::write(&path, &fixture).unwrap();

    let registry = RemoteWorkerRegistry::load(path.clone());
    assert!(registry.for_orchestrator("orch").is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), fixture);
}

#[test]
fn in_memory_registry_writes_nothing() {
    // new() has no path → pure in-memory, no disk I/O.
    let reg = RemoteWorkerRegistry::new();
    reg.record_spawn("orch", "eight", "w1", "W", None, now_ms_epoch());
    assert_eq!(reg.for_orchestrator("orch").len(), 1);
}

#[test]
fn status_parsing_picks_lifecycle_token() {
    assert_eq!(status_from_body("STATUS: done | shipped"), "done");
    assert_eq!(status_from_body("status:blocked, need creds"), "blocked");
    assert_eq!(status_from_body("STATUS: partial"), "partial");
    assert_eq!(status_from_body("just working on it"), "working");
}

#[test]
fn completed_report_removes_worker_from_active_registry() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_spawn("orch", "eight", "w1", "Renderer", None, 100);
    reg.record_report("orch", "eight", "w1", "Renderer", None, "STATUS: done", 101);
    assert!(reg.for_orchestrator("orch").is_empty());
}

#[test]
fn blocked_and_partial_reports_remain_actionable() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_report(
        "orch",
        "eight",
        "blocked",
        "Blocked",
        None,
        "STATUS: blocked",
        100,
    );
    reg.record_report(
        "orch",
        "eight",
        "partial",
        "Partial",
        None,
        "STATUS: partial",
        101,
    );
    let workers = reg.for_orchestrator("orch");
    assert_eq!(workers.len(), 2);
    assert!(workers.iter().any(|worker| worker.status == "blocked"));
    assert!(workers.iter().any(|worker| worker.status == "partial"));
}

#[test]
fn upsert_replaces_same_worker_and_keeps_distinct() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_report(
        "orch",
        "eight",
        "w1",
        "Worker-1",
        None,
        "STATUS: working",
        10,
    );
    reg.record_report(
        "orch",
        "eight",
        "w1",
        "Worker-1",
        Some("opus".into()),
        "STATUS: partial",
        20,
    );
    reg.record_report("orch", "eight", "w2", "Worker-2", None, "working", 15);

    let list = reg.for_orchestrator("orch");
    assert_eq!(list.len(), 2, "two distinct remote Tabs");
    // Newest activity first.
    assert_eq!(list[0].remote_tab, "w1");
    assert_eq!(
        list[0].status, "partial",
        "same worker upserts, not duplicates"
    );
    assert_eq!(list[0].model.as_deref(), Some("opus"));
    assert_eq!(list[1].remote_tab, "w2");
}

#[test]
fn stale_entries_and_empty_buckets_are_evicted() {
    let reg = RemoteWorkerRegistry::new();
    // Old worker under orch-a; a fresh report elsewhere triggers the sweep.
    reg.record_report("orch-a", "eight", "w1", "W", None, "working", 1);
    let fresh = super::STALE_MS + 1_000;
    reg.record_report("orch-b", "eight", "w2", "W", None, "working", fresh);
    assert!(
        reg.for_orchestrator("orch-a").is_empty(),
        "stale worker + its empty bucket must be evicted"
    );
    assert_eq!(reg.for_orchestrator("orch-b").len(), 1);
}

#[test]
fn record_spawn_inserts_pending_worker_then_report_upgrades_without_clobber() {
    let reg = RemoteWorkerRegistry::new();
    // Spawn registers a pending worker before any report.
    reg.record_spawn("orch", "eight", "w1", "Canvas", Some("opus".into()), 100);
    let list = reg.for_orchestrator("orch");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].remote_tab, "w1");
    assert_eq!(list[0].status, "working");
    assert_eq!(list[0].model.as_deref(), Some("opus"));

    // A terminal report removes the completed worker from the active registry.
    reg.record_report(
        "orch",
        "eight",
        "w1",
        "Canvas",
        Some("opus".into()),
        "STATUS: done",
        200,
    );
    let list = reg.for_orchestrator("orch");
    assert!(
        list.is_empty(),
        "completed workers leave the active registry"
    );

    // A later spawn represents a fresh worker lifecycle for the tab id.
    reg.record_spawn("orch", "eight", "w1", "Canvas", None, 300);
    let list = reg.for_orchestrator("orch");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].status, "working");
}

#[test]
fn forget_orchestrator_drops_bucket() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_report("orch", "eight", "w1", "W", None, "working", 1);
    reg.forget_orchestrator("orch");
    assert!(reg.for_orchestrator("orch").is_empty());
}

#[test]
fn forget_by_remote_tab_removes_ghost_and_empty_bucket() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_spawn("orch", "rollacode-bot", "6d2e36b4", "Main", None, 100);
    reg.record_spawn("orch", "rollacode-bot", "aaaa1111", "Other", None, 101);

    let touched = reg.forget_by_remote_tab("6d2e36b4", None, None);
    assert_eq!(touched, vec!["orch".to_string()]);
    let list = reg.for_orchestrator("orch");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].remote_tab, "aaaa1111");
}

#[test]
fn forget_by_remote_tab_respects_peer_filter() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_spawn("orch", "peer-a", "w1", "A", None, 1);
    reg.record_spawn("orch", "peer-b", "w1", "B", None, 2);

    let touched = reg.forget_by_remote_tab("w1", Some("peer-a"), None);
    assert_eq!(touched, vec!["orch".to_string()]);
    let list = reg.for_orchestrator("orch");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].peer, "peer-b");
}

#[test]
fn forget_by_remote_tab_scopes_to_one_orchestrator() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_spawn("orch-a", "peer", "w1", "W", None, 1);
    reg.record_spawn("orch-b", "peer", "w1", "W", None, 2);

    let touched = reg.forget_by_remote_tab("w1", None, Some("orch-a"));
    assert_eq!(touched, vec!["orch-a".to_string()]);
    assert!(reg.for_orchestrator("orch-a").is_empty());
    assert_eq!(reg.for_orchestrator("orch-b").len(), 1);
}

#[test]
fn forget_by_remote_tab_no_match_returns_empty() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_spawn("orch", "peer", "w1", "W", None, 1);
    assert!(reg.forget_by_remote_tab("missing", None, None).is_empty());
    assert_eq!(reg.for_orchestrator("orch").len(), 1);
}

#[test]
fn distinct_orchestrators_are_isolated() {
    let reg = RemoteWorkerRegistry::new();
    reg.record_report("orch-a", "eight", "w1", "W", None, "working", 1);
    reg.record_report("orch-b", "eight", "w2", "W", None, "working", 1);
    assert_eq!(reg.for_orchestrator("orch-a").len(), 1);
    assert_eq!(reg.for_orchestrator("orch-b").len(), 1);
    assert!(reg.for_orchestrator("orch-c").is_empty());
}

#[test]
fn load_normalizes_pane_era_workers_and_isolates_malformed_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote_workers.json");
    let fixture = serde_json::json!({
        "orch-tab": [
            {
                "peer": "peer-a",
                "remote_pane": "worker-tab",
                "title": "Worker",
                "model": "model",
                "status": "working",
                "last_message": "still working",
                "last_seen_ms": now_ms_epoch()
            },
            {
                "remote_pane": ["invalid"],
                "status": "working"
            }
        ]
    })
    .to_string();
    std::fs::write(&path, &fixture).unwrap();

    let registry = RemoteWorkerRegistry::load(path.clone());
    let workers = registry.for_orchestrator("orch-tab");
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].remote_tab, "worker-tab");
    assert_eq!(std::fs::read_to_string(path).unwrap(), fixture);
}
