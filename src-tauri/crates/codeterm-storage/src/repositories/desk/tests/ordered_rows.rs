use super::*;

fn ordered_ordinals(
    database: &rusqlite::Connection,
    sql: &str,
    workspace_id: Option<&str>,
) -> Vec<i64> {
    let mut statement = database.prepare(sql).unwrap();
    statement
        .query_map([workspace_id], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn desk_with_workspaces(ids: &[&str]) -> DeskSnapshot {
    let mut snapshot = desk();
    snapshot
        .workspaces
        .extend(ids.iter().enumerate().map(|(index, id)| DeskWorkspace {
            record: WorkspaceRecord::new(
                WorkspaceId::new(*id).unwrap(),
                format!("Workspace {index}"),
            ),
            sort_order: index as i32,
            active_tab_id: None,
            tabs: vec![],
        }));
    snapshot
}

fn desk_with_tabs(ids: &[&str]) -> DeskSnapshot {
    let mut snapshot = desk();
    snapshot.workspaces[0].tabs = ids
        .iter()
        .enumerate()
        .map(|(index, id)| DeskTab {
            id: (*id).into(),
            title: format!("Tab {index}"),
            description: None,
            sort_order: index as i32,
            layout: DeskLayoutNode::Leaf {
                tab_id: (*id).into(),
                cwd: None,
                fields: DeskLeafFields::default(),
            },
        })
        .collect();
    snapshot.workspaces[0].active_tab_id = ids.first().map(|id| (*id).to_string());
    snapshot
}

#[test]
fn storage_desk_renumbers_gapped_teams_after_dropping_orphans_and_record_change() {
    let (root, storage) = storage();
    storage
        .desk()
        .import(desk_with_teams(&["team-a", "team-b", "team-c"]))
        .wait(Duration::from_secs(5))
        .unwrap();
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let database = rusqlite::Connection::open(root.path().join("storage/state.db")).unwrap();
    database
        .execute_batch(
            "UPDATE desk_teams SET ordinal = ordinal + 100 WHERE id IN ('team-b', 'team-c');
             UPDATE desk_teams SET ordinal = 2 WHERE id = 'team-b';
             UPDATE desk_teams SET ordinal = 3 WHERE id = 'team-c';
             INSERT INTO desk_teams (id, ordinal, name, lifecycle, created_at, workspace_id, revision)
             VALUES ('team-orphan', 1, 'Orphan', 'active', 4, 'removed-workspace', 1);",
        )
        .unwrap();
    drop(database);

    let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let mut target = before.value.snapshot.clone();
    target.teams.retain(|team| team.id != "team-orphan");
    target.teams[2].name = Some("Updated C".into());

    storage
        .desk()
        .save_preserving(target, before.value.revision)
        .wait(Duration::from_secs(5))
        .unwrap();
    let committed = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        committed
            .value
            .snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-a", "team-b", "team-c"]
    );
    assert_eq!(
        committed.value.snapshot.teams[2].name.as_deref(),
        Some("Updated C")
    );
    assert_eq!(
        committed.value.team_revisions["team-a"],
        before.value.team_revisions["team-a"]
    );
    assert_eq!(
        committed.value.team_revisions["team-b"],
        before.value.team_revisions["team-b"]
    );
    let database = rusqlite::Connection::open(root.path().join("storage/state.db")).unwrap();
    assert_eq!(
        ordered_ordinals(
            &database,
            "SELECT ordinal FROM desk_teams WHERE ?1 IS NULL ORDER BY ordinal",
            None
        ),
        vec![0, 1, 2]
    );
    drop(database);
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let restored_storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let restored = restored_storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        restored
            .value
            .snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-a", "team-b", "team-c"]
    );
    restored_storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .unwrap();
}

#[test]
fn storage_desk_renumbers_gapped_workspaces_after_record_change_and_restores() {
    let (root, storage) = storage();
    storage
        .desk()
        .import(desk_with_workspaces(&["ws-b", "ws-c"]))
        .wait(Duration::from_secs(5))
        .unwrap();
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let database = rusqlite::Connection::open(root.path().join("storage/state.db")).unwrap();
    database
        .execute_batch(
            "UPDATE workspaces SET ordinal = ordinal + 100 WHERE id IN ('ws-b', 'ws-c');
             UPDATE workspaces SET ordinal = 2 WHERE id = 'ws-b';
             UPDATE workspaces SET ordinal = 3 WHERE id = 'ws-c';",
        )
        .unwrap();
    drop(database);

    let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let mut target = before.value.snapshot.clone();
    target.workspaces[2].record.name = "Updated C".into();
    storage
        .desk()
        .save_preserving(target, before.value.revision)
        .wait(Duration::from_secs(5))
        .unwrap();
    let committed = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        committed
            .value
            .snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.record.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ws-a", "ws-b", "ws-c"]
    );
    assert_eq!(
        committed.value.snapshot.workspaces[2].record.name,
        "Updated C"
    );
    assert_eq!(
        committed.value.workspace_revisions["ws-a"],
        before.value.workspace_revisions["ws-a"]
    );
    assert_eq!(
        committed.value.workspace_revisions["ws-b"],
        before.value.workspace_revisions["ws-b"]
    );
    let database = rusqlite::Connection::open(root.path().join("storage/state.db")).unwrap();
    assert_eq!(
        ordered_ordinals(
            &database,
            "SELECT ordinal FROM workspaces WHERE ?1 IS NULL ORDER BY ordinal",
            None
        ),
        vec![0, 1, 2]
    );
    drop(database);
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let restored_storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let restored = restored_storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        restored
            .value
            .snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.record.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ws-a", "ws-b", "ws-c"]
    );
    restored_storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .unwrap();
}

#[test]
fn storage_desk_renumbers_gapped_tabs_after_record_change_and_restores() {
    let (root, storage) = storage();
    storage
        .desk()
        .import(desk_with_tabs(&["tab-a", "tab-b", "tab-c"]))
        .wait(Duration::from_secs(5))
        .unwrap();
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let database = rusqlite::Connection::open(root.path().join("storage/state.db")).unwrap();
    database
        .execute_batch(
            "UPDATE desk_tabs SET ordinal = ordinal + 100 WHERE id IN ('tab-b', 'tab-c');
             UPDATE desk_tabs SET ordinal = 2 WHERE id = 'tab-b';
             UPDATE desk_tabs SET ordinal = 3 WHERE id = 'tab-c';",
        )
        .unwrap();
    drop(database);

    let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let mut target = before.value.snapshot.clone();
    target.workspaces[0].tabs[2].title = "Updated C".into();
    storage
        .desk()
        .save_preserving(target, before.value.revision)
        .wait(Duration::from_secs(5))
        .unwrap();
    let committed = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        committed.value.snapshot.workspaces[0]
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["tab-a", "tab-b", "tab-c"]
    );
    assert_eq!(
        committed.value.snapshot.workspaces[0].tabs[2].title,
        "Updated C"
    );
    assert_eq!(
        committed.value.tab_revisions["tab-a"],
        before.value.tab_revisions["tab-a"]
    );
    assert_eq!(
        committed.value.tab_revisions["tab-b"],
        before.value.tab_revisions["tab-b"]
    );
    let database = rusqlite::Connection::open(root.path().join("storage/state.db")).unwrap();
    assert_eq!(
        ordered_ordinals(
            &database,
            "SELECT ordinal FROM desk_tabs WHERE workspace_id = ?1 ORDER BY ordinal",
            Some("ws-a")
        ),
        vec![0, 1, 2]
    );
    drop(database);
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let restored_storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let restored = restored_storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        restored.value.snapshot.workspaces[0]
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["tab-a", "tab-b", "tab-c"]
    );
    restored_storage
        .shutdown()
        .wait(Duration::from_secs(5))
        .unwrap();
}
