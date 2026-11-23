use std::{
    fs::{self, FileTimes},
    time::{Duration, UNIX_EPOCH},
};

use super::*;

fn write_fixture(path: &Path, contents: &[u8], modified_seconds: u64) {
    fs::write(path, contents).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(
            FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(modified_seconds)),
        )
        .unwrap();
}

const MTIME: u64 = 1_720_000_000;

#[test]
fn unchanged_binary_reuses_digest_without_reading_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent");
    write_fixture(&path, b"first", MTIME);
    let cache = BinaryDigestCache::default();
    let digest = cache.digest(&path).unwrap();
    let alias = dir.path().join(".").join("agent");
    assert_eq!(
        cache.read_with(&alias, |_| panic!("cached binary was read again")),
        Some(digest)
    );
}

#[test]
fn changed_size_or_mtime_rehashes_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent");
    let cache = BinaryDigestCache::default();
    write_fixture(&path, b"first", MTIME);
    let first = cache.digest(&path).unwrap();
    write_fixture(&path, b"second", MTIME);
    let second = cache.digest(&path).unwrap();
    assert_ne!(first, second);
    write_fixture(&path, b"third!", MTIME + 10);
    assert_ne!(second, cache.digest(&path).unwrap());
}

#[test]
fn changed_file_identity_rehashes_at_the_same_path_size_and_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent");
    let replacement = dir.path().join("new-agent");
    let cache = BinaryDigestCache::default();
    write_fixture(&path, b"first", MTIME);
    write_fixture(&replacement, b"other", MTIME);
    let first = cache.digest(&path).unwrap();
    let original = FileStamp::read(&File::open(&path).unwrap()).unwrap();
    fs::remove_file(&path).unwrap();
    fs::rename(replacement, &path).unwrap();
    let replaced = FileStamp::read(&File::open(&path).unwrap()).unwrap();
    assert_eq!(original.bytes, replaced.bytes);
    assert_eq!(original.modified, replaced.modified);
    assert_ne!(original.identity, replaced.identity);
    assert_ne!(first, cache.digest(&path).unwrap());
}

#[test]
fn binary_changed_during_hash_is_never_cached() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent");
    write_fixture(&path, b"first", MTIME);
    let cache = BinaryDigestCache::default();
    assert_eq!(
        cache.read_with(&path, |_| {
            write_fixture(&path, b"second", MTIME + 10);
            Some("invalidated".to_owned())
        }),
        None
    );
    let digest = cache.digest(&path).unwrap();
    assert_eq!(
        cache.read_with(&path, |_| panic!("current binary was read again")),
        Some(digest)
    );
}
