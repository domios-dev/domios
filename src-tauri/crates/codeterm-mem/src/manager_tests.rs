use super::*;

#[test]
fn open_in_memory_works() {
    let mem = MemManager::open_in_memory().unwrap();
    let stats = mem.stats().unwrap();
    assert_eq!(stats.total_items, 0);
}

#[test]
fn save_and_get_roundtrip() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save(NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "test fact".into(),
            summary: Some("a test".into()),
            area: Some("test".into()),
            tags: vec!["tag1".into()],
            body: "the body of the fact".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    assert!(id.starts_with("fact_"));
    let got = mem.get(&id).unwrap().unwrap();
    assert_eq!(got.title, "test fact");
    assert_eq!(got.body, "the body of the fact");
}

#[test]
fn save_same_title_new_body_versions_in_place() {
    let mem = MemManager::open_in_memory().unwrap();
    let item = |body: &str| NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: "save version contract".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    };
    let first = mem.save_with_outcome(item("v1"), false).unwrap();
    let before = mem.get(&first.id).unwrap().unwrap();
    let updated =
        store::with_changed_by("agent:caller", || mem.save_with_outcome(item("v2"), false))
            .unwrap();
    let after = mem.get(&updated.id).unwrap().unwrap();
    assert_eq!(updated.outcome, "updated");
    assert_eq!(after.body, "v2");
    assert_eq!(after.uid, before.uid);
    assert_eq!(after.created_at, before.created_at);
    let history = mem.get_item_history(&updated.id).unwrap();
    let version = history
        .iter()
        .find(|entry| entry.old_body.as_deref() == Some("v1"))
        .unwrap();
    assert_eq!(version.changed_by, "agent:caller");
    assert_eq!(version.changed_fields, vec!["body"]);
}

#[test]
fn save_same_title_same_body_no_extract() {
    let mem = MemManager::open_in_memory().unwrap();
    let item = |body: &str| NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: "save unchanged contract".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    };
    let created = mem.save_with_outcome(item(" same  body "), false).unwrap();
    let unchanged = mem.save_with_outcome(item("same body"), false).unwrap();
    assert_eq!(unchanged.outcome, "unchanged");
    assert_eq!(unchanged.id, created.id);
    assert_eq!(mem.get(&created.id).unwrap().unwrap().recurrence_count, 1);
    assert_eq!(mem.get_item_history(&created.id).unwrap().len(), 1);
}

#[test]
fn replace_preserves_uid_and_history() {
    let mem = MemManager::open_in_memory().unwrap();
    let item = |body: &str| NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: "replace identity contract".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    };
    let first = mem.save_with_outcome(item("old"), false).unwrap();
    let uid = mem.get(&first.id).unwrap().unwrap().uid;
    let replaced = mem.save_with_outcome(item("new"), true).unwrap();
    assert_eq!(replaced.outcome, "updated");
    assert_eq!(mem.get(&first.id).unwrap().unwrap().uid, uid);
    assert!(mem
        .get_item_history(&first.id)
        .unwrap()
        .iter()
        .any(|entry| entry.old_body.as_deref() == Some("old")));
}

#[test]
fn mem_update_noop_reports_unchanged() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save(NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: "no-op update contract".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "same".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    let changed = mem
        .update(
            &id,
            store::MemUpdate {
                body: Some("same".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!changed);
    assert_eq!(mem.get_item_history(&id).unwrap().len(), 1);
}

#[test]
fn history_tracks_full_lifecycle_with_promote() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save(NewMemItem {
            kind: MemKind::Learning,
            scope: MemScope::Global,
            title: "lifecycle test".into(),
            summary: None,
            area: Some("test".into()),
            tags: vec![],
            body: "first cut".into(),
            source: None,
            importance: 2.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();

    mem.update(
        &id,
        store::MemUpdate {
            title: None,
            summary: None,
            area: None,
            tags: None,
            body: Some("refined body".into()),
            importance: Some(3.5),
            bump_recurrence: None,
            pinned: None,
        },
    )
    .unwrap();

    mem.promote_learning(&id).unwrap();

    let history = mem.get_item_history(&id).unwrap();
    let change_types: Vec<&str> = history.iter().map(|h| h.change_type.as_str()).collect();
    assert!(change_types.contains(&"create"));
    assert!(change_types.contains(&"update"));
    assert!(change_types.contains(&"promote"));
    let promote = history.iter().find(|h| h.change_type == "promote").unwrap();
    assert_eq!(promote.old_kind.as_deref(), Some("learning"));
    assert_eq!(promote.new_kind.as_deref(), Some("fact"));
}

#[test]
fn taxonomy_manager_aggregates_by_area_and_kind() {
    let mem = MemManager::open_in_memory().unwrap();
    for (title, kind, area) in [
        ("t1", MemKind::Fact, Some("deploy")),
        ("t2", MemKind::Note, Some("deploy")),
        ("t3", MemKind::Rule, None),
    ] {
        mem.save(NewMemItem {
            kind,
            scope: MemScope::Global,
            title: title.into(),
            summary: None,
            area: area.map(String::from),
            tags: vec![],
            body: "b".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    }
    let tax = mem.get_taxonomy(None).unwrap();
    assert!(tax.get("deploy").and_then(|m| m.get("fact")).is_some());
    assert!(tax.get("(none)").and_then(|m| m.get("rule")).is_some());
}

#[test]
fn try_conn_for_times_out_when_held_by_another_thread() {
    use std::sync::mpsc;
    use std::time::Duration;

    let mem = MemManager::open_in_memory().unwrap();
    let (block_tx, block_rx) = mpsc::channel::<()>();
    let (held_tx, held_rx) = mpsc::channel::<()>();

    let holder = {
        let mem = mem.clone();
        std::thread::spawn(move || {
            let _guard = mem.conn().lock();
            held_tx.send(()).unwrap();
            block_rx.recv().unwrap();
        })
    };
    held_rx.recv().unwrap();

    let start = std::time::Instant::now();
    let result = mem.try_conn_for(Duration::from_millis(50));
    let elapsed = start.elapsed();
    assert!(result.is_none(), "expected timeout, got Some guard");
    assert!(
        elapsed >= Duration::from_millis(40),
        "try_conn_for returned too early: {:?}",
        elapsed
    );

    block_tx.send(()).unwrap();
    holder.join().unwrap();

    let ok = mem.try_conn_for(Duration::from_millis(200));
    assert!(
        ok.is_some(),
        "lock should be reacquirable after holder drops"
    );
}
