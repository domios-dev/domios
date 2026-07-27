use std::collections::HashMap;

use crate::agent::chat_types::ChatMsg;
use crate::agent::reports::{AgentReport, ReportStatus};
use crate::agent::status::{TabStatus, TabStatusTracker};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;

use super::{
    build_observation, collect_watch_group, is_manager_watcher, observation_config_from_json,
    resolve_orchestrator_id, ObservationConfig, ObservationSources,
};

fn fixture_mux() -> (MultiplexerState, WatchFixtureIds) {
    let (mut mux, orch) = MultiplexerState::new();
    mux.rename_tab(&orch, "Orchestrator").unwrap();

    let planner = mux.create_tab();
    mux.set_tab_role(&planner, AgentRole::Manager, Some(&orch))
        .unwrap();
    mux.rename_tab(&planner, "Planner").unwrap();
    mux.get_tab_mut(&planner).unwrap().role_profile = Some("planner".into());

    let watcher = mux.create_tab();
    mux.set_tab_role(&watcher, AgentRole::Manager, Some(&orch))
        .unwrap();
    mux.rename_tab(&watcher, "Watcher-1").unwrap();
    mux.get_tab_mut(&watcher).unwrap().role_profile = Some("watcher".into());

    let watcher2 = mux.create_tab();
    mux.set_tab_role(&watcher2, AgentRole::Manager, Some(&orch))
        .unwrap();
    mux.rename_tab(&watcher2, "Watcher-2").unwrap();
    mux.get_tab_mut(&watcher2).unwrap().role_profile = Some("watcher".into());

    let worker_a = mux.create_tab();
    mux.set_tab_role(&worker_a, AgentRole::Worker, Some(&orch))
        .unwrap();
    mux.rename_tab(&worker_a, "Worker-A").unwrap();

    let worker_b = mux.create_tab();
    mux.set_tab_role(&worker_b, AgentRole::Worker, Some(&orch))
        .unwrap();
    mux.rename_tab(&worker_b, "Worker-B").unwrap();

    (
        mux,
        WatchFixtureIds {
            orch,
            planner,
            watcher,
            watcher2,
            worker_a,
            worker_b,
        },
    )
}

struct WatchFixtureIds {
    orch: String,
    planner: String,
    watcher: String,
    watcher2: String,
    worker_a: String,
    worker_b: String,
}

fn snapshot_with_idle(mux: &MultiplexerState) -> crate::multiplexer::state::MultiplexerSnapshot {
    let tracker = TabStatusTracker::new();
    mux.snapshot_with_status(&tracker)
}

#[test]
fn group_excludes_self_and_other_watchers() {
    let (mux, ids) = fixture_mux();
    let snap = snapshot_with_idle(&mux);
    let group = collect_watch_group(&snap, &ids.watcher).expect("group");
    assert_eq!(group.orchestrator_id, ids.orch);
    let set: std::collections::HashSet<_> = group.tab_ids.iter().cloned().collect();
    assert!(set.contains(&ids.orch));
    assert!(set.contains(&ids.planner));
    assert!(set.contains(&ids.worker_a));
    assert!(set.contains(&ids.worker_b));
    assert!(!set.contains(&ids.watcher));
    assert!(!set.contains(&ids.watcher2));
}

#[test]
fn parent_chain_resolves_orchestrator() {
    let (mux, ids) = fixture_mux();
    let snap = snapshot_with_idle(&mux);
    assert_eq!(
        resolve_orchestrator_id(&snap, &ids.watcher).as_deref(),
        Some(ids.orch.as_str())
    );
    assert_eq!(
        resolve_orchestrator_id(&snap, &ids.worker_a).as_deref(),
        Some(ids.orch.as_str())
    );
}

#[test]
fn is_manager_watcher_matches_profile_only() {
    let (mux, ids) = fixture_mux();
    let snap = snapshot_with_idle(&mux);
    assert!(is_manager_watcher(snap.tabs.get(&ids.watcher).unwrap()));
    assert!(!is_manager_watcher(snap.tabs.get(&ids.planner).unwrap()));
}

#[test]
fn build_observation_includes_expected_tabs() {
    let (mux, ids) = fixture_mux();
    let mut snap = snapshot_with_idle(&mux);
    snap.tab_statuses
        .insert(ids.worker_a.clone(), TabStatus::Working);

    let chat_store: HashMap<String, Vec<ChatMsg>> = HashMap::new();
    let activity = HashMap::from([(ids.worker_a.clone(), 9_000u64)]);
    let chat_tail = |tab_id: &str, n: usize| {
        chat_store
            .get(tab_id)
            .map(|v| v.iter().rev().take(n).rev().cloned().collect())
            .unwrap_or_default()
    };
    let sources = ObservationSources {
        chat_tail: &chat_tail,
        reports: &[],
        last_activity_ms: &activity,
    };
    let obs = build_observation(&snap, &ids.watcher, &ObservationConfig::default(), &sources);
    let tabs = obs["tabs"].as_array().expect("Tabs array");
    let tab_ids: Vec<_> = tabs
        .iter()
        .filter_map(|p| p.get("tab_id").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(tab_ids.len(), 4);
    assert!(tab_ids.contains(&ids.orch.as_str()));
    assert!(tab_ids.contains(&ids.planner.as_str()));
    assert!(tab_ids.contains(&ids.worker_a.as_str()));
    assert!(tab_ids.contains(&ids.worker_b.as_str()));
    assert!(!tab_ids.contains(&ids.watcher.as_str()));
    assert!(!tab_ids.contains(&ids.watcher2.as_str()));

    let worker = tabs
        .iter()
        .find(|p| p["tab_id"] == ids.worker_a)
        .expect("worker row");
    assert_eq!(worker["status"], "Working");
    assert_eq!(worker["last_activity_ms"], 9_000);
}

#[test]
fn chat_tail_attaches_last_n_messages() {
    let (mux, ids) = fixture_mux();
    let snap = snapshot_with_idle(&mux);
    let chat_store = HashMap::from([(
        ids.worker_a.clone(),
        vec![
            ChatMsg {
                id: "1".into(),
                kind: "user".into(),
                content: "one".into(),
                ..Default::default()
            },
            ChatMsg {
                id: "2".into(),
                kind: "assistant".into(),
                content: "two".into(),
                ..Default::default()
            },
            ChatMsg {
                id: "3".into(),
                kind: "assistant".into(),
                content: "three".into(),
                ..Default::default()
            },
        ],
    )]);
    let chat_tail = |tab_id: &str, n: usize| {
        chat_store
            .get(tab_id)
            .map(|msgs| {
                let start = msgs.len().saturating_sub(n);
                msgs[start..].to_vec()
            })
            .unwrap_or_default()
    };
    let sources = ObservationSources {
        chat_tail: &chat_tail,
        reports: &[],
        last_activity_ms: &HashMap::new(),
    };
    let config = ObservationConfig {
        orchestration: true,
        chat_tail: 2,
        reports: false,
    };
    let obs = build_observation(&snap, &ids.watcher, &config, &sources);
    let worker = obs["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["tab_id"] == ids.worker_a)
        .expect("worker");
    let tail = worker["chatTail"].as_array().expect("chatTail");
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0]["content"], "two");
    assert_eq!(tail[1]["content"], "three");
}

#[test]
fn reports_included_when_configured() {
    let (mux, ids) = fixture_mux();
    let snap = snapshot_with_idle(&mux);
    let reports = vec![AgentReport {
        id: "r1".into(),
        from_tab_id: ids.worker_a.clone(),
        from_title: "Worker-A".into(),
        message: "done".into(),
        timestamp: 42,
        status: Some(ReportStatus::Done),
        plan_id: None,
        task_id: None,
        branch: None,
        sha: None,
        changed_files: None,
        test_cmd: None,
        test_result: None,
        skipped: None,
        attachments: vec![],
        model: None,
        planned_model: None,
        requested_model: None,
        provider_resolved_model: None,
        actual_model: None,
        model_fallback: None,
        model_identity_mismatch: None,
    }];
    let chat_tail = |_: &str, _: usize| Vec::new();
    let sources = ObservationSources {
        chat_tail: &chat_tail,
        reports: &reports,
        last_activity_ms: &HashMap::new(),
    };
    let config = ObservationConfig {
        orchestration: false,
        chat_tail: 0,
        reports: true,
    };
    let obs = build_observation(&snap, &ids.watcher, &config, &sources);
    assert!(obs.get("tabs").is_none());
    let rows = obs["reports"].as_array().expect("reports");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["message"], "done");
}

#[test]
fn observation_config_from_json_defaults() {
    let cfg = observation_config_from_json(&serde_json::json!({}));
    assert!(cfg.orchestration);
    assert_eq!(cfg.chat_tail, 0);
    assert!(!cfg.reports);
}

#[test]
fn observation_config_from_json_reads_fields() {
    let cfg = observation_config_from_json(&serde_json::json!({
        "orchestration": false,
        "chatTail": 5,
        "reports": true
    }));
    assert!(!cfg.orchestration);
    assert_eq!(cfg.chat_tail, 5);
    assert!(cfg.reports);
}
