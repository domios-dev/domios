use super::*;
use crate::plan::PlanStatus;
use crate::service_plan::load_audit::{assert_never_loaded, start};
use std::collections::BTreeSet;

fn attempt(placement: WorkerPlacement, closed: bool, retired: bool) -> TaskExecutionAttempt {
    TaskExecutionAttempt {
        id: format!("attempt-{placement:?}-{closed}-{retired}"),
        placement,
        closed_at: closed.then_some(2),
        retired_at: retired.then_some(2),
        ..Default::default()
    }
}

fn save_peer(
    store: &PlanStore,
    path: &str,
    canonical: Option<&str>,
    attempts: Vec<TaskExecutionAttempt>,
    archived: bool,
) -> String {
    let mut peer = super::plan(path);
    if let Some(root) = canonical {
        peer.canonical_project_path = root.into();
    }
    if archived {
        peer.status = PlanStatus::Archived;
    }
    let mut task = super::task("peer", &["src/shared.rs"], false);
    task.execution_attempts = attempts;
    peer.tasks.push(task);
    store.save(&mut peer, 1).unwrap();
    peer.id
}

#[test]
fn checkout_peer_plans_never_loads_unmatched_plans() {
    let root = tempfile::tempdir().unwrap();
    let foreign = tempfile::tempdir().unwrap();
    let checkout = root.path().display().to_string();
    let foreign_path = foreign.path().display().to_string();
    let alias = root.path().join(".").display().to_string();
    let store = PlanStore::new_for_test(root.path());
    let shared = save_peer(
        &store,
        &checkout,
        None,
        vec![attempt(WorkerPlacement::Shared, false, false)],
        false,
    );
    let exclusive = save_peer(
        &store,
        &checkout,
        None,
        vec![attempt(WorkerPlacement::CanonicalWriter, false, false)],
        true,
    );
    let aliased = save_peer(
        &store,
        &alias,
        None,
        vec![attempt(WorkerPlacement::Shared, false, false)],
        false,
    );
    let overridden = save_peer(
        &store,
        &foreign_path,
        Some(&checkout),
        vec![attempt(WorkerPlacement::CanonicalWriter, false, false)],
        false,
    );
    let unmatched = [
        save_peer(&store, &checkout, None, vec![], false),
        save_peer(
            &store,
            &checkout,
            None,
            vec![attempt(WorkerPlacement::IsolatedLane, false, false)],
            false,
        ),
        save_peer(
            &store,
            &checkout,
            None,
            vec![attempt(WorkerPlacement::Shared, true, false)],
            false,
        ),
        save_peer(
            &store,
            &checkout,
            None,
            vec![attempt(WorkerPlacement::CanonicalWriter, false, true)],
            false,
        ),
        save_peer(
            &store,
            &foreign_path,
            None,
            vec![attempt(WorkerPlacement::Shared, false, false)],
            false,
        ),
        save_peer(
            &store,
            &foreign_path,
            None,
            vec![attempt(WorkerPlacement::CanonicalWriter, false, false)],
            false,
        ),
        save_peer(
            &store,
            &checkout,
            None,
            vec![
                attempt(WorkerPlacement::Shared, true, false),
                attempt(WorkerPlacement::IsolatedLane, false, false),
            ],
            false,
        ),
    ];
    start(&store);

    let peers = store.checkout_peer_plans(&super::plan(&checkout));

    let expected = BTreeSet::from([shared, exclusive, aliased, overridden]);
    assert_eq!(
        peers
            .into_iter()
            .map(|plan| plan.id)
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert_eq!(
        store
            .plan_loads_for_test()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert_never_loaded(
        &store,
        &unmatched.iter().map(String::as_str).collect::<Vec<_>>(),
    );
}
