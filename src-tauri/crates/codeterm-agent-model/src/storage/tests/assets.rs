use super::*;

#[test]
fn storage_contract_asset_publication_and_rollback_orphans() {
    let root = temp_root();
    let storage = open(root.path());
    let created = create(&storage, &["a"]).revision;
    let attached = done(attach(&storage, "a", created, "snapshot", b"first"));
    let current = attached.receipt.revision;
    assert_eq!(
        asset_bytes(&storage, "a", "snapshot"),
        Some(b"first".to_vec())
    );
    let before = inventory(&storage);
    let expected = vec![WorkspaceAsset {
        role: role("snapshot"),
        asset: attached.value.clone(),
    }];
    assert_eq!(stored(&before, "a").assets, expected);

    done(storage.inject_fault(Fault::FailNextCommit));
    let rolled_back = failed(attach(&storage, "a", current, "second", b"second"));
    assert_eq!(rolled_back, E::Injected { point: "commit" });
    assert_eq!(inventory(&storage), before);
    assert_eq!(
        audit(&storage),
        (1, 0, 1, 0, 0),
        "a reclaimable orphan, never a dangling reference"
    );

    done(storage.inject_fault(Fault::FailNextAssetPublish));
    let unpublished = failed(attach(&storage, "a", current, "third", b"third"));
    assert_eq!(
        unpublished,
        E::Injected {
            point: "asset publish"
        }
    );
    let stale = failed(attach(&storage, "a", created, "fourth", b"x"));
    assert!(matches!(stale, E::StaleRevision { .. }));
    assert_eq!(
        audit(&storage),
        (1, 0, 1, 1, 0),
        "a stale attach publishes nothing"
    );

    let swept = done(storage.assets().sweep_orphans());
    assert_eq!(
        swept,
        SweepReport {
            removed_blobs: 1,
            removed_temp: 1
        }
    );
    assert_eq!(audit(&storage), (1, 0, 0, 0, 0));
    close(storage);

    let reopened = open(root.path());
    assert_eq!(
        asset_bytes(&reopened, "a", "snapshot"),
        Some(b"first".to_vec())
    );
    assert_eq!(audit(&reopened), (1, 0, 0, 0, 0));
    close(reopened);
}

#[test]
fn storage_contract_asset_detach_cleanup_failure_is_retained() {
    let root = temp_root();
    let storage = open(root.path());
    let created = create(&storage, &["a"]).revision;
    let attached = done(attach(&storage, "a", created, "log", b"bytes"));
    let revision = attached.receipt.revision;
    let missing = failed(
        storage
            .workspaces()
            .detach_asset(id("a"), revision, role("none")),
    );
    assert!(matches!(missing, E::NotFound { .. }));

    done(storage.inject_fault(Fault::FailNextCleanup));
    let detached = done(
        storage
            .workspaces()
            .detach_asset(id("a"), revision, role("log")),
    );
    assert_eq!(detached.value, attached.value.id);
    assert!(stored(&inventory(&storage), "a").assets.is_empty());
    assert_eq!(asset_bytes(&storage, "a", "log"), None);
    assert_eq!(
        audit(&storage),
        (0, 0, 0, 0, 1),
        "reference removed, file removal retained"
    );
    let retained = done(storage.assets().audit()).pending_cleanup;
    assert_eq!(retained[0].asset, attached.value.id);
    assert!(retained[0].last_error.is_some());
    close(storage);

    let storage = open(root.path());
    assert_eq!(
        done(storage.assets().audit()).pending_cleanup,
        retained,
        "retained across restart"
    );
    let report = done(storage.assets().run_cleanup());
    assert_eq!(
        (report.removed, report.retained.len(), report.deferred),
        (1, 0, 0)
    );
    assert_eq!(audit(&storage), (0, 0, 0, 0, 0));

    let revision = stored(&inventory(&storage), "a").revision;
    let one = done(attach(&storage, "a", revision, "slot", b"one"));
    let two = done(attach(&storage, "a", one.receipt.revision, "slot", b"two"));
    assert_ne!(one.value.id, two.value.id);
    assert_eq!(
        audit(&storage),
        (1, 0, 0, 0, 0),
        "the replaced asset is released after commit"
    );
    assert_eq!(asset_bytes(&storage, "a", "slot"), Some(b"two".to_vec()));
    close(storage);
}

#[test]
fn storage_contract_asset_read_survives_concurrent_detach() {
    let root = temp_root();
    let storage = open(root.path());
    let created = create(&storage, &["a"]).revision;
    let attached = done(attach(&storage, "a", created, "log", b"payload"));
    let pause = storage.pause_next_asset_read();
    let read = storage.workspaces().asset(id("a"), role("log"));
    pause
        .wait_held(DEADLINE)
        .expect("reader holds a snapshot that sees the reference");

    let revision = attached.receipt.revision;
    let detached = done(
        storage
            .workspaces()
            .detach_asset(id("a"), revision, role("log")),
    );
    assert_eq!(detached.value, attached.value.id);
    assert!(
        stored(&inventory(&storage), "a").assets.is_empty(),
        "other reads stay usable"
    );
    assert_eq!(
        audit(&storage),
        (0, 0, 0, 0, 1),
        "removal waits for the pinned snapshot"
    );
    let pending = done(storage.assets().audit()).pending_cleanup;
    assert_eq!(pending[0].last_error, None);
    let retry = done(storage.assets().run_cleanup());
    assert_eq!(
        (retry.removed, retry.deferred),
        (0, 1),
        "explicit retry respects the pin"
    );

    pause.release();
    assert_eq!(done(read).value, Some(b"payload".to_vec()));
    assert_eq!(
        audit(&storage),
        (0, 0, 0, 0, 0),
        "the ended snapshot released the removal"
    );
    close(storage);
}

#[test]
fn storage_contract_asset_publication_never_replaces_existing_blob() {
    let root = temp_root();
    let storage = open(root.path());
    let created = create(&storage, &["a"]).revision;
    let first = done(attach(&storage, "a", created, "one", b"original"));
    done(storage.inject_fault(Fault::NextAssetId(first.value.id.clone())));
    let revision = first.receipt.revision;
    let collided = failed(attach(&storage, "a", revision, "two", b"intruder"));
    let refused = matches!(
        collided,
        E::Io {
            kind: std::io::ErrorKind::AlreadyExists,
            ..
        }
    );
    assert!(refused, "{collided:?}");
    assert_eq!(
        asset_bytes(&storage, "a", "one"),
        Some(b"original".to_vec())
    );
    assert_eq!(
        audit(&storage),
        (1, 0, 0, 0, 0),
        "nothing replaced and no staging left"
    );
    let second = done(attach(&storage, "a", revision, "two", b"second"));
    assert_ne!(second.value.id, first.value.id);
    close(storage);
}

#[test]
fn storage_contract_deep_root_publishes_restores_and_reopens() {
    let base = temp_root();
    let mut root = base.path().to_path_buf();
    while root.as_os_str().len() < 300 {
        root.push("d".repeat(40));
    }
    let storage = open(&root);
    let created = create(&storage, &["a"]).revision;
    done(attach(&storage, "a", created, "deep", b"long path payload"));
    let blob_path = storage_dir(&root)
        .join("assets")
        .join("blobs")
        .join("x".repeat(32));
    assert!(
        blob_path.as_os_str().len() > 300,
        "fixture exceeds MAX_PATH"
    );
    let image = root.join("image");
    done(storage.backup_to(&image));
    close(storage);

    let reopened = open(&root);
    assert_eq!(
        asset_bytes(&reopened, "a", "deep"),
        Some(b"long path payload".to_vec())
    );
    assert_eq!(audit(&reopened), (1, 0, 0, 0, 0));
    close(reopened);
    let restored_root = root.join("restored");
    Storage::restore_backup(&image, &restored_root).expect("restore into a deep root");
    let restored = open(&restored_root);
    assert_eq!(
        asset_bytes(&restored, "a", "deep"),
        Some(b"long path payload".to_vec())
    );
    close(restored);
}
