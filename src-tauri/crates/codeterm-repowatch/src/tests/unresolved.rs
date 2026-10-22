use super::support::*;
use crate::events::{RepoWatchError, RepoWatchUnavailable};
use crate::unresolved::{PathShape, UnresolvedPaths};
use std::path::Path;

#[test]
fn refusal_holds_only_while_the_path_shape_is_unchanged() {
    let mut paths = UnresolvedPaths::default();
    let missing = PathShape {
        exists: false,
        has_git: false,
    };
    let created = PathShape {
        exists: true,
        has_git: false,
    };
    let error = RepoWatchError::not_a_repository(Path::new("D:/gone"), "missing");

    assert!(paths.refusal("d:/gone", missing).is_none());
    paths.refuse("d:/gone".into(), missing, error.clone());
    assert_eq!(paths.refusal("d:/gone", missing), Some(error));
    assert!(paths.refusal("d:/gone", created).is_none());
    assert!(paths.refusal("d:/other", missing).is_none());
    paths.resolved("d:/gone");
    assert!(paths.refusal("d:/gone", missing).is_none());
}

#[test]
fn a_missing_repository_is_reported_once_per_shape_until_it_appears() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let root = tempfile::tempdir().expect("temp dir");
    let target = root.path().join("gone");
    let (backend, _taps) = ProbeBackend::paired();
    let (registry, rx) = registry_with(backend);

    for _ in 0..50 {
        let error = registry
            .subscribe(&target)
            .expect_err("a missing path is no repository");
        assert_eq!(error.reason, RepoWatchUnavailable::NotARepository);
    }
    assert_eq!(collected(&rx).len(), 1);

    std::fs::create_dir(&target).expect("create plain dir");
    for _ in 0..50 {
        registry
            .subscribe(&target)
            .expect_err("a plain directory is no repository");
    }
    assert_eq!(collected(&rx).len(), 1);

    git(&target, &["init", "-q"]);
    registry
        .subscribe(&target)
        .expect("an initialised repository is watched");
    assert!(collected(&rx).is_empty());
}
