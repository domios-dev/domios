use super::*;
use crate::mem::store::{init_schema, insert, MemKind, MemScope, NewMemItem};
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
        source: None, importance: 5.0, source_path: None,
    }, &blobs).unwrap();
    insert(&conn, &NewMemItem {
        kind: MemKind::Rule, scope: MemScope::Global,
        title: "never edit autogen".into(), summary: None, area: None, tags: vec![],
        body: "Never edit CLAUDE.md or AGENTS.md".into(),
        source: None, importance: 5.0, source_path: None,
    }, &blobs).unwrap();
    insert(&conn, &NewMemItem {
        kind: MemKind::Peer, scope: MemScope::Global,
        title: "ainyiva-bot".into(), summary: Some("mac mini, paired".into()), area: None,
        tags: vec![], body: "ainyiva-bot is the Mac mini peer".into(),
        source: None, importance: 4.0, source_path: None,
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
            body: "x".repeat(200), source: None, importance: 4.5, source_path: None,
        }, &blobs).unwrap();
    }
    insert(&conn, &NewMemItem {
        kind: MemKind::Rule, scope: MemScope::Global,
        title: "important rule".into(), summary: None, area: None, tags: vec![],
        body: "this rule must always survive".into(), source: None, importance: 5.0, source_path: None,
    }, &blobs).unwrap();

    let out = generate(&conn, &WakeUpOptions {
        l0_budget_tokens: 50, l1_budget_tokens: 100,
        include_l0: false, include_l1: true,
    }, &blobs).unwrap();
    assert!(out.contains("important rule"), "rule must survive eviction: {}", out);
    assert!(out.contains("dropped"));
}
