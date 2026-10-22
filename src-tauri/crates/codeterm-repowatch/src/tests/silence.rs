use super::support::*;
use crate::backend::WatchSignal;
use crate::identity::RepoIdentity;

#[test]
fn repository_watcher_filters_irrelevant_batches() {
    let repo = tempfile::tempdir().unwrap();
    git2::Repository::init(repo.path()).unwrap();
    write(&repo.path().join(".gitignore"), "ignored/\n");
    let identity = RepoIdentity::resolve(repo.path()).expect("identity resolves");
    let root = &identity.workdir;
    let gitdir = &identity.gitdir;
    let (backend, taps) = ProbeBackend::paired();
    let (registry, rx) = registry_with(backend);
    let _sub = registry.subscribe(root).expect("subscribe");

    for paths in [
        vec![
            gitdir.join("objects/ab/cdef0123456789"),
            gitdir.join("objects/pack/pack-test.pack"),
        ],
        vec![gitdir.join("logs/HEAD")],
        vec![gitdir.join("index.lock")],
        vec![root.join("ignored/build-output.txt")],
        (0..500)
            .map(|i| root.join(format!("ignored/burst/file-{i}.o")))
            .collect(),
    ] {
        let signal = WatchSignal::Batch(paths.clone());
        assert_eq!(fire_worktree(&taps, signal.clone()), 1);
        assert_eq!(fire_shared(&taps, signal), 1);
        assert!(collected(&rx).is_empty(), "irrelevant batch: {paths:?}");
    }

    for (paths, shared) in [
        (vec![root.join("tracked.txt")], false),
        (vec![gitdir.join("refs/heads/main")], true),
    ] {
        let signal = WatchSignal::Batch(paths);
        let fired = if shared {
            fire_shared(&taps, signal)
        } else {
            fire_worktree(&taps, signal)
        };
        assert_eq!(fired, 1);
        assert_eq!(
            collected(&rx),
            vec![SinkEvent::Changed(identity.id.clone())]
        );
    }
}
