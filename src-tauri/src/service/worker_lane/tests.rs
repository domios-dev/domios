use super::*;
use std::path::PathBuf;

fn git(dir: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    let output = command.current_dir(dir).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn source_only_fixture() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);
    std::fs::create_dir(project.join("node_modules")).unwrap();
    std::fs::write(project.join("node_modules/marker.txt"), "canonical").unwrap();
    (root, project)
}

fn remote_bundle_fixture() -> (tempfile::TempDir, PathBuf, String, String, Vec<u8>) {
    let (_root, origin) = source_only_fixture();
    std::fs::remove_dir_all(origin.join("node_modules")).unwrap();
    let base = git_value(origin.to_str().unwrap(), ["rev-parse", "HEAD"]).unwrap();
    let peer_root = tempfile::tempdir().unwrap();
    let peer = peer_root.path().join("peer");
    let peer_str = peer.to_str().unwrap();
    let mut clone = Command::new("git");
    process_util::no_window_std(&mut clone);
    let output = clone
        .args(["clone", "-q", "--shared"])
        .arg(&origin)
        .arg(&peer)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git(&peer, &["config", "user.email", "peer@example.com"]);
    git(&peer, &["config", "user.name", "Peer"]);
    git(&peer, &["switch", "-c", "lane/remote-test"]);
    std::fs::write(peer.join("remote.txt"), "peer work").unwrap();
    git(&peer, &["add", "remote.txt"]);
    git(&peer, &["commit", "-qm", "peer work"]);
    let lane_tip = git_value(peer_str, ["rev-parse", "HEAD"]).unwrap();
    let bundle_dir = tempfile::tempdir().unwrap();
    let bundle_file = bundle_dir.path().join("lane.bundle");
    let mut bundle = Command::new("git");
    process_util::no_window_std(&mut bundle);
    let output = bundle
        .current_dir(&peer)
        .args(["bundle", "create"])
        .arg(&bundle_file)
        .arg("refs/heads/lane/remote-test")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle_bytes = std::fs::read(&bundle_file).unwrap();
    (_root, origin, base, lane_tip, bundle_bytes)
}

#[test]
fn repository_remote_normalization_unifies_supported_forms() {
    let expected = "github.com/rollacode/codeterm";
    for remote in [
        "git@github.com:rollacode/codeterm.git",
        "ssh://git@github.com/rollacode/codeterm.git/",
        "ssh://other@GITHUB.com:2222/rollacode/codeterm.git",
        "https://github.com/rollacode/codeterm",
        "https://user:token@GITHUB.com:443/rollacode/codeterm.git/",
        "git://github.com/rollacode/codeterm.git",
    ] {
        assert_eq!(sanitized_remote_identity(remote).as_deref(), Some(expected));
    }
    assert_ne!(
        sanitized_remote_identity("git@github.com:rollacode/another-repo.git").as_deref(),
        Some(expected)
    );
}

#[test]
fn repository_identity_intersection_matches_one_of_multiple_remotes() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(
        &project,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:rollacode/codeterm.git",
        ],
    );
    git(
        &project,
        &[
            "remote",
            "add",
            "upstream",
            "https://github.com/rollacode/codeterm.git",
        ],
    );
    git(
        &project,
        &[
            "remote",
            "add",
            "fork",
            "https://github.com/another-owner/codeterm.git",
        ],
    );
    let expected = vec!["github.com/rollacode/codeterm".to_string()];
    assert!(repository_matches(
        project.to_str().unwrap(),
        &expected,
        "unused-legacy-identity"
    ));
    assert!(matching_remote(project.to_str().unwrap(), &expected).is_some());
    assert!(!repository_matches(
        project.to_str().unwrap(),
        &["github.com/another-owner/other-repo".into()],
        "unused-legacy-identity"
    ));
}

#[test]
fn legacy_repository_identity_fails_closed_for_different_remote_forms() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(
        &project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/rollacode/codeterm.git",
        ],
    );
    let legacy_identity = legacy_repository_identity(project.to_str().unwrap()).unwrap();
    assert!(repository_matches(
        project.to_str().unwrap(),
        &[],
        &legacy_identity
    ));

    git(
        &project,
        &[
            "remote",
            "set-url",
            "origin",
            "git@github.com:rollacode/codeterm.git",
        ],
    );
    assert!(!repository_matches(
        project.to_str().unwrap(),
        &[],
        &legacy_identity
    ));
}

#[test]
fn linked_worktree_resolves_to_its_primary_checkout() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let linked = root.path().join("linked");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);
    git(
        &project,
        &[
            "worktree",
            "add",
            "-b",
            "linked/fixture",
            linked.to_str().unwrap(),
        ],
    );

    assert!(is_linked_worktree(&linked));
    assert_eq!(
        primary_checkout(&linked).unwrap(),
        std::fs::canonicalize(project).unwrap()
    );
}

#[test]
fn missing_base_fetches_once_from_a_local_bare_remote() {
    let root = tempfile::tempdir().unwrap();
    let bare = root.path().join("remote.git");
    let source = root.path().join("source");
    let peer = root.path().join("peer");
    std::fs::create_dir_all(&source).unwrap();
    git(
        root.path(),
        &["init", "--bare", "-q", bare.to_str().unwrap()],
    );
    git(&source, &["init", "-q"]);
    git(&source, &["config", "user.email", "test@example.com"]);
    git(&source, &["config", "user.name", "Test"]);
    git(
        &source,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    std::fs::write(source.join("base.txt"), "base").unwrap();
    git(&source, &["add", "base.txt"]);
    git(&source, &["commit", "-qm", "base"]);
    git(
        &source,
        &["push", "--quiet", "origin", "HEAD:refs/heads/fixture"],
    );
    git(
        root.path(),
        &[
            "--git-dir",
            bare.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/fixture",
        ],
    );
    git(
        root.path(),
        &[
            "clone",
            "--quiet",
            bare.to_str().unwrap(),
            peer.to_str().unwrap(),
        ],
    );
    std::fs::write(source.join("next.txt"), "next").unwrap();
    git(&source, &["add", "next.txt"]);
    git(&source, &["commit", "-qm", "next"]);
    let missing_base = git_value(source.to_str().unwrap(), ["rev-parse", "HEAD"]).unwrap();
    git(
        &source,
        &["push", "--quiet", "origin", "HEAD:refs/heads/fixture"],
    );
    assert!(!contains_commit(peer.to_str().unwrap(), &missing_base));

    fetch_base_commit(peer.to_str().unwrap(), "origin", &missing_base).unwrap();

    assert!(contains_commit(peer.to_str().unwrap(), &missing_base));
    let error = fetch_base_commit(peer.to_str().unwrap(), "origin", &"0".repeat(40)).unwrap_err();
    assert!(error.contains("git fetch exited"), "{error}");
}

#[test]
fn mesh_remote_lane_merge_records_ancestry_after_real_merge() {
    let (_root, origin, base, lane_tip, bundle_bytes) = remote_bundle_fixture();

    let result = integrate_remote_lane_bundle(
        origin.to_str().unwrap(),
        &bundle_bytes,
        "lane/remote-test",
        "attempt_remote_test",
        &base,
        &lane_tip,
        &current_branch(origin.to_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(result.lane_tip, lane_tip);
    assert!(is_ancestor(
        origin.to_str().unwrap(),
        &result.lane_tip,
        &result.target_tip
    ));
    let parents = git_value(
        origin.to_str().unwrap(),
        ["rev-list", "--parents", "-n", "1", "HEAD"],
    )
    .unwrap();
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "integration must create a merge commit"
    );
    assert!(!git_success(
        origin.to_str().unwrap(),
        &[
            "show-ref",
            "--verify",
            "refs/codeterm/mesh/attempt_remote_test"
        ]
    ));
}

#[test]
fn mesh_remote_lane_refuses_tip_different_from_reviewed_sha() {
    let (_root, origin, base, _lane_tip, bundle) = remote_bundle_fixture();
    let error = integrate_remote_lane_bundle(
        origin.to_str().unwrap(),
        &bundle,
        "lane/remote-test",
        "attempt_tip_mismatch",
        &base,
        "reviewed-sha-does-not-match",
        &current_branch(origin.to_str().unwrap()).unwrap(),
    )
    .unwrap_err();
    assert!(error.contains("lane tip differs"), "{error}");
}

#[test]
fn mesh_remote_lane_refuses_dirty_origin_checkout() {
    let (_root, origin, base, lane_tip, bundle) = remote_bundle_fixture();
    std::fs::write(origin.join("untracked.txt"), "local data").unwrap();
    let error = integrate_remote_lane_bundle(
        origin.to_str().unwrap(),
        &bundle,
        "lane/remote-test",
        "attempt_dirty",
        &base,
        &lane_tip,
        &current_branch(origin.to_str().unwrap()).unwrap(),
    )
    .unwrap_err();
    assert!(error.contains("must be clean"), "{error}");
}

#[test]
fn mesh_remote_lane_refuses_wrong_destination_branch() {
    let (_root, origin, base, lane_tip, bundle) = remote_bundle_fixture();
    let error = integrate_remote_lane_bundle(
        origin.to_str().unwrap(),
        &bundle,
        "lane/remote-test",
        "attempt_wrong_destination",
        &base,
        &lane_tip,
        "unrelated-branch",
    )
    .unwrap_err();
    assert!(error.contains("recorded destination branch"), "{error}");
}

#[test]
fn mesh_remote_lane_conflict_aborts_merge_and_keeps_checkout_clean() {
    let (_root, origin, base, lane_tip, bundle) = remote_bundle_fixture();
    std::fs::write(origin.join("remote.txt"), "origin edit").unwrap();
    git(&origin, &["add", "remote.txt"]);
    git(&origin, &["commit", "-qm", "origin edit"]);
    let destination = current_branch(origin.to_str().unwrap()).unwrap();
    let error = integrate_remote_lane_bundle(
        origin.to_str().unwrap(),
        &bundle,
        "lane/remote-test",
        "attempt_conflict",
        &base,
        &lane_tip,
        &destination,
    )
    .unwrap_err();
    assert!(error.contains("real merge of peer lane failed"), "{error}");
    assert!(
        git_value(origin.to_str().unwrap(), ["status", "--porcelain"])
            .unwrap()
            .is_empty()
    );
    assert!(!git_success(
        origin.to_str().unwrap(),
        &["rev-parse", "--verify", "MERGE_HEAD"]
    ));
    assert!(!git_success(
        origin.to_str().unwrap(),
        &[
            "show-ref",
            "--verify",
            "refs/codeterm/mesh/attempt_conflict"
        ]
    ));
}

/// The provenance is persisted and later replayed as a git working directory,
/// so a verbatim prefix here is what refuses a finished worker's report.
#[test]
fn lane_provenance_is_a_path_git_accepts_as_a_working_directory() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);

    let canonical_project = std::fs::canonicalize(&project).unwrap();
    let lane = provision_lane(canonical_project.to_str().unwrap(), "team_test").unwrap();
    let provenance = verify_worker_lane(
        canonical_project.to_str().unwrap(),
        &lane.cwd,
        Some(&lane.branch),
        None,
        LaneProtection::Enforced,
    )
    .unwrap();

    for path in [&provenance.worktree_path, &provenance.canonical_cwd] {
        assert!(
            !path.starts_with(r"\\?\"),
            "lane provenance carries a verbatim prefix git cannot use: {path}"
        );
    }
    run_git(
        &provenance.worktree_path,
        ["rev-parse", "--is-inside-work-tree"],
    )
    .expect("the recorded worktree must validate where the lane's commits live");
    lane.cleanup().unwrap();
}

#[test]
fn canonical_checkout_provisions_a_sibling_worker_lane() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);

    let canonical_project = std::fs::canonicalize(&project).unwrap();
    let lane = provision_lane(canonical_project.to_str().unwrap(), "team_test").unwrap();
    let lane_path = PathBuf::from(&lane.cwd);
    assert!(lane_path.is_dir());
    assert_eq!(
        std::fs::canonicalize(lane_path.parent().unwrap()).unwrap(),
        std::fs::canonicalize(root.path().join("project-worktrees")).unwrap()
    );
    assert!(lane.branch.starts_with("lane/team-test-"));
    assert!(!is_canonical_checkout(
        &lane.cwd,
        Some(project.to_str().unwrap())
    ));
    lane.cleanup().unwrap();
}

#[test]
fn non_git_workspace_is_not_treated_as_a_canonical_checkout() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_cwd = workspace.path().to_str().unwrap();

    assert!(!is_canonical_checkout(workspace_cwd, Some(workspace_cwd)));
}

#[test]
fn worker_lane_verification_reads_back_branch_and_base_commit() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);

    let lane = provision_lane(project.to_str().unwrap(), "verify").unwrap();
    let provenance = lane.verify().unwrap();
    assert_eq!(provenance.branch, lane.branch);
    assert_eq!(
        provenance.base,
        git_value(project.to_str().unwrap(), ["rev-parse", "HEAD"]).unwrap()
    );
    let error = verify_worker_lane(
        project.to_str().unwrap(),
        &lane.cwd,
        Some("lane/wrong"),
        Some(&provenance.base),
        LaneProtection::Enforced,
    )
    .unwrap_err();
    assert!(error.contains("branch mismatch"));
    lane.cleanup().unwrap();
}

/// The branch lookup once used `git branch --show-current`, which needs git
/// 2.22. A GUI-launched daemon can resolve an older git, and the failure
/// surfaced as "no repository checkout resolved" for every lane on the box.
#[test]
fn branch_lookup_reads_attached_and_detached_heads() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);

    let lane = provision_lane(project.to_str().unwrap(), "head-shape").unwrap();
    assert_eq!(current_branch(&lane.cwd).unwrap(), lane.branch);

    let head = git_value(&lane.cwd, ["rev-parse", "HEAD"]).unwrap();
    git(Path::new(&lane.cwd), &["checkout", "-q", "--detach", &head]);
    assert_eq!(current_branch(&lane.cwd).unwrap(), "");

    let error = verify_worker_lane(
        project.to_str().unwrap(),
        &lane.cwd,
        None,
        None,
        LaneProtection::Enforced,
    )
    .unwrap_err();
    assert!(error.contains("detached HEAD"), "{error}");
    verify_worker_lane(
        project.to_str().unwrap(),
        &lane.cwd,
        None,
        None,
        LaneProtection::Waived,
    )
    .expect("a waived lane may sit on a detached HEAD");
    lane.cleanup().unwrap();
}

/// An inspection that never reached a repository is not evidence of a
/// detached HEAD, and reading it as an empty branch once turned a missing
/// git into a detached-lane refusal on every lane of the machine.
#[test]
fn missing_git_or_non_repository_is_not_reported_as_detached_head() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("not-a-repository");
    std::fs::create_dir_all(&outside).unwrap();
    // A garbage gitfile pins the discovery boundary here, so the lookup fails
    // on this directory instead of finding whatever repository encloses TMPDIR.
    std::fs::write(outside.join(".git"), "not a gitfile").unwrap();
    let outside = outside.to_str().unwrap();

    let error = current_branch(outside).unwrap_err();
    assert!(error.contains("failed to inspect worker lane"), "{error}");
    assert!(!error.contains("detached HEAD"), "{error}");

    // No binary to run is the missing-git shape: the spawn fails before any
    // exit status exists to classify as a detached HEAD.
    let unspawnable = root.path().join("absent").join("deeper");
    let error = current_branch(unspawnable.to_str().unwrap()).unwrap_err();
    assert!(error.contains("failed to inspect worker lane"), "{error}");

    let error =
        verify_worker_lane(outside, outside, None, None, LaneProtection::Enforced).unwrap_err();
    assert!(error.contains("failed to inspect worker lane"), "{error}");
    assert!(!error.contains("detached HEAD"), "{error}");

    let error =
        verify_worker_lane(outside, outside, None, None, LaneProtection::Waived).unwrap_err();
    assert!(error.contains("failed to inspect worker lane"), "{error}");
}

#[test]
fn lane_provisioning_source_only_skips_dependency_link_and_preserves_canonical_modules() {
    let (_root, project) = source_only_fixture();
    let canonical_marker = project.join("node_modules/marker.txt");
    let canonical_before = std::fs::read_to_string(&canonical_marker).unwrap();
    let lane = provision_lane_for_test(project.to_str().unwrap(), "source-only").unwrap();
    let lane_modules = Path::new(&lane.cwd).join("node_modules");

    assert!(
        std::fs::symlink_metadata(&lane_modules).is_err(),
        "source-only provisioning must not create a dependency link"
    );
    assert_eq!(
        std::fs::read_to_string(canonical_marker).unwrap(),
        canonical_before
    );
    lane.cleanup().unwrap();
}

#[cfg(windows)]
#[test]
fn lane_provisioning_windows_junction_resolution_is_test_fixture_only() {
    let (_root, project) = source_only_fixture();
    let canonical_marker = project.join("node_modules/marker.txt");
    let canonical_before = std::fs::read_to_string(&canonical_marker).unwrap();
    let lane = provision_lane_for_test(project.to_str().unwrap(), "winlink").unwrap();
    let lane_modules = Path::new(&lane.cwd).join("node_modules");
    let mut command = Command::new("cmd");
    process_util::no_window_std(&mut command);
    let output = command
        .args(["/C", "mklink", "/J"])
        .arg(&lane_modules)
        .arg(project.join("node_modules"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        std::fs::read_to_string(lane_modules.join("marker.txt")).unwrap(),
        canonical_before
    );
    assert_eq!(
        std::fs::read_to_string(canonical_marker).unwrap(),
        canonical_before
    );
    std::fs::remove_dir(&lane_modules).unwrap();
    lane.cleanup().unwrap();
}
