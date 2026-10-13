use super::*;

fn tab(tab_id: &str, ws: &str, title: &str, closed_at: u64) -> ClosedTab {
    ClosedTab {
        tab_id: tab_id.into(),
        parent_tab_id: None,
        title: title.into(),
        description: None,
        backing: TabBacking::Pty,
        cwd: None,
        agent_type: None,
        agent_session_id: None,
        role_profile: None,
        worker_id: None,
        model: None,
        restorable_launch: Default::default(),
        chat_view: false,
        closed_at,
        workspace_id: ws.into(),
        workspace_name: "WS".into(),
    }
}

fn group(id: &str, kind: GroupKind, ws: &str, closed_at: u64) -> ClosedGroup {
    ClosedGroup {
        group_id: id.into(),
        kind,
        label: "".into(),
        workspace_id: ws.into(),
        workspace_name: "WS".into(),
        workspace_cwd: None,
        closed_at,
    }
}

fn temp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "closed-test-{}/closed_history.db",
        ulid::Ulid::new()
    ))
}

#[test]
fn insert_then_list_newest_first() {
    let store = ClosedTabsStore::in_memory();
    store
        .insert_group(
            &group("g1", GroupKind::Single, "w", 100),
            &[tab("p1", "w", "first", 100)],
        )
        .unwrap();
    store
        .insert_group(
            &group("g2", GroupKind::Single, "w", 200),
            &[tab("p2", "w", "second", 200)],
        )
        .unwrap();
    let list = store.list_for_workspace("w");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].title, "second", "newest first");
    assert_eq!(list[1].title, "first");
}

#[test]
fn per_workspace_cap_drops_oldest() {
    let cap = 3;
    let store = ClosedTabsStore::in_memory_with_caps(cap, CLOSED_GROUP_CAP);
    store
        .insert_group(
            &group("other-group", GroupKind::Single, "other", 0),
            &[tab("other-tab", "other", "other", 0)],
        )
        .unwrap();
    for i in 0..(cap + 2) {
        store
            .insert_group(
                &group(&format!("g{i}"), GroupKind::Single, "w", i as u64),
                &[tab(&format!("p{i}"), "w", "t", i as u64)],
            )
            .unwrap();
    }
    assert_eq!(store.count_for_workspace("w"), cap);
    let ids: Vec<_> = store
        .list_for_workspace("w")
        .into_iter()
        .map(|tab| tab.tab_id)
        .collect();
    assert_eq!(ids, ["p4", "p3", "p2"]);
    assert_eq!(store.list_for_workspace("other")[0].tab_id, "other-tab");
}

#[test]
fn take_in_workspace_fetches_and_removes() {
    let store = ClosedTabsStore::in_memory();
    store
        .insert_group(
            &group("g1", GroupKind::Single, "w", 1),
            &[tab("older", "w", "older", 1)],
        )
        .unwrap();
    store
        .insert_group(
            &group("g2", GroupKind::Single, "w", 2),
            &[tab("newer", "w", "newer", 2)],
        )
        .unwrap();
    let taken = store.take_in_workspace("w", 1).unwrap();
    assert_eq!(taken.tab_id, "older");
    let rest = store.list_for_workspace("w");
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].tab_id, "newer");
    assert!(store.take_group("g1").is_none());
}

#[test]
fn take_out_of_range_returns_none() {
    let store = ClosedTabsStore::in_memory();
    store
        .insert_group(
            &group("g1", GroupKind::Single, "w", 1),
            &[tab("p", "w", "t", 1)],
        )
        .unwrap();
    assert!(store.take_in_workspace("w", 5).is_none());
}

#[test]
fn take_group_returns_meta_and_all_tabs() {
    let store = ClosedTabsStore::in_memory();
    let mut root = tab("root", "w", "Orchestrator", 10);
    let mut child = tab("child", "w", "Worker", 10);
    child.parent_tab_id = Some("root".into());
    root.parent_tab_id = None;
    store
        .insert_group(&group("g1", GroupKind::Cascade, "w", 10), &[root, child])
        .unwrap();
    let (g, tabs) = store.take_group("g1").unwrap();
    assert_eq!(g.kind, GroupKind::Cascade);
    assert_eq!(tabs.len(), 2);
    assert!(tabs
        .iter()
        .any(|p| p.parent_tab_id.as_deref() == Some("root")));
    assert!(store.list_for_workspace("w").is_empty());
}

#[test]
fn peek_group_is_non_destructive_until_delete() {
    let store = ClosedTabsStore::in_memory();
    let root = tab("root", "w", "Orchestrator", 10);
    let mut child = tab("child", "w", "Worker", 10);
    child.parent_tab_id = Some("root".into());
    store
        .insert_group(&group("g1", GroupKind::Cascade, "w", 10), &[root, child])
        .unwrap();

    // Peeking (what reopen does first) does NOT consume the group.
    let (_g, tabs) = store.peek_group("g1").expect("group present");
    assert_eq!(tabs.len(), 2);
    assert_eq!(
        store.peek_group("g1").map(|(_, p)| p.len()),
        Some(2),
        "a second peek still sees the group — a failed materialize loses nothing"
    );
    assert_eq!(
        store.list_for_workspace("w").len(),
        2,
        "both Tabs remain in history until delete"
    );

    // Only an explicit delete (after a fully successful materialize) drops it.
    store.delete_group("g1").expect("delete group succeeds");
    assert!(
        store.peek_group("g1").is_none(),
        "group consumed only after delete_group"
    );
    assert!(store.list_for_workspace("w").is_empty());
}

#[test]
fn peek_in_workspace_then_delete_row_is_a_split_take() {
    let store = ClosedTabsStore::in_memory();
    store
        .insert_group(
            &group("g1", GroupKind::Single, "w", 1),
            &[tab("p1", "w", "kept", 1)],
        )
        .unwrap();
    let (row_id, peeked) = store.peek_in_workspace("w", 0).expect("peeked");
    assert_eq!(peeked.tab_id, "p1");
    assert_eq!(
        store.list_for_workspace("w").len(),
        1,
        "peek leaves the Tab in history"
    );
    store.delete_tab_row(row_id).expect("row deleted");
    assert!(
        store.list_for_workspace("w").is_empty(),
        "delete_tab_row removes only after success"
    );
}

#[test]
fn delete_reports_failure_instead_of_swallowing() {
    let store = ClosedTabsStore::in_memory();
    store
        .insert_group(
            &group("g1", GroupKind::Single, "w", 1),
            &[tab("p1", "w", "t", 1)],
        )
        .unwrap();
    let (row_id, _) = store.peek_in_workspace("w", 0).expect("peeked");

    // First delete removes the row and reports success.
    store.delete_tab_row(row_id).expect("first delete succeeds");
    // Second delete of the same id removes nothing → Err (entry already gone).
    assert!(
        store.delete_tab_row(row_id).is_err(),
        "deleting an absent row must report failure, not silently 'succeed'"
    );
    // Deleting an absent group still succeeds (idempotent: nothing to retry).
    assert!(store.delete_group("nope").is_ok());
}

#[test]
fn list_recent_carries_group_kind() {
    let store = ClosedTabsStore::in_memory();
    store
        .insert_group(
            &group("g1", GroupKind::Workspace, "w", 5),
            &[tab("p1", "w", "t", 5)],
        )
        .unwrap();
    let recent = store.list_recent(10);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].group_kind, GroupKind::Workspace);
    assert_eq!(recent[0].group_id, "g1");
}

#[test]
fn backing_round_trips_through_sqlite() {
    let store = ClosedTabsStore::in_memory();
    let mut p = tab("p1", "w", "Chat", 1);
    p.backing = TabBacking::ChatBackend {
        plugin_id: "lmstudio".into(),
        session_id: "s7".into(),
    };
    store
        .insert_group(&group("g1", GroupKind::Single, "w", 1), &[p])
        .unwrap();
    let back = store.list_for_workspace("w");
    assert_eq!(
        back[0].backing,
        TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        }
    );
}

#[test]
fn v1_database_migrates_with_terminal_mode_default() {
    let db = temp_db();
    {
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE closed_group (
                group_id TEXT PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'single',
                label TEXT NOT NULL DEFAULT '',
                workspace_id TEXT NOT NULL DEFAULT '',
                workspace_name TEXT NOT NULL DEFAULT '',
                workspace_cwd TEXT,
                closed_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE closed_tab (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                group_id TEXT NOT NULL,
                tab_id TEXT NOT NULL DEFAULT '',
                parent_tab_id TEXT,
                title TEXT NOT NULL DEFAULT '',
                description TEXT,
                backing_json TEXT NOT NULL DEFAULT '{"kind":"pty"}',
                cwd TEXT,
                agent_type TEXT,
                agent_session_id TEXT,
                model TEXT,
                workspace_id TEXT NOT NULL DEFAULT '',
                workspace_name TEXT NOT NULL DEFAULT '',
                closed_at INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO closed_group (group_id, kind, label, workspace_id, workspace_name, closed_at)
            VALUES ('g1', 'single', 'old', 'w', 'WS', 1);
            INSERT INTO closed_tab (group_id, tab_id, title, backing_json, agent_type, workspace_id, workspace_name, closed_at)
            VALUES ('g1', 'p1', 'old tab', '{"kind":"pty"}', 'codex', 'w', 'WS', 1);
            PRAGMA user_version = 1;
            "#,
        )
        .unwrap();
    }

    let store = ClosedTabsStore::open_at(&db).unwrap();
    let list = store.list_for_workspace("w");
    assert_eq!(list.len(), 1);
    assert!(!list[0].chat_view);
    let conn = Connection::open(&db).unwrap();
    assert_eq!(
        ClosedTabsStore::user_version(&conn).unwrap(),
        CURRENT_CLOSED_SCHEMA_VERSION
    );
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn survives_drop_and_reopen() {
    let db = temp_db();
    {
        let store = ClosedTabsStore::open_at(&db).unwrap();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[tab("p1", "w", "kept", 1)],
            )
            .unwrap();
    } // dropped — simulates restart
    let reopened = ClosedTabsStore::open_at(&db).unwrap();
    let list = reopened.list_for_workspace("w");
    assert_eq!(list.len(), 1, "history persists across reopen");
    assert_eq!(list[0].title, "kept");
    let conn = Connection::open(&db).unwrap();
    assert_eq!(
        ClosedTabsStore::user_version(&conn).unwrap(),
        CURRENT_CLOSED_SCHEMA_VERSION
    );
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn global_group_cap_drops_oldest_groups() {
    let cap = 3;
    let store = ClosedTabsStore::in_memory_with_caps(RECENTLY_CLOSED_CAP, cap);
    for i in 0..(cap + 2) {
        let ws = format!("w{i}");
        store
            .insert_group(
                &group(&format!("g{i}"), GroupKind::Workspace, &ws, i as u64),
                &[
                    tab(&format!("p{i}-a"), &ws, "t", i as u64),
                    tab(&format!("p{i}-b"), &ws, "t", i as u64),
                ],
            )
            .unwrap();
    }
    let rows = store.list_recent(20);
    let ids: Vec<_> = rows.into_iter().map(|row| row.tab.tab_id).collect();
    assert_eq!(ids, ["p4-b", "p4-a", "p3-b", "p3-a", "p2-b", "p2-a"]);
    assert!(store.peek_group("g0").is_none());
    assert!(store.peek_group("g1").is_none());
    for id in ["g2", "g3", "g4"] {
        assert_eq!(store.peek_group(id).unwrap().1.len(), 2);
    }
}

/// The exact on-disk shape a fully-migrated pre-rename (v3) install left
/// behind: `closed_pane` with `pane_id`/`parent_pane_id`.
fn write_pre_rename_v3_db(db: &std::path::Path) {
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let conn = Connection::open(db).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE closed_group (
            group_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL DEFAULT 'single',
            label TEXT NOT NULL DEFAULT '',
            workspace_id TEXT NOT NULL DEFAULT '',
            workspace_name TEXT NOT NULL DEFAULT '',
            workspace_cwd TEXT,
            closed_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE closed_pane (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id TEXT NOT NULL,
            pane_id TEXT NOT NULL DEFAULT '',
            parent_pane_id TEXT,
            title TEXT NOT NULL DEFAULT '',
            description TEXT,
            backing_json TEXT NOT NULL DEFAULT '{"kind":"pty"}',
            cwd TEXT,
            agent_type TEXT,
            agent_session_id TEXT,
            model TEXT,
            workspace_id TEXT NOT NULL DEFAULT '',
            workspace_name TEXT NOT NULL DEFAULT '',
            closed_at INTEGER NOT NULL DEFAULT 0,
            chat_view INTEGER NOT NULL DEFAULT 0,
            role_profile TEXT
        );
        CREATE INDEX idx_closed_pane_workspace ON closed_pane(workspace_id, closed_at DESC);
        CREATE INDEX idx_closed_pane_group ON closed_pane(group_id);
        CREATE INDEX idx_closed_pane_recent ON closed_pane(closed_at DESC);
        INSERT INTO closed_group (group_id, kind, label, workspace_id, workspace_name, closed_at)
        VALUES ('g-old', 'single', 'pre-rename', 'w', 'WS', 42);
        INSERT INTO closed_pane (group_id, pane_id, title, backing_json, workspace_id, workspace_name, closed_at)
        VALUES ('g-old', 'pane-1', 'kept from pane era', '{"kind":"pty"}', 'w', 'WS', 42);
        PRAGMA user_version = 3;
        "#,
    )
    .unwrap();
}

#[test]
fn pre_rename_v3_database_opens_migrates_and_records_new_closes() {
    let db = temp_db();
    write_pre_rename_v3_db(&db);

    let store = ClosedTabsStore::open_at(&db).expect("pre-rename db must open");
    let list = store.list_for_workspace("w");
    assert_eq!(list.len(), 1, "pane-era row survives the rename");
    assert_eq!(list[0].tab_id, "pane-1");
    assert_eq!(list[0].title, "kept from pane era");

    store
        .insert_group(
            &group("g-new", GroupKind::Single, "w", 100),
            &[tab("t-new", "w", "post-migration close", 100)],
        )
        .expect("insert into migrated db");
    assert_eq!(store.list_for_workspace("w").len(), 2);

    let conn = Connection::open(&db).unwrap();
    assert_eq!(
        ClosedTabsStore::user_version(&conn).unwrap(),
        CURRENT_CLOSED_SCHEMA_VERSION,
        "version ladder ran after the rename"
    );
    assert!(!ClosedTabsStore::table_exists(&conn, "closed_pane").unwrap());
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn pre_rename_v1_database_migrates_through_full_ladder() {
    let db = temp_db();
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE closed_group (
                group_id TEXT PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'single',
                label TEXT NOT NULL DEFAULT '',
                workspace_id TEXT NOT NULL DEFAULT '',
                workspace_name TEXT NOT NULL DEFAULT '',
                workspace_cwd TEXT,
                closed_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE closed_pane (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                group_id TEXT NOT NULL,
                pane_id TEXT NOT NULL DEFAULT '',
                parent_pane_id TEXT,
                title TEXT NOT NULL DEFAULT '',
                description TEXT,
                backing_json TEXT NOT NULL DEFAULT '{"kind":"pty"}',
                cwd TEXT,
                agent_type TEXT,
                agent_session_id TEXT,
                model TEXT,
                workspace_id TEXT NOT NULL DEFAULT '',
                workspace_name TEXT NOT NULL DEFAULT '',
                closed_at INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO closed_group (group_id, kind, workspace_id, workspace_name, closed_at)
            VALUES ('g1', 'single', 'w', 'WS', 1);
            INSERT INTO closed_pane (group_id, pane_id, title, workspace_id, workspace_name, closed_at)
            VALUES ('g1', 'p1', 'v1 pane row', 'w', 'WS', 1);
            PRAGMA user_version = 1;
            "#,
        )
        .unwrap();
    }

    let store = ClosedTabsStore::open_at(&db).expect("v1 pane db must open");
    let list = store.list_for_workspace("w");
    assert_eq!(list.len(), 1);
    assert!(!list[0].chat_view, "v2 column added with default");
    let conn = Connection::open(&db).unwrap();
    assert_eq!(
        ClosedTabsStore::user_version(&conn).unwrap(),
        CURRENT_CLOSED_SCHEMA_VERSION
    );
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}

#[test]
fn insert_group_surfaces_storage_failure_as_err() {
    let store = ClosedTabsStore::in_memory();
    store
        .conn
        .lock()
        .execute("DROP TABLE closed_tab", [])
        .unwrap();
    let err = store
        .insert_group(
            &group("g1", GroupKind::Single, "w", 1),
            &[tab("p1", "w", "t", 1)],
        )
        .expect_err("a broken store must report the insert failure");
    assert!(err.contains("no such table"), "actual error: {err}");
}

#[test]
fn reopening_existing_db_is_idempotent() {
    let db = temp_db();
    let s1 = ClosedTabsStore::open_at(&db).unwrap();
    s1.insert_group(
        &group("g1", GroupKind::Single, "w", 1),
        &[tab("p1", "w", "t", 1)],
    )
    .unwrap();
    drop(s1);
    let s2 = ClosedTabsStore::open_at(&db).unwrap();
    assert_eq!(s2.list_for_workspace("w").len(), 1);
    std::fs::remove_dir_all(db.parent().unwrap()).ok();
}
