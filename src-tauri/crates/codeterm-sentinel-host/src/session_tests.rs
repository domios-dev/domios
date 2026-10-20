use codeterm_sentinel::session::*;

#[test]
fn blueprint_session_safety_counts_share_graph_and_exclude_foreign_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::tests::store(
        dir.path(),
        codeterm_cron::clock::MockClock::starting_at(1_000),
    );
    let mut root = super::tests::create(&mut store, "team");
    root.transitions
        .push(codeterm_sentinel::handlers::Transition {
            causal_run: None,
            decision: None,
            id: "root-event".into(),
            bucket: root.buckets[0].clone(),
            accepted_at: 0,
            terminal: false,
            handlers: Vec::new(),
        });
    let mut child = root.clone();
    child.id = "child".into();
    child.scenario_id = Some(root.id.clone());
    child.transitions[0].accepted_at = 1;
    let mut foreign = child.clone();
    foreign.team_id = Some("foreign".into());
    let usage = usage(&[root.clone(), child, foreign], &root, 60_000);
    assert_eq!(usage.exchanges, 2);
    assert_eq!(usage.recent, 1);
    assert_eq!(usage.actions, 0);
}
