//! Unit tests for the pure, `ServiceContext`-free core of the compute base:
//! the SDK-shape mappers and opts parsing. `ServiceContext` cannot be built in
//! a unit test (needs a `tauri::AppHandle` + ~40 fields), so the live
//! `host.worker`/`host.agent` round-trip is proven at integration (Track F /
//! Track V). The svc-absent envelope + the preamble Promise/sync split are
//! proven by JS-eval tests in `vm.rs`.

use super::*;
use crate::mem::workers::make_info;
use crate::mem::MemItem;

fn fake_item(id: &str, body: &str) -> MemItem {
    MemItem {
        id: id.to_string(),
        uid: "uid".to_string(),
        kind: "fact".to_string(),
        scope: "global".to_string(),
        title: "t".to_string(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.to_string(),
        source: None,
        importance: 1.0,
        created_at: "now".to_string(),
        updated_at: "now".to_string(),
        accessed_at: None,
        access_count: 0,
        recurrence_count: 0,
    }
}

fn worker(id: &str) -> GraphWorkerInfo {
    make_info(
        id.to_string(),
        "pane1".to_string(),
        "plugin:p:worker:x".to_string(),
        "plugin:p".to_string(),
        "hsl(0,0%,0%)".to_string(),
    )
}

#[test]
fn search_hits_map_to_id_text_score() {
    let hits = vec![
        SearchHit {
            item: fake_item("mem_a", "alpha body"),
            score: 0.9,
        },
        SearchHit {
            item: fake_item("mem_b", "beta body"),
            score: 0.5,
        },
    ];
    let v = search_hits_json(&hits);
    let arr = v["hits"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], "mem_a");
    assert_eq!(arr[0]["text"], "alpha body");
    assert_eq!(arr[0]["score"], 0.9);
    assert_eq!(arr[1]["id"], "mem_b");
    assert_eq!(arr[1]["text"], "beta body");
}

#[test]
fn search_hits_empty_is_empty_array() {
    let v = search_hits_json(&[]);
    assert_eq!(v["hits"].as_array().unwrap().len(), 0);
}

#[test]
fn worker_poll_unknown_job_is_done_with_error() {
    let v = worker_poll_json(None);
    assert_eq!(v["done"], true);
    assert_eq!(v["error"], "job not found");
    assert!(v.get("report").is_none());
}

#[test]
fn worker_poll_running_is_not_done() {
    let mut w = worker("gw_run");
    w.status = WorkerStatus::Reading;
    let v = worker_poll_json(Some(&w));
    assert_eq!(v["done"], false);
    assert!(v.get("report").is_none());
    assert!(v.get("error").is_none());
}

#[test]
fn worker_poll_done_carries_report() {
    let mut w = worker("gw_done");
    w.status = WorkerStatus::Done;
    w.report = Some("the result".to_string());
    let v = worker_poll_json(Some(&w));
    assert_eq!(v["done"], true);
    assert_eq!(v["report"], "the result");
    assert!(v.get("error").is_none());
}

#[test]
fn worker_poll_error_status_maps_to_error() {
    let mut w = worker("gw_err");
    w.status = WorkerStatus::Error;
    let v = worker_poll_json(Some(&w));
    assert_eq!(v["done"], true);
    assert!(v["error"].as_str().unwrap().contains("Error"));
    assert!(v.get("report").is_none());
}

#[test]
fn agent_poll_done_uses_reply_not_report() {
    let mut w = worker("gw_a");
    w.status = WorkerStatus::Done;
    w.report = Some("npc speaks".to_string());
    let v = agent_poll_json(Some(&w));
    assert_eq!(v["done"], true);
    assert_eq!(v["reply"], "npc speaks");
    // agent poll never exposes a `report` key — the SDK names it `reply`.
    assert!(v.get("report").is_none());
}

#[test]
fn agent_poll_running_is_not_done() {
    let mut w = worker("gw_a2");
    w.status = WorkerStatus::Analyzing;
    let v = agent_poll_json(Some(&w));
    assert_eq!(v["done"], false);
    assert!(v.get("reply").is_none());
}

#[test]
fn backend_defaults_to_claude_and_empty_model() {
    let (p, m) = backend_provider_model(&None);
    assert_eq!(p, "claude");
    assert_eq!(m, "");
}

#[test]
fn backend_honours_explicit_provider_and_model() {
    let b = Some(Backend {
        provider: Some("codex".to_string()),
        model: Some("gpt-5".to_string()),
    });
    let (p, m) = backend_provider_model(&b);
    assert_eq!(p, "codex");
    assert_eq!(m, "gpt-5");
}

#[test]
fn agent_resume_echoes_session_id() {
    let v: serde_json::Value = serde_json::from_str(&agent_resume_json("sess-7")).unwrap();
    assert_eq!(v["sessionId"], "sess-7");
}

#[test]
fn agent_get_is_null_host_keeps_no_key_map() {
    assert_eq!(agent_get_json(), "null");
}

#[test]
fn agent_usage_is_null_until_wired() {
    assert_eq!(agent_usage_json(), "null");
}
