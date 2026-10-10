use super::*;
use crate::repositories::workspaces::{WorkspaceId, WorkspaceRecord};
use crate::{Storage, StorageConfig};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[path = "tests/ordered_rows.rs"]
mod ordered_rows;

#[test]
fn restored_shell_fields_preserve_command_profile_and_stopped_state() {
    let values = serde_json::json!({
        "close_restore": {"command": "opaque-command", "stopped": true, "record_id": "close-id", "exact_session": false},
        "shell_profile": {"id": "fixture", "name": "Fixture", "executable": "fixture-shell", "args": [], "wsl_distro": null}
    });
    assert!(DeskLeafFields {
        values: values.clone()
    }
    .validate()
    .is_ok());
    assert!(DeskLeafFields {
        values: serde_json::json!({"close_restore": "wrong-type"})
    }
    .validate()
    .is_err());
    assert!(DeskLeafFields {
        values: serde_json::json!({"shell_profile": 9})
    }
    .validate()
    .is_err());
}

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codeterm-storage-desk-{}-{stamp}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempRoot(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn storage() -> (TempRoot, Storage) {
    let root = TempRoot::new();
    let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    (root, storage)
}

fn desk() -> DeskSnapshot {
    DeskSnapshot {
        version: "1.12.4".into(),
        timestamp: 91,
        active_workspace_id: Some("ws-a".into()),
        workspaces: vec![DeskWorkspace {
            record: WorkspaceRecord {
                id: WorkspaceId::new("ws-a").unwrap(),
                name: "Alpha".into(),
                cwd: Some("C:/project".into()),
                provisioned_cwd: None,
                protected: true,
                task_key_prefix: None,
                task_key_next: 0,
            },
            sort_order: 7,
            active_tab_id: Some("tab-a".into()),
            tabs: vec![DeskTab {
                id: "tab-a".into(),
                title: "A".into(),
                description: None,
                sort_order: 4,
                layout: DeskLayoutNode::Split {
                    orientation: "horizontal".into(),
                    ratio: 0.37,
                    children: vec![
                        DeskLayoutNode::Leaf {
                            tab_id: "tab-a".into(),
                            cwd: Some("C:/project".into()),
                            fields: DeskLeafFields::default(),
                        },
                        DeskLayoutNode::Split {
                            orientation: "vertical".into(),
                            ratio: 0.62,
                            fields: Default::default(),
                            children: vec![DeskLayoutNode::Leaf {
                                tab_id: "hidden-ref".into(),
                                cwd: None,
                                fields: DeskLeafFields::default(),
                            }],
                        },
                    ],
                    fields: Default::default(),
                },
            }],
        }],
        teams: vec![DeskTeam {
            id: "team-a".into(),
            name: Some("A".into()),
            lifecycle: DeskTeamLifecycle::Closing,
            created_at: 7,
            workspace_id: "ws-a".into(),
        }],
        agent_session_identities: vec![DeskAgentSessionIdentity {
            provider: "codex".into(),
            session_id: "session-a".into(),
            record_provider: "codex".into(),
            role: "Worker".into(),
            role_profile: Some("executor".into()),
            parent_tab_id: Some("absent-parent".into()),
            can_spawn: false,
        }],
    }
}

fn desk_with_teams(ids: &[&str]) -> DeskSnapshot {
    let mut snapshot = desk();
    snapshot.teams = ids
        .iter()
        .enumerate()
        .map(|(index, id)| DeskTeam {
            id: (*id).into(),
            name: Some((*id).into()),
            lifecycle: DeskTeamLifecycle::Active,
            created_at: index as u64 + 1,
            workspace_id: "ws-a".into(),
        })
        .collect();
    snapshot
}

#[test]
fn storage_desk_validation_rejects_team_without_a_workspace() {
    let mut snapshot = desk();
    snapshot.teams[0].workspace_id = "missing-workspace".into();

    let error = snapshot.validate().unwrap_err().to_string();

    assert!(error.contains("Team workspace"));
    assert!(error.contains("must name a desk workspace"));
}

#[test]
fn storage_desk_import_round_trips_normalized_layout_and_identity_rows() {
    let (_root, storage) = storage();
    let expected = desk();
    storage
        .desk()
        .import(expected.clone())
        .wait(Duration::from_secs(5))
        .unwrap();

    let actual = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();

    assert_eq!(actual.value.snapshot, expected);
    assert_eq!(actual.value.workspace_revisions.len(), 1);
    assert_eq!(actual.value.tab_revisions.len(), 1);
}

#[test]
fn storage_desk_empty_inventory_round_trips_without_fabricating_workspace() {
    let (_root, storage) = storage();
    let empty = DeskSnapshot {
        version: "1.12.4".into(),
        timestamp: 0,
        active_workspace_id: None,
        workspaces: vec![],
        teams: vec![],
        agent_session_identities: vec![],
    };

    storage
        .desk()
        .import(empty.clone())
        .wait(Duration::from_secs(5))
        .unwrap();

    assert_eq!(
        storage
            .desk()
            .snapshot()
            .wait(Duration::from_secs(5))
            .unwrap()
            .value
            .snapshot,
        empty
    );
}

#[test]
fn storage_desk_rename_is_revision_fenced_and_keeps_unrelated_entities() {
    let (_root, storage) = storage();
    let mut source = desk();
    for index in 0..128 {
        source.workspaces.push(DeskWorkspace {
            record: WorkspaceRecord::new(
                WorkspaceId::new(format!("ws-extra-{index}")).unwrap(),
                format!("Workspace {index}"),
            ),
            sort_order: index,
            active_tab_id: None,
            tabs: vec![],
        });
    }
    storage
        .desk()
        .import(source)
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let workspace_revision = before.value.workspace_revisions["ws-a"];
    let tab_revision = before.value.tab_revisions["tab-a"];
    let unrelated_revision = before.value.workspace_revisions["ws-extra-127"];

    let renamed = storage
        .desk()
        .rename_workspace("ws-a".into(), workspace_revision, "Renamed".into())
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(renamed.receipt.rows_written, 3);
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();

    assert_eq!(after.value.snapshot.workspaces[0].record.name, "Renamed");
    assert_eq!(after.value.tab_revisions["tab-a"], tab_revision);
    assert_eq!(
        after.value.workspace_revisions["ws-extra-127"],
        unrelated_revision
    );
    assert!(storage
        .desk()
        .rename_workspace("ws-a".into(), workspace_revision, "Stale".into())
        .wait(Duration::from_secs(5))
        .is_err());
}

#[test]
fn storage_desk_delta_drops_orphan_team_and_commits_valid_changes() {
    let (_root, storage) = storage();
    let initial = desk();
    storage
        .desk()
        .import(initial.clone())
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let mut submitted = initial;
    submitted.workspaces[0].record.name = "Updated".into();
    submitted.teams.push(DeskTeam {
        id: "team-orphan".into(),
        name: Some("Orphan".into()),
        lifecycle: DeskTeamLifecycle::Active,
        created_at: 8,
        workspace_id: "removed-workspace".into(),
    });

    storage
        .desk()
        .save_preserving(submitted, before.value.revision)
        .wait(Duration::from_secs(5))
        .unwrap();
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();

    assert_eq!(after.value.snapshot.workspaces[0].record.name, "Updated");
    assert_eq!(
        after
            .value
            .snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-a"]
    );
}

#[test]
fn storage_desk_team_swap_commits_and_preserves_order() {
    let (_root, storage) = storage();
    let mut snapshot = desk_with_teams(&["team-a", "team-b", "team-c"]);
    storage
        .desk()
        .import(snapshot.clone())
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    snapshot.teams.swap(0, 1);

    storage
        .desk()
        .save_preserving(snapshot, before.value.revision)
        .wait(Duration::from_secs(5))
        .unwrap();

    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        after
            .value
            .snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-b", "team-a", "team-c"]
    );
}

#[test]
fn storage_desk_team_swap_after_middle_close_commits() {
    let (_root, storage) = storage();
    storage
        .desk()
        .import(desk_with_teams(&["team-a", "team-b", "team-c"]))
        .wait(Duration::from_secs(5))
        .unwrap();
    let before_close = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    storage
        .desk()
        .remove_team("team-b".into(), before_close.value.team_revisions["team-b"])
        .wait(Duration::from_secs(5))
        .unwrap();
    let before_reorder = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let mut target = before_reorder.value.snapshot.clone();
    target.teams.reverse();

    storage
        .desk()
        .save_preserving(target, before_reorder.value.revision)
        .wait(Duration::from_secs(5))
        .unwrap();

    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        after
            .value
            .snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-c", "team-a"]
    );
}

#[test]
fn storage_desk_drops_seeded_orphan_then_reorders_and_restores() {
    let (root, storage) = storage();
    storage
        .desk()
        .import(desk_with_teams(&["team-a", "team-c"]))
        .wait(Duration::from_secs(5))
        .unwrap();
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let database = rusqlite::Connection::open(root.path().join("storage/state.db")).unwrap();
    database
        .execute("UPDATE desk_teams SET ordinal = 2 WHERE id = 'team-c'", [])
        .unwrap();
    database
        .execute(
            "INSERT INTO desk_teams (id, ordinal, name, lifecycle, created_at, workspace_id, revision) \
             VALUES ('team-orphan', 1, 'Orphan', 'active', 3, 'removed-workspace', 1)",
            [],
        )
        .unwrap();
    drop(database);

    let storage = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        before
            .value
            .snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-a", "team-orphan", "team-c"]
    );
    let mut target = before.value.snapshot.clone();
    target.teams.retain(|team| team.id != "team-orphan");
    target.teams.reverse();
    storage
        .desk()
        .save_preserving(target, before.value.revision)
        .wait(Duration::from_secs(5))
        .unwrap();
    storage.shutdown().wait(Duration::from_secs(5)).unwrap();
    drop(storage);

    let restored = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let after = restored
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        after
            .value
            .snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        vec!["team-c", "team-a"]
    );
}

#[test]
fn storage_desk_team_rename_is_revision_fenced() {
    let (_root, storage) = storage();
    storage
        .desk()
        .import(desk())
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let team_revision = before.value.team_revisions["team-a"];

    let renamed = storage
        .desk()
        .rename_team("team-a".into(), team_revision, "Renamed".into())
        .wait(Duration::from_secs(5))
        .unwrap();
    assert!(storage
        .desk()
        .rename_team("team-a".into(), team_revision, "Stale".into())
        .wait(Duration::from_secs(5))
        .is_err());
    storage
        .desk()
        .set_team_lifecycle(
            "team-a".into(),
            renamed.receipt.revision,
            DeskTeamLifecycle::Active,
        )
        .wait(Duration::from_secs(5))
        .unwrap();
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();

    assert_eq!(
        after.value.snapshot.teams[0].name.as_deref(),
        Some("Renamed")
    );
    assert_eq!(
        after.value.snapshot.teams[0].lifecycle,
        DeskTeamLifecycle::Active
    );
}

#[test]
fn storage_desk_workspace_scalar_update_preserves_task_key_and_path_fields() {
    let (_root, storage) = storage();
    storage
        .desk()
        .import(desk())
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let mut record = before.value.snapshot.workspaces[0].record.clone();
    record.cwd = Some("C:/next-project".into());
    record.provisioned_cwd = Some("C:/provisioned".into());
    record.protected = false;
    record.task_key_prefix = Some("issue".into());
    record.task_key_next = 42;
    let revision = before.value.workspace_revisions["ws-a"];
    let stale_record = record.clone();

    storage
        .desk()
        .update_workspace(record, revision)
        .wait(Duration::from_secs(5))
        .unwrap();
    assert!(storage
        .desk()
        .update_workspace(stale_record, revision)
        .wait(Duration::from_secs(5))
        .is_err());
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let updated = &after.value.snapshot.workspaces[0].record;

    assert_eq!(updated.cwd.as_deref(), Some("C:/next-project"));
    assert_eq!(updated.provisioned_cwd.as_deref(), Some("C:/provisioned"));
    assert!(!updated.protected);
    assert_eq!(updated.task_key_prefix.as_deref(), Some("issue"));
    assert_eq!(updated.task_key_next, 42);
}

#[test]
fn storage_desk_reorder_close_and_flush_commit_incrementally() {
    let (_root, storage) = storage();
    let mut source = desk();
    source.workspaces[0].tabs.push(DeskTab {
        id: "tab-b".into(),
        title: "B".into(),
        description: None,
        sort_order: 9,
        layout: DeskLayoutNode::Leaf {
            tab_id: "tab-b".into(),
            cwd: None,
            fields: DeskLeafFields::default(),
        },
    });
    storage
        .desk()
        .import(source)
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let order_revision = before.value.workspace_order_revisions["ws-a"];

    storage
        .desk()
        .reorder_tabs(
            "ws-a".into(),
            order_revision,
            vec!["tab-b".into(), "tab-a".into()],
        )
        .wait(Duration::from_secs(5))
        .unwrap();
    let close_revision = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap()
        .value
        .tab_revisions["tab-a"];
    storage
        .desk()
        .close_tab("tab-a".into(), close_revision)
        .wait(Duration::from_secs(5))
        .unwrap();
    storage.desk().flush().wait(Duration::from_secs(5)).unwrap();

    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(after.value.snapshot.workspaces[0].tabs.len(), 1);
    assert_eq!(after.value.snapshot.workspaces[0].tabs[0].id, "tab-b");
    assert_eq!(
        after.value.snapshot.workspaces[0].active_tab_id.as_deref(),
        Some("tab-b")
    );
}

#[test]
#[cfg(feature = "test-support")]
fn storage_desk_commit_failure_keeps_the_published_snapshot_unchanged() {
    let (_root, storage) = storage();
    storage
        .desk()
        .import(desk())
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    storage
        .inject_fault(crate::Fault::FailNextCommit)
        .wait(Duration::from_secs(5))
        .unwrap();

    assert!(storage
        .desk()
        .rename_workspace(
            "ws-a".into(),
            before.value.workspace_revisions["ws-a"],
            "Uncommitted".into(),
        )
        .wait(Duration::from_secs(5))
        .is_err());
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(after.value.snapshot.workspaces[0].record.name, "Alpha");
    assert_eq!(
        after.value.workspace_revisions["ws-a"],
        before.value.workspace_revisions["ws-a"]
    );
}

#[test]
#[cfg(feature = "test-support")]
fn storage_desk_concurrent_rename_reorder_close_and_flush_respect_writer_order() {
    let (_root, storage) = storage();
    let mut source = desk();
    source.workspaces[0].tabs.push(DeskTab {
        id: "tab-b".into(),
        title: "B".into(),
        description: None,
        sort_order: 9,
        layout: DeskLayoutNode::Leaf {
            tab_id: "tab-b".into(),
            cwd: None,
            fields: DeskLeafFields::default(),
        },
    });
    storage
        .desk()
        .import(source)
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let tab_revision = before.value.tab_revisions["tab-a"];
    let close_revision = before.value.tab_revisions["tab-b"];
    let order_revision = before.value.workspace_order_revisions["ws-a"];
    let hold = storage.hold_writer().unwrap();
    hold.wait_held(Duration::from_secs(5)).unwrap();
    let rename = storage
        .desk()
        .rename_tab("tab-a".into(), tab_revision, "renamed".into());
    let reorder = storage.desk().reorder_tabs(
        "ws-a".into(),
        order_revision,
        vec!["tab-b".into(), "tab-a".into()],
    );
    let close = storage.desk().close_tab("tab-b".into(), close_revision);
    let flush = storage.desk().flush();
    hold.release();

    rename.wait(Duration::from_secs(5)).unwrap();
    reorder.wait(Duration::from_secs(5)).unwrap();
    close.wait(Duration::from_secs(5)).unwrap();
    flush.wait(Duration::from_secs(5)).unwrap();
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(after.value.snapshot.workspaces[0].tabs[0].id, "tab-a");
    assert_eq!(after.value.snapshot.workspaces[0].tabs[0].title, "renamed");
    assert_eq!(
        after.value.snapshot.workspaces[0].active_tab_id.as_deref(),
        Some("tab-a")
    );
}

#[test]
fn storage_desk_close_active_workspace_selects_the_next_durable_workspace() {
    let (_root, storage) = storage();
    let mut source = desk();
    source.workspaces.push(DeskWorkspace {
        record: WorkspaceRecord::new(WorkspaceId::new("ws-b").unwrap(), "Beta"),
        sort_order: 12,
        active_tab_id: None,
        tabs: vec![],
    });
    storage
        .desk()
        .import(source)
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();

    storage
        .desk()
        .close_workspace("ws-a".into(), before.value.workspace_revisions["ws-a"])
        .wait(Duration::from_secs(5))
        .unwrap();

    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        after.value.snapshot.active_workspace_id.as_deref(),
        Some("ws-b")
    );
    assert_eq!(after.value.snapshot.workspaces.len(), 1);
}

#[test]
#[cfg(feature = "test-support")]
fn storage_desk_shutdown_closes_admission_and_settles_accepted_writes() {
    let (_root, storage) = storage();
    storage
        .desk()
        .import(desk())
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let hold = storage.hold_writer().unwrap();
    hold.wait_held(Duration::from_secs(5)).unwrap();
    let accepted = storage.desk().rename_workspace(
        "ws-a".into(),
        before.value.workspace_revisions["ws-a"],
        "Accepted".into(),
    );
    let shutdown = storage.shutdown();
    let refused = storage.desk().rename_workspace(
        "ws-a".into(),
        before.value.workspace_revisions["ws-a"],
        "Refused".into(),
    );
    hold.release();

    accepted.wait(Duration::from_secs(5)).unwrap();
    assert!(refused.wait(Duration::from_secs(5)).is_err());
    shutdown.wait(Duration::from_secs(5)).unwrap();
}

#[test]
fn storage_desk_additive_restore_never_overwrites_live_workspace_or_team() {
    let (_root, storage) = storage();
    let initial = desk();
    storage
        .desk()
        .import(initial.clone())
        .wait(Duration::from_secs(5))
        .unwrap();
    let mut saved = initial.clone();
    saved.workspaces[0].record.name = "stale saved name".into();
    saved.teams[0].name = Some("stale saved team".into());
    saved.workspaces.push(DeskWorkspace {
        record: WorkspaceRecord::new(WorkspaceId::new("ws-b").unwrap(), "Restored"),
        sort_order: 19,
        active_tab_id: None,
        tabs: vec![],
    });

    storage
        .desk()
        .additive_restore(saved.clone())
        .wait(Duration::from_secs(5))
        .unwrap();
    storage
        .desk()
        .additive_restore(saved)
        .wait(Duration::from_secs(5))
        .unwrap();
    let actual = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();

    assert_eq!(actual.value.snapshot.workspaces[0].record.name, "Alpha");
    assert_eq!(actual.value.snapshot.teams[0].name.as_deref(), Some("A"));
    assert_eq!(actual.value.snapshot.workspaces.len(), 2);
}

#[test]
#[cfg(feature = "test-support")]
fn storage_desk_held_writer_keeps_committed_reads_available_until_commit() {
    let (_root, storage) = storage();
    storage
        .desk()
        .import(desk())
        .wait(Duration::from_secs(5))
        .unwrap();
    let before = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    let revision = before.value.workspace_revisions["ws-a"];
    let hold = storage.hold_writer().unwrap();
    hold.wait_held(Duration::from_secs(5)).unwrap();
    let pending = storage
        .desk()
        .rename_workspace("ws-a".into(), revision, "Later".into());

    let committed_read = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        committed_read.value.snapshot.workspaces[0].record.name,
        "Alpha"
    );
    hold.release();
    pending.wait(Duration::from_secs(5)).unwrap();
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(after.value.snapshot.workspaces[0].record.name, "Later");
}

#[test]
fn desk_leaf_fields_accept_a_surface_backing_object_only() {
    let surface = serde_json::json!({
        "surface": { "plugin_id": "dash", "surface_id": "board", "instance_id": "inst-1" }
    });
    assert!(DeskLeafFields { values: surface }.validate().is_ok());
    let resumable = serde_json::json!({
        "surface": {
            "plugin_id": "browser",
            "surface_id": "browser",
            "instance_id": "inst-2",
            "resume_url": "https://a.dev/",
            "resume_profile": "work"
        }
    });
    assert!(DeskLeafFields { values: resumable }.validate().is_ok());
    let malformed = serde_json::json!({ "surface": "dash:board" });
    assert!(DeskLeafFields { values: malformed }.validate().is_err());
}
