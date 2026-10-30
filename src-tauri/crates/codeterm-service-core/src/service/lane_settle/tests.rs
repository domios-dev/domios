use super::*;
use std::process::Command;
use std::sync::Arc;

mod close_fixture;
mod sql_candidates;

const CANONICAL: &str = "/repo";

struct FakeGit {
    ancestry: Ancestry,
    removal: Result<(), String>,
    holder: Option<String>,
    ancestry_calls: Vec<(String, String)>,
    removals: Vec<LaneRemovalRequest>,
}

impl FakeGit {
    fn answering(ancestry: Ancestry) -> Self {
        Self {
            ancestry,
            removal: Ok(()),
            holder: None,
            ancestry_calls: Vec::new(),
            removals: Vec::new(),
        }
    }
}

impl LaneGit for FakeGit {
    fn ancestry(&mut self, _canonical: &str, branch: &str, destination: &str) -> Ancestry {
        self.ancestry_calls
            .push((branch.to_string(), destination.to_string()));
        self.ancestry.clone()
    }

    fn remove(&mut self, request: &LaneRemovalRequest) -> Result<(), String> {
        self.removals.push(request.clone());
        self.removal.clone()
    }

    fn holder(&mut self, _worktree: &str) -> Option<String> {
        self.holder.clone()
    }
}

fn finished_lane(task_id: &str) -> LaneRecord {
    LaneRecord {
        task_id: task_id.into(),
        terminal: true,
        bound_tabs: vec!["worker-tab".into()],
        worktree: Some(format!("/lanes/{task_id}")),
        branch: Some(format!("lane/{task_id}")),
        destination: Some("master".into()),
        ..Default::default()
    }
}

fn recorded(settlement: &Settlement) -> (&'static str, &[String]) {
    match settlement {
        Settlement::Record { status, hints, .. } => (status, hints),
        Settlement::Removed => panic!("expected a recorded settlement, got a removal"),
    }
}

#[test]
fn lane_settle_accept_runs_one_ancestry_check_then_safe_removal() {
    let mut git = FakeGit::answering(Ancestry::Merged);

    let settlement = settle(&finished_lane("a"), CANONICAL, true, &mut git);

    assert_eq!(settlement, Settlement::Removed);
    assert_eq!(git.ancestry_calls, [("lane/a".into(), "HEAD".into())]);
    assert_eq!(git.removals.len(), 1);
    let removal = &git.removals[0];
    assert!(!removal.force, "automatic cleanup never forces");
    assert_eq!(removal.worktree_path, "/lanes/a");
    assert_eq!(removal.branch.as_deref(), Some("lane/a"));
    assert_eq!(removal.canonical_cwd.as_deref(), Some(CANONICAL));
}

#[test]
fn lane_settle_live_holder_defers_cleanup_until_its_tab_closes_once() {
    let mut accepted = FakeGit::answering(Ancestry::Merged);
    accepted.holder = Some("'Ember' (worker-tab)".into());

    let at_accept = settle(&finished_lane("a"), CANONICAL, true, &mut accepted);

    assert!(
        accepted.removals.is_empty(),
        "a held worktree is not touched"
    );
    let (status, hints) = recorded(&at_accept);
    assert_eq!(status, CLEANUP_HELD);
    assert!(hints[0].contains("worker-tab"));
    let held = LaneRecord {
        cleanup_status: Some(CLEANUP_HELD.into()),
        ..finished_lane("a")
    };
    assert!(
        !held.wants(SettleTrigger::Decided),
        "no recheck before the close"
    );
    assert!(held.bound_to("worker-tab") && held.wants(SettleTrigger::HolderClosed));
    assert!(!held.bound_to("other-tab"));

    let mut closed = FakeGit::answering(Ancestry::Merged);
    assert_eq!(
        settle(&held, CANONICAL, true, &mut closed),
        Settlement::Removed
    );
    assert_eq!(closed.ancestry_calls.len(), 1);
    assert!(!closed.removals[0].force);

    for status in [CLEANUP_DONE] {
        let settled = LaneRecord {
            cleanup_status: Some(status.into()),
            ..finished_lane("a")
        };
        assert!(
            !settled.wants(SettleTrigger::HolderClosed),
            "{status} is not revisited on close"
        );
    }
    for status in [CLEANUP_RETAINED, CLEANUP_OFFERED, "cleanup_debt"] {
        let settled = LaneRecord {
            cleanup_status: Some(status.into()),
            ..finished_lane("a")
        };
        assert!(settled.wants(SettleTrigger::HolderClosed));
        assert!(!settled.wants(SettleTrigger::Decided));
    }
}

#[test]
fn lane_settle_unmerged_lane_is_kept_and_recorded_with_reason() {
    let mut git = FakeGit::answering(Ancestry::Unmerged);

    let settlement = settle(&finished_lane("a"), CANONICAL, true, &mut git);

    assert!(git.removals.is_empty(), "unmerged work is never removed");
    let (status, hints) = recorded(&settlement);
    assert_eq!(status, CLEANUP_RETAINED);
    assert!(hints[0].contains("not merged into HEAD"));
    assert!(hints[1].starts_with("domios plan lane-remove"));
    let update = lane_update(&settlement).unwrap();
    assert_eq!(update.worktree_path, None, "the lane reference stays");
    assert_eq!(update.branch, None);
}

#[test]
fn lane_settle_dirty_worktree_refusal_is_recorded_not_forced() {
    let mut git = FakeGit::answering(Ancestry::Merged);
    git.removal = Err("'/lanes/a' contains modified or untracked files".into());

    let settlement = settle(&finished_lane("a"), CANONICAL, true, &mut git);

    assert_eq!(git.removals.len(), 1);
    assert!(!git.removals[0].force);
    let (status, hints) = recorded(&settlement);
    assert_eq!(status, CLEANUP_RETAINED);
    assert!(hints[0].contains("modified or untracked"));
}

#[test]
fn lane_settle_gone_branch_and_worktree_is_terminal_without_removal() {
    let mut git = FakeGit::answering(Ancestry::BranchGone);

    let settlement = settle(&finished_lane("a"), CANONICAL, false, &mut git);

    assert!(git.removals.is_empty());
    assert_eq!(git.ancestry_calls.len(), 1);
    assert_eq!(recorded(&settlement).0, CLEANUP_DONE);
    let update = lane_update(&settlement).unwrap();
    assert_eq!(update.worktree_path, Some(None));
    assert_eq!(update.branch, Some(None));
    assert_eq!(update.cleanup_status, Some(Some(CLEANUP_DONE.into())));
}

#[test]
fn lane_settle_unknown_ancestry_is_recorded_once_with_bounded_reason() {
    let mut git = FakeGit::answering(Ancestry::Unknown("x".repeat(2_000)));

    let settlement = settle(&finished_lane("a"), CANONICAL, true, &mut git);

    assert!(git.removals.is_empty());
    let (status, hints) = recorded(&settlement);
    assert_eq!(status, CLEANUP_RETAINED);
    assert!(hints[0].chars().count() <= MAX_REASON_CHARS + 1);
}

#[test]
fn lane_settle_peer_lane_is_offered_without_git() {
    let mut git = FakeGit::answering(Ancestry::Merged);
    let record = LaneRecord {
        peer: Some("laptop".into()),
        ..finished_lane("a")
    };

    let settlement = settle(&record, CANONICAL, true, &mut git);

    assert!(git.ancestry_calls.is_empty());
    assert!(git.removals.is_empty());
    assert_eq!(recorded(&settlement).0, CLEANUP_OFFERED);
}

#[test]
fn lane_settle_skips_terminal_active_and_laneless_records() {
    assert!(finished_lane("a").needs_settlement());
    for status in [
        CLEANUP_DONE,
        CLEANUP_OFFERED,
        CLEANUP_RETAINED,
        "cleanup_debt",
    ] {
        let settled = LaneRecord {
            cleanup_status: Some(status.into()),
            ..finished_lane("a")
        };
        assert!(!settled.needs_settlement(), "{status} is final");
    }
    let active = LaneRecord {
        terminal: false,
        ..finished_lane("a")
    };
    assert!(!active.needs_settlement());
    let laneless = LaneRecord {
        worktree: None,
        branch: None,
        ..finished_lane("a")
    };
    assert!(!laneless.needs_settlement());
}

#[test]
fn lane_settle_history_marks_every_finished_lane_terminal_without_git() {
    let settled = LaneRecord {
        cleanup_status: Some(CLEANUP_DONE.into()),
        ..finished_lane("settled")
    };
    let active = LaneRecord {
        terminal: false,
        ..finished_lane("active")
    };
    let peer = LaneRecord {
        peer: Some("laptop".into()),
        ..finished_lane("peer")
    };
    let records = [
        finished_lane("present"),
        finished_lane("vanished"),
        settled,
        active,
        peer,
    ];

    let marked = settle_history(&records, CANONICAL, &|path: &str| path == "/lanes/present");

    let ids: Vec<_> = marked.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["present", "vanished", "peer"]);
    let (status, hints) = recorded(&marked[0].1);
    assert_eq!(status, CLEANUP_OFFERED);
    assert!(hints[0].starts_with("domios plan lane-remove '/lanes/present'"));
    assert_eq!(recorded(&marked[1].1).0, CLEANUP_DONE);
    let vanished = lane_update(&marked[1].1).unwrap();
    assert_eq!(vanished.worktree_path, Some(None));
    assert_eq!(vanished.branch, None, "a surviving branch stays recorded");
    assert_eq!(recorded(&marked[2].1).0, CLEANUP_OFFERED);

    let remarked: Vec<_> = records
        .iter()
        .map(|record| {
            let marked_status = marked
                .iter()
                .find(|(id, _)| *id == record.task_id)
                .and_then(|(_, settlement)| lane_update(settlement))
                .and_then(|update| update.cleanup_status.flatten());
            LaneRecord {
                cleanup_status: marked_status.or(record.cleanup_status.clone()),
                ..record.clone()
            }
        })
        .collect();
    assert!(remarked.iter().all(|record| !record.needs_settlement()));
    assert!(settle_history(&remarked, CANONICAL, &|_: &str| true).is_empty());
}

#[test]
fn lane_settle_repository_changes_only_reach_the_ui() {
    let ui = Arc::new(crate::service::test_fixture::RecordedUiEvents::default());
    let sink = crate::repowatch::TauriRepoEventSink::new(ui.clone());

    codeterm_repowatch::RepoEventSink::repository_changed(&sink, CANONICAL);

    let events = ui.0.lock();
    let names: Vec<_> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["repository-changed"]);
}

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn lane_settle_git_ancestry_classifies_merged_unmerged_and_deleted_branches() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    git(path, &["init", "-q", "-b", "master"]);
    git(path, &["config", "user.name", "Lane Settle Test"]);
    git(
        path,
        &["config", "user.email", "lane-settle@example.invalid"],
    );
    git(path, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(path, &["branch", "lane/merged"]);
    git(path, &["checkout", "-q", "-b", "lane/open"]);
    git(path, &["commit", "-q", "--allow-empty", "-m", "lane work"]);
    git(path, &["checkout", "-q", "master"]);
    let canonical = path.to_str().unwrap();

    assert_eq!(
        git_ancestry(canonical, "lane/merged", "master"),
        (Ancestry::Merged, 1)
    );
    assert_eq!(
        git_ancestry(canonical, "lane/open", "master"),
        (Ancestry::Unmerged, 1)
    );
    assert_eq!(
        git_ancestry(canonical, "lane/deleted", "master"),
        (Ancestry::BranchGone, 2)
    );
}
