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
fn preference_auto_extracted_from_note() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.save(NewMemItem {
        kind: MemKind::Note,
        scope: MemScope::Global,
        title: "user said".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: "I usually prefer PostgreSQL over MySQL for new projects".into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    })
    .unwrap();
    let stats = mem.stats().unwrap();
    assert!(stats.total_items >= 2, "expected ≥2 items, got {}", stats.total_items);
    let pref_count = stats.by_kind.get("preference").copied().unwrap_or(0);
    assert!(pref_count >= 1, "expected ≥1 preference, got {}", pref_count);
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
    assert!(ok.is_some(), "lock should be reacquirable after holder drops");
}
