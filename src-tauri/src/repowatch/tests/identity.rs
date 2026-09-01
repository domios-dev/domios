use super::support::*;
use crate::repowatch::identity::RepoIdentity;
use std::sync::Arc;

/// R2 — identity resolution, with linked worktrees as the normal case.
#[test]
fn repository_identity_resolves_linked_worktrees() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let primary = primary_repo();
    let holder = linked_worktree(primary.path(), "lane");
    let lane = holder.path().join("lane");

    // 1. discover resolves both; the linked worktree owns its own identity.
    let primary_id = RepoIdentity::resolve(primary.path()).expect("primary resolves");
    let lane_id = RepoIdentity::resolve(&lane).expect("linked worktree resolves");
    assert!(lane.join(".git").is_file(), "linked worktree .git is a FILE");
    assert_ne!(
        primary_id.key, lane_id.key,
        "a linked worktree is its own bubble, not the primary's"
    );
    assert_ne!(
        primary_id.gitdir, lane_id.gitdir,
        "each worktree has its own HEAD and index"
    );

    // 3. commondir is shared — the ref store is one store.
    assert_eq!(
        primary_id.common_key, lane_id.common_key,
        "both worktrees report the same commondir"
    );

    // 2 + 3. two entries, one shared ref watch, refcounted independently.
    let (registry, _rx) = registry_with(Arc::new(InertBackend));
    let a = registry.subscribe(primary.path()).expect("primary subscribe").token;
    let b = registry.subscribe(&lane).expect("lane subscribe").token;
    assert_eq!(registry.entry_count(), 2, "two identities, two entries");
    assert_eq!(registry.shared_count(), 1, "one shared ref watch");
    assert_eq!(registry.shared_refcount(&primary_id.common_key), 2);

    registry.release(&a);
    assert_eq!(registry.entry_count(), 1);
    assert_eq!(
        registry.shared_count(),
        1,
        "the shared ref watch survives while one worktree still holds it"
    );
    assert_eq!(registry.shared_refcount(&lane_id.common_key), 1);
    registry.release(&b);
    assert_eq!(registry.shared_count(), 0, "released by the last worktree");

    // 4. aliases collapse to one key.
    let dotted = primary.path().join("..").join(
        primary
            .path()
            .file_name()
            .expect("temp dir has a final component"),
    );
    let dotted_id = RepoIdentity::resolve(&dotted).expect("dotted alias resolves");
    assert_eq!(
        dotted_id.key, primary_id.key,
        "a path containing .. maps to the same identity"
    );

    let (alias_registry, _alias_rx) = registry_with(Arc::new(InertBackend));
    let plain = alias_registry.subscribe(primary.path()).unwrap().token;
    let via_dots = alias_registry.subscribe(&dotted).unwrap().token;
    assert_eq!(alias_registry.entry_count(), 1, "aliases share one entry");
    assert_eq!(alias_registry.refcount(&primary_id.key), 2);
    alias_registry.release(&plain);
    alias_registry.release(&via_dots);

    case_insensitive_alias_collapses(primary.path(), &primary_id);
}

/// Guarded by an explicit platform condition rather than skipped silently: on
/// a case-sensitive filesystem two spellings ARE two paths.
#[cfg(any(windows, target_os = "macos"))]
fn case_insensitive_alias_collapses(root: &std::path::Path, expected: &RepoIdentity) {
    let upper = std::path::PathBuf::from(root.to_string_lossy().to_uppercase());
    let Ok(cased) = RepoIdentity::resolve(&upper) else {
        panic!("case-differing path must still resolve on a case-insensitive filesystem");
    };
    assert_eq!(
        cased.key, expected.key,
        "a case-differing form maps to one registry key"
    );
    let (registry, _rx) = registry_with(Arc::new(InertBackend));
    registry.subscribe(root).unwrap();
    registry.subscribe(&upper).unwrap();
    assert_eq!(registry.entry_count(), 1, "case aliases share one entry");
}

#[cfg(not(any(windows, target_os = "macos")))]
fn case_insensitive_alias_collapses(_root: &std::path::Path, _expected: &RepoIdentity) {
    eprintln!("[R2] case-alias collapse not applicable: case-sensitive filesystem");
}
