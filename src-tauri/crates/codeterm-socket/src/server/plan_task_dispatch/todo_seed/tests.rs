use super::*;

fn attempt(tab: &str, finished: bool) -> TaskExecutionAttempt {
    TaskExecutionAttempt {
        executor_tab_id: tab.into(),
        closed_at: finished.then_some(10),
        ..Default::default()
    }
}

#[test]
fn predecessors_are_finished_local_non_competitive_executors_newest_first() {
    let mut competitor = attempt("competitor", true);
    competitor.competition_group_id = Some("group".into());
    let mut remote = attempt("remote", true);
    remote.mesh_peer = Some("peer".into());
    let mut retired = attempt("retired", false);
    retired.retired_at = Some(11);
    let attempts = vec![
        attempt("oldest", true),
        retired,
        attempt("live", false),
        competitor,
        remote,
        attempt("oldest", true),
        attempt("new-tab", true),
    ];
    assert_eq!(
        predecessor_tab_ids(&attempts, "new-tab"),
        ["oldest", "retired"]
    );
}
