use super::*;
use crate::repositories::workspaces::{WorkspaceId, WorkspaceRecord};
use crate::{Storage, StorageConfig};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

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
                            children: vec![DeskLayoutNode::Leaf {
                                tab_id: "hidden-ref".into(),
                                cwd: None,
                                fields: DeskLeafFields::default(),
                            }],
                        },
                    ],
                },
            }],
        }],
        teams: vec![DeskTeam {
            id: "team-a".into(),
            name: Some("A".into()),
            lifecycle: DeskTeamLifecycle::Closing,
            created_at: 7,
            workspace_id: "missing-workspace".into(),
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
fn storage_desk_rename_is_revision_fenced_and_keeps_unrelated_entities() {
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
    let workspace_revision = before.value.workspace_revisions["ws-a"];
    let tab_revision = before.value.tab_revisions["tab-a"];

    storage
        .desk()
        .rename_workspace("ws-a".into(), workspace_revision, "Renamed".into())
        .wait(Duration::from_secs(5))
        .unwrap();
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();

    assert_eq!(after.value.snapshot.workspaces[0].record.name, "Renamed");
    assert_eq!(after.value.tab_revisions["tab-a"], tab_revision);
    assert!(storage
        .desk()
        .rename_workspace("ws-a".into(), workspace_revision, "Stale".into())
        .wait(Duration::from_secs(5))
        .is_err());
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
fn storage_desk_concurrent_rename_close_and_flush_respect_writer_order() {
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
    let old_revision = before.value.tab_revisions["tab-a"];
    let hold = storage.hold_writer().unwrap();
    hold.wait_held(Duration::from_secs(5)).unwrap();
    let rename = storage
        .desk()
        .rename_tab("tab-a".into(), old_revision, "renamed".into());
    let close = storage.desk().close_tab("tab-a".into(), old_revision);
    let flush = storage.desk().flush();
    hold.release();

    rename.wait(Duration::from_secs(5)).unwrap();
    assert!(close.wait(Duration::from_secs(5)).is_err());
    flush.wait(Duration::from_secs(5)).unwrap();
    let after = storage
        .desk()
        .snapshot()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(after.value.snapshot.workspaces[0].tabs[0].title, "renamed");
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
