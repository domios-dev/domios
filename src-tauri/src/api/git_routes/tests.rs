use super::*;
use std::process::Command;

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00")
        .output()
        .expect("git run");
    assert!(
        status.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&status.stderr)
    );
}

fn write(dir: &std::path::Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

/// init → commits → branch → rename → merge.
fn build_fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    run(p, &["init", "-q", "-b", "main"]);
    run(p, &["config", "user.name", "Tester"]);
    run(p, &["config", "user.email", "t@example.com"]);
    run(p, &["config", "commit.gpgsign", "false"]);

    write(p, "a.txt", "one\ntwo\nthree\n");
    run(p, &["add", "a.txt"]);
    run(p, &["commit", "-q", "-m", "root commit"]);

    write(p, "a.txt", "one\ntwo\nthree\nfour\n");
    run(p, &["add", "a.txt"]);
    run(p, &["commit", "-q", "-m", "add line"]);
    run(p, &["tag", "v1"]);

    run(p, &["checkout", "-q", "-b", "feature"]);
    write(p, "b.txt", "feature file\n");
    run(p, &["add", "b.txt"]);
    run(p, &["commit", "-q", "-m", "feature work"]);

    run(p, &["checkout", "-q", "main"]);
    // Rename a.txt -> renamed.txt with a tiny edit so -M detects it.
    std::fs::remove_file(p.join("a.txt")).unwrap();
    write(p, "renamed.txt", "one\ntwo\nthree\nfour\nfive\n");
    run(p, &["add", "-A"]);
    run(p, &["commit", "-q", "-m", "rename a.txt"]);

    run(p, &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"]);
    tmp
}

#[test]
fn classify_refs_by_prefix() {
    assert_eq!(classify_ref("refs/heads/main").unwrap().kind, "branch");
    assert_eq!(classify_ref("refs/remotes/origin/main").unwrap().kind, "remote");
    assert_eq!(classify_ref("refs/tags/v1").unwrap().kind, "tag");
    let head = classify_ref("HEAD -> refs/heads/main").unwrap();
    assert_eq!(head.kind, "head");
    assert_eq!(head.name, "main");
    assert_eq!(classify_ref("HEAD").unwrap().kind, "head");
    assert!(classify_ref("").is_none());
    assert!(classify_ref("garbage").is_none());
}

#[test]
fn parse_graph_records() {
    // Two records, leading FIELD_SEP, second prefixed by newline from RECORD_SEP.
    let raw = b"\x1fabc123\x1fp1 p2\x1fAlice\x1f2026-01-01T00:00:00+00:00\x1fHEAD -> refs/heads/main, tag: refs/tags/v1\x1fmerge: do stuff\x1e\n\x1fdef456\x1f\x1fBob\x1f2026-01-02T00:00:00+00:00\x1f\x1froot\x1e";
    let commits = parse_graph(raw);
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].sha, "abc123");
    assert_eq!(commits[0].parents, vec!["p1", "p2"]);
    assert_eq!(commits[0].author, "Alice");
    assert_eq!(commits[0].subject, "merge: do stuff");
    let kinds: Vec<&str> = commits[0].refs.iter().map(|r| r.kind.as_str()).collect();
    assert!(kinds.contains(&"head"));
    assert!(kinds.contains(&"tag"));
    // Root commit: empty parents.
    assert_eq!(commits[1].sha, "def456");
    assert!(commits[1].parents.is_empty());
    assert_eq!(commits[1].subject, "root");
}

#[test]
fn numstat_modified_and_rename() {
    let modified = parse_numstat_line("3\t1\tsrc/main.rs").unwrap();
    assert_eq!(modified.path, "src/main.rs");
    assert_eq!(modified.added, Some(3));
    assert_eq!(modified.deleted, Some(1));
    assert_eq!(modified.status, "modified");
    assert!(modified.old_path.is_none());

    let renamed = parse_numstat_line("1\t0\told.txt => new.txt").unwrap();
    assert_eq!(renamed.status, "renamed");
    assert_eq!(renamed.old_path.as_deref(), Some("old.txt"));
    assert_eq!(renamed.path, "new.txt");

    let brace = parse_numstat_line("0\t0\tsrc/{old => new}/file.rs").unwrap();
    assert_eq!(brace.status, "renamed");
    assert_eq!(brace.old_path.as_deref(), Some("src/old/file.rs"));
    assert_eq!(brace.path, "src/new/file.rs");

    let binary = parse_numstat_line("-\t-\timage.png").unwrap();
    assert_eq!(binary.status, "binary");
    assert!(binary.added.is_none());
    assert!(binary.deleted.is_none());
}

#[test]
fn sha_validation() {
    assert!(is_valid_sha("abc123"));
    assert!(is_valid_sha("deadbeef^1"));
    assert!(is_valid_sha("abc~3"));
    assert!(!is_valid_sha(""));
    assert!(!is_valid_sha("--upload-pack=evil"));
    assert!(!is_valid_sha("abc; rm -rf /"));
    assert!(!is_valid_sha(&"a".repeat(65)));
}

#[test]
fn render_capped_marks_truncation() {
    let small = render_capped(b"diff hunk\n");
    assert!(!small.contains("TRUNCATED"));
    let big = vec![b'x'; OUTPUT_CAP];
    let out = render_capped(&big);
    assert!(out.ends_with(TRUNCATION_MARKER));
}

#[tokio::test]
async fn graph_against_fixture() {
    if !git_available() {
        eprintln!("skip: git not available");
        return;
    }
    let tmp = build_fixture();
    let cwd = tmp.path().to_str().unwrap();

    ensure_repo(cwd).await.expect("is a repo");

    let pretty = "--pretty=format:%x1f%H%x1f%P%x1f%an%x1f%aI%x1f%D%x1f%s%x1e";
    let log = run_git(
        cwd,
        &[
            "-c",
            "core.quotePath=false",
            "log",
            "--branches",
            "--tags",
            "--remotes",
            "HEAD",
            "--topo-order",
            "--decorate=full",
            pretty,
            "-n",
            "300",
        ],
    )
    .await
    .unwrap();
    assert!(log.status_ok);
    let commits = parse_graph(&log.stdout);
    // root, add line, feature work, rename, merge = 5
    assert_eq!(commits.len(), 5, "expected 5 commits, got {}", commits.len());

    // Merge commit is first (topo-order) and has two parents.
    let merge = commits.iter().find(|c| c.subject == "merge feature").unwrap();
    assert_eq!(merge.parents.len(), 2);

    // Root commit has no parents.
    let root = commits.iter().find(|c| c.subject == "root commit").unwrap();
    assert!(root.parents.is_empty());

    // Tag v1 surfaces as a tag ref somewhere.
    let has_tag = commits
        .iter()
        .flat_map(|c| &c.refs)
        .any(|r| r.kind == "tag" && r.name == "v1");
    assert!(has_tag, "tag v1 not classified");

    // head ref points at main.
    let has_head = commits
        .iter()
        .flat_map(|c| &c.refs)
        .any(|r| r.kind == "head" && r.name == "main");
    assert!(has_head, "HEAD->main not classified");
}

#[tokio::test]
async fn commit_files_detects_rename() {
    if !git_available() {
        return;
    }
    let tmp = build_fixture();
    let cwd = tmp.path().to_str().unwrap();

    let rename_sha = String::from_utf8_lossy(
        &run_git(cwd, &["rev-parse", ":/rename a.txt"])
            .await
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    let out = run_git(
        cwd,
        &["show", "-M", "-C", "--numstat", "--format=", &rename_sha],
    )
    .await
    .unwrap();
    let files = parse_numstat(&out.stdout);
    let renamed = files.iter().find(|f| f.status == "renamed");
    assert!(renamed.is_some(), "rename not detected: {:?}", files);
    let r = renamed.unwrap();
    assert_eq!(r.old_path.as_deref(), Some("a.txt"));
    assert_eq!(r.path, "renamed.txt");
}

#[tokio::test]
async fn merge_diff_uses_first_parent() {
    if !git_available() {
        return;
    }
    let tmp = build_fixture();
    let cwd = tmp.path().to_str().unwrap();

    let merge_sha = String::from_utf8_lossy(
        &run_git(cwd, &["rev-parse", "HEAD"]).await.unwrap().stdout,
    )
    .trim()
    .to_string();

    assert!(is_merge(cwd, &merge_sha).await, "HEAD should be a merge");

    // First-parent diff of the merge for b.txt yields a patch (feature file added on the branch).
    let parent_spec = format!("{}^1", merge_sha);
    let out = run_git(
        cwd,
        &["diff", "-M", "-C", "--no-color", &parent_spec, &merge_sha, "--", "b.txt"],
    )
    .await
    .unwrap();
    assert!(out.status_ok);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("b.txt"), "merge first-parent diff missing b.txt");

    // A regular (non-merge) commit is not a merge.
    let root_sha = String::from_utf8_lossy(
        &run_git(cwd, &["rev-parse", ":/root commit"])
            .await
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert!(!is_merge(cwd, &root_sha).await);
}

#[tokio::test]
async fn ensure_repo_rejects_non_repo() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().to_str().unwrap();
    let res = ensure_repo(cwd).await;
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().0, StatusCode::NOT_FOUND);
}
