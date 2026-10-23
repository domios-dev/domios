use super::*;

#[test]
fn bundle_lock_cleanup_preserves_foreign_call_handles() {
    let root = tempfile::tempdir().unwrap();
    let owned = root.path().join("owned");
    let foreign = root.path().join("foreign");
    for directory in [&owned, &foreign] {
        std::fs::create_dir(directory).unwrap();
    }
    for (id, directory) in [
        ("bundle-owned-lock", &owned),
        ("bundle-foreign-lock", &foreign),
    ] {
        let path = directory.join("held.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.try_lock().unwrap();
        held_file_locks().lock().insert(
            id.into(),
            HeldFileLock {
                call_id: id.into(),
                path: path.canonicalize().unwrap(),
                file,
            },
        );
    }
    release_bundle_file_locks(&owned);
    std::fs::rename(&owned, root.path().join("released")).unwrap();
    let mut locks = held_file_locks().lock();
    assert!(!locks.contains_key("bundle-owned-lock"));
    assert!(locks.contains_key("bundle-foreign-lock"));
    locks.remove("bundle-foreign-lock");
}
