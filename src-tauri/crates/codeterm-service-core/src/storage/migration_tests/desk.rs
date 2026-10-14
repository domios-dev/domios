use super::*;

/// Most snapshots the legacy desk backup retention could keep at once.
const LEGACY_DESK_BACKUP_RETENTION: u64 = 142;

fn desk_snapshot(storage: &Storage) -> codeterm_storage::DeskSnapshot {
    storage
        .desk()
        .snapshot()
        .wait(DEADLINE)
        .expect("desk snapshot")
        .value
        .snapshot
}

#[test]
fn storage_migration_desk_import_preserves_snapshot_catalogue_and_archived_sources() {
    let fixture = Fixture::seeded_with_desk();
    let originals = tree(&fixture.source());
    let capture = fixture.capture();
    for (relative, role) in [
        ("sessions/named.json", SourceRole::NamedSnapshot),
        (
            "sessions/backups/1730000000.json",
            SourceRole::RotatingBackup,
        ),
        ("sessions/last.json.tmp", SourceRole::InterruptedWrite),
        (
            "sessions/last.json.corrupt-1730000000",
            SourceRole::Quarantined,
        ),
        (
            "sessions/last.json.reduced-1730000000",
            SourceRole::Quarantined,
        ),
        (
            "sessions/agent-session-identities.json.tmp",
            SourceRole::InterruptedWrite,
        ),
    ] {
        assert!(capture
            .files()
            .iter()
            .any(|file| file.relative == relative && file.role == role));
    }
    let storage = fixture.open();
    let before = storage.metrics().committed_operations;

    let receipt = committed(import_copied_root(&storage, &fixture.request()));
    let snapshot = desk_snapshot(&storage);

    assert_eq!(storage.metrics().committed_operations - before, 1);
    assert_eq!(snapshot.version, "legacy");
    assert_eq!(snapshot.timestamp, 91);
    assert_eq!(snapshot.active_workspace_id.as_deref(), Some("ws-a"));
    assert_eq!(
        snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.record.id.as_str())
            .collect::<Vec<_>>(),
        ["ws-a", "ws-b"]
    );
    assert_eq!(
        snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.sort_order)
            .collect::<Vec<_>>(),
        [8, 2]
    );
    assert_eq!(
        snapshot.workspaces[0]
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        ["group-a", "tab-a"]
    );
    assert_eq!(snapshot.workspaces[0].tabs[0].sort_order, 3);
    let DeskLayoutNode::Split {
        orientation,
        ratio,
        children,
        fields: split_fields,
    } = &snapshot.workspaces[0].tabs[0].layout
    else {
        panic!("outer split stays nested")
    };
    assert_eq!(orientation, "horizontal");
    assert_eq!(*ratio, 0.4);
    assert_eq!(split_fields["future_split_state"]["flag"], true);
    assert_eq!(
        split_fields["future_split_state"]["nullable"],
        serde_json::Value::Null
    );
    assert_eq!(children.len(), 2);
    let DeskLayoutNode::Leaf { tab_id, fields, .. } = &children[0] else {
        panic!("first child remains a leaf")
    };
    assert_eq!(tab_id, "tab-a");
    assert_eq!(fields.values["title"], serde_json::Value::Null);
    assert_eq!(
        fields.values["agent_session_model"],
        serde_json::Value::Null
    );
    let DeskLayoutNode::Split {
        children,
        fields: nested_fields,
        ..
    } = &children[1]
    else {
        panic!("second child remains a split")
    };
    assert_eq!(nested_fields["future_split_ratio"], serde_json::Value::Null);
    let DeskLayoutNode::Leaf { tab_id, .. } = &children[0] else {
        panic!("historical leaf stays in the layout")
    };
    assert_eq!(tab_id, "missing-tab");
    let DeskLayoutNode::Leaf { fields, .. } = &children[0] else {
        panic!("historical leaf stays a leaf")
    };
    assert!(!fields.values.as_object().unwrap().contains_key("cwd"));
    let DeskLayoutNode::Leaf { fields, .. } = &snapshot.workspaces[0].tabs[1].layout else {
        panic!("second tab stays a leaf")
    };
    assert_eq!(fields.values["cwd"], serde_json::Value::Null);
    assert_eq!(
        snapshot.workspaces[0].record.task_key_prefix.as_deref(),
        Some("COD")
    );
    assert_eq!(snapshot.workspaces[0].record.task_key_next, 4);
    assert_eq!(
        snapshot
            .teams
            .iter()
            .map(|team| team.id.as_str())
            .collect::<Vec<_>>(),
        ["team-a", "team-historical"]
    );
    assert_eq!(
        snapshot.teams[1].workspace_id, "deleted-workspace",
        "a historical workspace reference stays a reference"
    );
    assert!(!snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.record.id.as_str() == "deleted-workspace"));
    assert_eq!(snapshot.agent_session_identities.len(), 1);
    assert_eq!(snapshot.agent_session_identities[0].provider, "claude");
    assert_eq!(
        snapshot.agent_session_identities[0].session_id,
        "history-session"
    );
    assert_eq!(
        snapshot.agent_session_identities[0].record_provider,
        "codex"
    );
    assert_eq!(
        snapshot.agent_session_identities[0]
            .parent_tab_id
            .as_deref(),
        Some("deleted-tab")
    );
    assert!(!snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .any(|tab| tab.id == "missing-tab"));
    assert!(receipt.comparison.expect("comparison").is_clean());
    assert_eq!(tree(&fixture.backup().join("originals")), originals);
    assert_eq!(tree(&fixture.source()), originals);
    assert_eq!(receipt.archive, Ok(()));
    close(storage);
}

#[test]
fn storage_migration_desk_close_reopen_restart_preserves_nonempty_import() {
    let fixture = Fixture::seeded_with_desk();
    let storage = fixture.open();

    let receipt = committed(import_copied_root(&storage, &fixture.request()));
    let imported = desk_snapshot(&storage);
    assert_eq!(imported.workspaces.len(), 2);
    assert_eq!(receipt.comparison.expect("comparison").is_clean(), true);
    close(storage);

    let reopened = fixture.open();
    assert_eq!(desk_snapshot(&reopened), imported);
    let replay = import_copied_root(&reopened, &fixture.request()).expect("reopened replay");
    assert!(matches!(replay, ImportOutcome::AlreadyCommitted(_)));
    assert_eq!(desk_snapshot(&reopened), imported);
    assert_eq!(
        marker(&reopened).map(|marker| marker.revision),
        Some(receipt.revision)
    );
    close(reopened);
}

#[test]
fn storage_migration_desk_corrupt_current_requires_rotating_backup_selection() {
    let fixture = Fixture::seeded_with_desk();
    fixture.put("sessions/last.json", b"{broken");
    let originals = tree(&fixture.source());
    let storage = fixture.open();

    let error = import_copied_root(&storage, &fixture.request())
        .expect_err("a named snapshot cannot replace a corrupt current silently");
    match refusal(error) {
        ImportRefusal::RecoveryRequired {
            domain,
            current,
            candidates,
        } => {
            assert_eq!(domain, ImportDomain::Desk);
            assert_eq!(current.code(), "malformed");
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].relative, "sessions/backups/1730000000.json");
            assert_eq!(candidates[0].role, SourceRole::RotatingBackup);
        }
        other => panic!("expected rotating-backup recovery selection, got {other:?}"),
    }
    assert_untouched(&storage);
    assert_eq!(tree(&fixture.backup().join("originals")), originals);

    let mut request = fixture.request();
    request.selections.insert(
        ImportDomain::Desk,
        SourceSelection::DeskBackups {
            snapshot: Some("sessions/named.json".to_string()),
            identities: None,
        },
    );
    let error = import_copied_root(&storage, &request)
        .expect_err("named snapshots are retained only in the immutable archive");
    assert!(matches!(
        refusal(error),
        ImportRefusal::InvalidSelection {
            domain: ImportDomain::Desk,
            reason: "named snapshots are archive-only; select a rotating snapshot backup"
        }
    ));
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_desk_identity_backup_requires_explicit_selection() {
    let fixture = Fixture::seeded_with_desk();
    fixture.put("sessions/agent-session-identities.json", b"{broken");
    let storage = fixture.open();

    let error = import_copied_root(&storage, &fixture.request())
        .expect_err("the identity backup cannot replace the current catalogue silently");
    match refusal(error) {
        ImportRefusal::RecoveryRequired {
            domain,
            current,
            candidates,
        } => {
            assert_eq!(domain, ImportDomain::Desk);
            assert_eq!(current.code(), "malformed");
            assert_eq!(candidates.len(), 1);
            assert_eq!(
                candidates[0].relative,
                "sessions/agent-session-identities.json.bak"
            );
        }
        other => panic!("expected identity-catalogue recovery selection, got {other:?}"),
    }
    assert_untouched(&storage);

    let mut request = fixture.request();
    request.selections.insert(
        ImportDomain::Desk,
        SourceSelection::DeskBackups {
            snapshot: None,
            identities: Some("sessions/agent-session-identities.json.bak".to_string()),
        },
    );
    let receipt = committed(import_copied_root(&storage, &request));
    let identities = desk_snapshot(&storage).agent_session_identities;
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].session_id, "old-session");
    assert_eq!(identities[0].record_provider, "claude");
    assert!(receipt.comparison.expect("comparison").is_clean());
    close(storage);
}

#[test]
fn storage_migration_desk_identity_backup_selection_acknowledges_absent_snapshot() {
    let fixture = Fixture::seeded_with_desk();
    fs::remove_file(fixture.source().join("sessions/last.json")).expect("remove snapshot");
    fs::remove_file(fixture.source().join("sessions/backups/1730000000.json"))
        .expect("remove rotating snapshot backup");
    fixture.put("sessions/agent-session-identities.json", b"{broken");
    let mut request = fixture.request();
    request.selections.insert(
        ImportDomain::Desk,
        SourceSelection::DeskBackups {
            snapshot: None,
            identities: Some("sessions/agent-session-identities.json.bak".to_string()),
        },
    );
    let storage = fixture.open();

    let receipt = committed(import_copied_root(&storage, &request));
    let snapshot = desk_snapshot(&storage);

    assert!(snapshot.workspaces.is_empty());
    assert_eq!(snapshot.agent_session_identities.len(), 1);
    assert_eq!(
        snapshot.agent_session_identities[0].session_id,
        "old-session"
    );
    assert!(receipt.comparison.expect("comparison").is_clean());
    close(storage);
}

#[test]
fn storage_migration_desk_bound_failure_leaves_every_domain_and_marker_empty() {
    let fixture = Fixture::seeded_with_desk();
    let oversized = format!("\"name\":\"{}\"", "x".repeat(1025));
    let source = DESK_SOURCE.replace("\"name\":\"Alpha\"", &oversized);
    fixture.put("sessions/last.json", source.as_bytes());
    let storage = fixture.open();

    let error = import_copied_root(&storage, &fixture.request())
        .expect_err("desk bounds refuse the whole import");

    assert!(matches!(
        refusal(error),
        ImportRefusal::Invalid {
            domain: ImportDomain::Desk,
            ..
        }
    ));
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_foreign_key_failure_in_each_domain_rolls_back_all_domains() {
    let fixture = Fixture::seeded_with_desk();
    let storage = fixture.open();

    for domain in [
        ImportDomain::Cron,
        ImportDomain::Sentinel,
        ImportDomain::Desk,
    ] {
        let error = storage
            .commit_legacy_import_tampered(
                fixture.plan(),
                DEADLINE,
                ImportTamper::ForeignKey(domain),
            )
            .expect_err("an injected domain foreign key violation aborts the joined transaction");
        assert!(matches!(error, MigrationError::Storage(_)), "{error:?}");
        assert_untouched(&storage);
    }

    let receipt = storage
        .commit_legacy_import(fixture.plan(), DEADLINE)
        .expect("a fresh plan restarts after each rolled-back attempt");
    assert!(receipt.comparison.expect("comparison").is_clean());
    assert_eq!(
        marker(&storage).map(|marker| marker.revision),
        Some(receipt.revision)
    );
    assert_eq!(desk_snapshot(&storage).workspaces.len(), 2);
    close(storage);
}

#[test]
fn storage_migration_desk_disk_full_rolls_back_and_restarts_losslessly() {
    let fixture = Fixture::seeded_with_desk();
    let large_cron = CRONS.replace(
        "\"check the build\"",
        &format!("\"{}\"", "x".repeat(128 * 1024)),
    );
    fixture.put("crons.json", large_cron.as_bytes());
    let originals = tree(&fixture.source());
    let storage = fixture.open();
    storage
        .inject_fault(Fault::DatabaseFull)
        .wait(DEADLINE)
        .expect("inject disk-full failure");

    let error = import_copied_root(&storage, &fixture.request())
        .expect_err("disk full aborts the all-domain import");
    assert!(matches!(
        error,
        MigrationError::Storage(StorageError::DatabaseFull)
    ));
    assert_untouched(&storage);
    storage
        .inject_fault(Fault::ClearDatabaseFull)
        .wait(DEADLINE)
        .expect("clear disk-full failure");

    let receipt = committed(import_copied_root(&storage, &fixture.request()));
    assert!(receipt.comparison.expect("comparison").is_clean());
    assert_eq!(desk_snapshot(&storage).workspaces.len(), 2);
    assert_eq!(tree(&fixture.source()), originals);
    close(storage);
}

#[test]
fn storage_migration_desk_commit_failure_rolls_back_and_restart_is_idempotent() {
    let fixture = Fixture::seeded_with_desk();
    let originals = tree(&fixture.source());
    let storage = fixture.open();
    storage
        .inject_fault(Fault::FailNextCommit)
        .wait(DEADLINE)
        .expect("inject commit failure");

    let error = import_copied_root(&storage, &fixture.request())
        .expect_err("failed commit leaves no domain or marker");
    assert!(matches!(
        error,
        MigrationError::Storage(StorageError::Injected { point: "commit" })
    ));
    assert_untouched(&storage);
    assert_eq!(tree(&fixture.source()), originals);
    let archived = tree(&fixture.backup().join("originals"));

    let receipt = committed(import_copied_root(&storage, &fixture.request()));
    assert!(receipt.comparison.expect("comparison").is_clean());
    assert_eq!(desk_snapshot(&storage).workspaces.len(), 2);
    assert_eq!(tree(&fixture.backup().join("originals")), archived);
    let replay = import_copied_root(&storage, &fixture.request()).expect("committed restart");
    assert!(matches!(replay, ImportOutcome::AlreadyCommitted(_)));
    assert_eq!(
        marker(&storage).map(|marker| marker.revision),
        Some(receipt.revision)
    );
    close(storage);
}

#[test]
fn storage_migration_root_at_full_legacy_backup_retention_imports_whole() {
    let fixture = Fixture::seeded_with_desk();
    for age in 0..LEGACY_DESK_BACKUP_RETENTION {
        fixture.put(
            &format!("sessions/backups/{}.json", 1_790_000_000 + age),
            NAMED_DESK.as_bytes(),
        );
    }
    for index in 0..crate::persisted_state::DEFAULT_BACKUPS as u64 {
        let name = format!("1790000000000-{index:06}.bak");
        fixture.put(
            &format!("state-backups/crons.json/{name}"),
            CRONS.as_bytes(),
        );
        fixture.put(
            &format!("state-backups/sentinels.json/{name}"),
            SENTINELS.as_bytes(),
        );
    }
    let originals = tree(&fixture.source());
    assert!(
        originals.len() > 128,
        "the root exceeds the former file cap"
    );
    assert_eq!(fixture.capture().files().len(), originals.len());
    let storage = fixture.open();

    let receipt = committed(import_copied_root(&storage, &fixture.request()));

    assert_eq!(receipt.archive, Ok(()));
    assert!(receipt.comparison.expect("comparison").is_clean());
    assert_eq!(desk_snapshot(&storage).workspaces.len(), 2);
    assert_eq!(tree(&fixture.backup().join("originals")), originals);
    assert_eq!(tree(&fixture.source()), originals);
    close(storage);
}
