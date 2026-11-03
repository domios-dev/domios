use super::*;
use crate::args::agent::ReportStatusArg;
use crate::cmd::report_attribution::ReportBase;
use std::fs;

fn git(path: &Path, args: &[&str]) -> String {
    let output = git_command(path).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

struct Fixture {
    _root: tempfile::TempDir,
    canonical: std::path::PathBuf,
    lane: std::path::PathBuf,
    scope: TaskReadinessScope,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let canonical = root.path().join("canonical");
        let lane = root.path().join("lane");
        fs::create_dir(&canonical).unwrap();
        git(&canonical, &["init", "--initial-branch", "main"]);
        git(
            &canonical,
            &["config", "user.email", "test@example.invalid"],
        );
        git(&canonical, &["config", "user.name", "Test"]);
        fs::write(canonical.join("owned.txt"), "initial").unwrap();
        git(&canonical, &["add", "owned.txt"]);
        git(&canonical, &["commit", "-m", "initial"]);
        git(
            &canonical,
            &["worktree", "add", "-b", "lane/task", lane.to_str().unwrap()],
        );
        let scope = TaskReadinessScope {
            worktree_path: Some(lane.clone()),
            canonical_continuation_path: Some(canonical.clone()),
            recorded_checkout: true,
            files: codeterm_types::plan::TaskFiles {
                edit: vec!["owned.txt".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        Self {
            _root: root,
            canonical,
            lane,
            scope,
        }
    }

    fn commit(&self, path: &Path, file: &str, body: &str) -> String {
        fs::write(path.join(file), body).unwrap();
        git(path, &["add", file]);
        git(path, &["commit", "-m", "task change"]);
        git(path, &["rev-parse", "HEAD"])
    }

    fn report(
        &self,
        sha: &str,
    ) -> Result<(Option<String>, Option<String>, Option<Vec<String>>), ReportRefusal> {
        enrich_ready_report(
            Some(&self.scope),
            Some(ReportStatusArg::InReview),
            None,
            Some(sha.into()),
            ReportBase::default(),
            Some(vec!["caller-supplied".into()]),
            true,
        )
    }
}

#[test]
fn integrated_lane_accepts_owned_canonical_commit_before_newer_foreign_head() {
    let fixture = Fixture::new();
    fixture.commit(&fixture.lane, "owned.txt", "lane work");
    git(
        &fixture.canonical,
        &["merge", "--no-ff", "--no-edit", "lane/task"],
    );
    let sha = fixture.commit(&fixture.canonical, "owned.txt", "canonical continuation");
    fixture.commit(&fixture.canonical, "foreign.txt", "another worker");
    fs::write(
        fixture.canonical.join("foreign.txt"),
        "uncommitted foreign work",
    )
    .unwrap();
    let (branch, reported, paths) = fixture.report(&sha).unwrap();
    assert_eq!(branch.as_deref(), Some("main"));
    assert_eq!(reported.as_deref(), Some(sha.as_str()));
    assert_eq!(paths.unwrap(), ["owned.txt"]);
}

#[test]
fn canonical_continuation_refuses_unmerged_lanes_foreign_diffs_and_owned_dirt() {
    for case in [
        "unmerged",
        "foreign",
        "rename",
        "dirty",
        "lane-dirty",
        "merge",
        "unknown",
    ] {
        let fixture = Fixture::new();
        if case == "unmerged" {
            fixture.commit(&fixture.lane, "lane.txt", "unmerged work");
        }
        let sha = if case == "rename" {
            git(&fixture.canonical, &["mv", "owned.txt", "foreign.txt"]);
            git(&fixture.canonical, &["commit", "-m", "rename"]);
            git(&fixture.canonical, &["rev-parse", "HEAD"])
        } else if case == "merge" {
            fixture.commit(&fixture.lane, "owned.txt", "lane work");
            git(
                &fixture.canonical,
                &["merge", "--no-ff", "--no-edit", "lane/task"],
            );
            git(&fixture.canonical, &["rev-parse", "HEAD"])
        } else {
            fixture.commit(
                &fixture.canonical,
                if case == "foreign" {
                    "foreign.txt"
                } else {
                    "owned.txt"
                },
                "continuation",
            )
        };
        if case == "dirty" {
            fs::write(fixture.canonical.join("owned.txt"), "dirty").unwrap();
        }
        if case == "lane-dirty" {
            fs::write(fixture.lane.join("owned.txt"), "dirty").unwrap();
        }
        let sha = if case == "unknown" {
            "a".repeat(40)
        } else {
            sha
        };
        assert!(fixture.report(&sha).is_err(), "accepted {case}");
    }
}
