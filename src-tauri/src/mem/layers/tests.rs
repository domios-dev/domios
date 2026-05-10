use super::*;
use crate::mem::store::{init_schema, insert, update, MemKind, MemScope, MemUpdate, NewMemItem};
use rusqlite::Connection;
use std::path::PathBuf;

fn fresh() -> (Connection, PathBuf) {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    let tmp = std::env::temp_dir().join(format!("memlay-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&tmp).unwrap();
    (conn, tmp)
}

#[test]
fn wake_up_includes_l0_l1() {
    let (conn, blobs) = fresh();
    insert(&conn, &NewMemItem {
        kind: MemKind::Preference, scope: MemScope::Global,
        title: "identity".into(), summary: None, area: None, tags: vec![],
        body: "I am Rollo, the GA on this machine.".into(),
        source: None, importance: 5.0, source_path: None, pinned: false,
    }, &blobs).unwrap();
    insert(&conn, &NewMemItem {
        kind: MemKind::Rule, scope: MemScope::Global,
        title: "never edit autogen".into(), summary: None, area: None, tags: vec![],
        body: "Never edit CLAUDE.md or AGENTS.md".into(),
        source: None, importance: 5.0, source_path: None, pinned: false,
    }, &blobs).unwrap();
    insert(&conn, &NewMemItem {
        kind: MemKind::Peer, scope: MemScope::Global,
        title: "build-mac".into(), summary: Some("mac mini, paired".into()), area: None,
        tags: vec![], body: "build-mac is the Mac mini peer".into(),
        source: None, importance: 4.0, source_path: None, pinned: false,
    }, &blobs).unwrap();

    let out = generate(&conn, &WakeUpOptions::default(), &blobs).unwrap();
    assert!(out.contains("L0 — Identity"));
    assert!(out.contains("Rollo"));
    assert!(out.contains("L1 — Critical context"));
    assert!(out.contains("Rules"));
    assert!(out.contains("never edit autogen"));
    assert!(out.contains("Peers"));
}

#[test]
fn wake_up_default_when_no_identity() {
    let (conn, blobs) = fresh();
    let out = generate(&conn, &WakeUpOptions::default(), &blobs).unwrap();
    assert!(out.contains("No identity configured"));
}

#[test]
fn budget_drops_low_priority_first() {
    let (conn, blobs) = fresh();
    // Big summary that won't fit
    for i in 0..20 {
        insert(&conn, &NewMemItem {
            kind: MemKind::Summary, scope: MemScope::Global,
            title: format!("summary {}", i), summary: None, area: None, tags: vec![],
            body: "x".repeat(200), source: None, importance: 4.5, source_path: None, pinned: false,
        }, &blobs).unwrap();
    }
    insert(&conn, &NewMemItem {
        kind: MemKind::Rule, scope: MemScope::Global,
        title: "important rule".into(), summary: None, area: None, tags: vec![],
        body: "this rule must always survive".into(), source: None, importance: 5.0, source_path: None, pinned: false,
    }, &blobs).unwrap();

    let out = generate(&conn, &WakeUpOptions {
        l0_budget_tokens: 50, l1_budget_tokens: 100,
        include_l0: false, include_l1: true,
        ..Default::default()
    }, &blobs).unwrap();
    assert!(out.contains("important rule"), "rule must survive eviction: {}", out);
    assert!(out.contains("dropped"));
}

#[test]
fn score_fresh_item_just_importance_plus_log_baseline() {
    use super::score::score_for;
    let s = score_for(3.0, 1, false, false);
    assert!((s - 3.151).abs() < 0.01, "got {s}");
}

#[test]
fn score_evidence_compounds_with_diminishing_returns() {
    use super::score::score_for;
    let baseline = score_for(3.0, 1, false, false);
    let s10 = score_for(3.0, 10, false, false);
    let s1000 = score_for(3.0, 1000, false, false);
    assert!(s10 > baseline + 0.3, "10 evidence should add at least +0.3 over baseline (got s10={s10}, baseline={baseline})");
    assert!(s1000 > s10 + 0.7 && s1000 < s10 + 1.5, "1000 should be sub-linear over 10 (got s10={s10}, s1000={s1000})");
}

#[test]
fn score_ws_match_adds_half() {
    use super::score::score_for;
    let no_ws = score_for(3.0, 1, false, false);
    let with_ws = score_for(3.0, 1, true, false);
    assert!((with_ws - no_ws - 0.5).abs() < 0.001);
}

#[test]
fn score_recency_adds_three_tenths() {
    use super::score::score_for;
    let no_recency = score_for(3.0, 1, false, false);
    let recent = score_for(3.0, 1, false, true);
    assert!((recent - no_recency - 0.3).abs() < 0.001);
}

fn insert_item(conn: &Connection, blobs: &PathBuf, title: &str, kind: &str, importance: f64, pinned: bool, scope: &str) -> String {
    let mem_kind = match kind {
        "rule" => MemKind::Rule, "preference" => MemKind::Preference, "peer" => MemKind::Peer,
        "fact" => MemKind::Fact, "summary" => MemKind::Summary,
        "skill" => MemKind::Skill, "learning" => MemKind::Learning,
        _ => MemKind::Fact,
    };
    let mem_scope = if scope == "global" {
        MemScope::Global
    } else {
        MemScope::Workspace(scope.strip_prefix("workspace:").unwrap_or(scope).to_string())
    };
    let id = insert(conn, &NewMemItem {
        kind: mem_kind, scope: mem_scope,
        title: title.to_string(), summary: None, area: None, tags: vec![],
        body: format!("body of {title}"), source: None, importance, source_path: None, pinned: false,
    }, blobs).unwrap();
    if pinned {
        update(conn, &id, &MemUpdate { pinned: Some(true), ..Default::default() }, blobs).unwrap();
    }
    id
}

#[test]
fn pinned_item_loads_regardless_of_importance() {
    let (conn, blobs) = fresh();
    insert_item(&conn, &blobs, "fact_pinned", "fact", 1.0, true, "global");
    insert_item(&conn, &blobs, "fact_high",   "fact", 5.0, false, "global");
    let opts = WakeUpOptions { include_l0: false, include_l1: true, ..Default::default() };
    let out = generate(&conn, &opts, &blobs).unwrap();
    assert!(out.contains("fact_pinned"), "pinned must appear regardless of low importance: {out}");
}

#[test]
fn workspace_match_outranks_global_at_margin() {
    let (conn, blobs) = fresh();
    insert_item(&conn, &blobs, "ws_a",     "fact", 3.0, false, "workspace:W1");
    insert_item(&conn, &blobs, "global_a", "fact", 3.0, false, "global");
    let opts = WakeUpOptions {
        include_l0: false, include_l1: true,
        current_workspace: Some("W1".into()),
        ..Default::default()
    };
    let out = generate(&conn, &opts, &blobs).unwrap();
    let pos_ws = out.find("ws_a").expect("ws_a present");
    let pos_g  = out.find("global_a").expect("global_a present");
    assert!(pos_ws < pos_g, "ws-scoped item must rank ahead of global at same importance");
}

#[test]
fn other_workspace_items_excluded() {
    let (conn, blobs) = fresh();
    insert_item(&conn, &blobs, "ws_a", "fact", 5.0, false, "workspace:W1");
    insert_item(&conn, &blobs, "ws_b", "fact", 5.0, false, "workspace:W2");
    let opts = WakeUpOptions {
        include_l0: false, include_l1: true,
        current_workspace: Some("W1".into()),
        ..Default::default()
    };
    let out = generate(&conn, &opts, &blobs).unwrap();
    assert!(out.contains("ws_a"));
    assert!(!out.contains("ws_b"), "items from other workspace must not load: {out}");
}

#[test]
fn pinned_section_marks_items_with_star() {
    let (conn, blobs) = fresh();
    insert_item(&conn, &blobs, "fact_pinned", "fact", 3.0, true, "global");
    let opts = WakeUpOptions { include_l0: false, include_l1: true, ..Default::default() };
    let out = generate(&conn, &opts, &blobs).unwrap();
    assert!(out.contains("★ fact_pinned"), "must mark pinned with ★ before title: {out}");
}

#[test]
fn pinned_set_over_budget_emits_warning() {
    let (conn, blobs) = fresh();
    for i in 0..30 {
        let id = format!("pinned_{i}");
        insert_item(&conn, &blobs, &id, "fact", 3.0, true, "global");
    }
    let opts = WakeUpOptions {
        include_l0: false, include_l1: true,
        l1_budget_tokens: 100,
        ..Default::default()
    };
    let out = generate(&conn, &opts, &blobs).unwrap();
    assert!(out.contains("WARN:") && out.contains("pinned"),
        "must surface eviction warning: {out}");
}

#[test]
fn wake_up_without_workspace_includes_globals_only() {
    let (conn, blobs) = fresh();
    insert_item(&conn, &blobs, "global_a", "fact", 5.0, false, "global");
    insert_item(&conn, &blobs, "ws_a",     "fact", 5.0, false, "workspace:W1");
    let opts = WakeUpOptions { include_l0: false, include_l1: true, ..Default::default() };
    let out = generate(&conn, &opts, &blobs).unwrap();
    assert!(out.contains("global_a"));
    assert!(!out.contains("ws_a"), "no workspace context = no workspace items");
}

#[test]
fn updated_at_30_day_boundary_recency_bonus() {
    let (conn, blobs) = fresh();
    insert_item(&conn, &blobs, "recent_a", "fact", 3.0, false, "global");
    let stale_id = insert_item(&conn, &blobs, "stale_a", "fact", 3.0, false, "global");
    conn.execute(
        "UPDATE mem_items SET updated_at = datetime('now','-31 days') WHERE id = ?1",
        rusqlite::params![stale_id]
    ).unwrap();
    let opts = WakeUpOptions { include_l0: false, include_l1: true, ..Default::default() };
    let out = generate(&conn, &opts, &blobs).unwrap();
    let pos_recent = out.find("recent_a").expect("recent_a present");
    let pos_stale = out.find("stale_a").expect("stale_a present");
    assert!(pos_recent < pos_stale, "recency bonus must place recent before stale (got recent@{pos_recent} stale@{pos_stale}): {out}");
}
