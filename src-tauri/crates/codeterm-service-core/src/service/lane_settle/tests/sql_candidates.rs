//! Differential check: the startup SQL candidates equal the Rust lane rule for
//! split task rows and for legacy plans that keep their tasks inline.

use super::super::finished_records;
use crate::plan::{OrchestratorRef, Plan, TaskStatus};
use crate::service::plan::{NewTask, PlanStore};
use std::collections::{BTreeMap, BTreeSet};

type Lane = (Option<&'static str>, Option<&'static str>);

const BLANK_CLEANUPS: [Option<&str>; 5] = [
    None,
    Some(""),
    Some(" \t\n"),
    Some("\u{3000}"),
    Some("\u{a0}\u{2028}\u{85}"),
];
const SETTLED_CLEANUPS: [&str; 4] = ["removed", "offered", " retained ", "\u{3000}cleanup_debt"];
const LANES: [Lane; 6] = [
    (None, None),
    (Some(""), None),
    (Some(" "), Some("\u{3000}")),
    (Some("/lanes/a"), None),
    (None, Some("lane/a")),
    (Some("\u{2003}"), Some("lane/b")),
];

struct Case {
    status: TaskStatus,
    cleanup: Option<&'static str>,
    lane: Lane,
}

fn cases() -> Vec<Case> {
    let cleanups = BLANK_CLEANUPS.into_iter().chain(SETTLED_CLEANUPS.map(Some));
    let mut cases: Vec<Case> = cleanups
        .flat_map(|cleanup| {
            LANES.map(|lane| Case {
                status: TaskStatus::Done,
                cleanup,
                lane,
            })
        })
        .collect();
    for status in [
        TaskStatus::Cancelled,
        TaskStatus::InProgress,
        TaskStatus::Paused,
    ] {
        for cleanup in [None, Some("\u{3000}"), Some("removed")] {
            for lane in [
                (None, Some("lane/a")),
                (Some("\u{2003}"), None),
                (Some("/lanes/a"), None),
            ] {
                cases.push(Case {
                    status: status.clone(),
                    cleanup,
                    lane,
                });
            }
        }
    }
    cases
}

/// (`$.tasks` JSON type left in the plan row, task row count) for one plan.
fn stored_shape(connection: &rusqlite::Connection, plan_id: &str) -> (Option<String>, i64) {
    connection
        .query_row(
            "SELECT json_type(document_json, '$.tasks'),
                    (SELECT count(*) FROM plan_tasks WHERE plan_id = ?1)
             FROM plans WHERE id = ?1",
            [plan_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[test]
fn startup_perf_unsettled_lane_candidates_match_the_rust_rule() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join(".domios").join("plans.db");
    let store = PlanStore::new_for_test(dir.path());
    let project = dir.path().display().to_string();
    let mut template = Plan::new(
        "template".into(),
        project.clone(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut template, 1).unwrap();
    store
        .add_task(
            &template.id,
            NewTask {
                name: "Lane".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let task = store.get(&template.id).unwrap().tasks.remove(0);

    let mut labels = BTreeMap::new();
    let mut unicode_blank = Vec::new();
    for case in cases() {
        for inline in [false, true] {
            let mut probe = task.clone();
            probe.status = case.status.clone();
            probe.lane.cleanup_status = case.cleanup.map(str::to_string);
            probe.lane.worktree_path = case.lane.0.map(str::to_string);
            probe.lane.branch = case.lane.1.map(str::to_string);
            let mut plan = Plan::new(
                "lane case".into(),
                project.clone(),
                OrchestratorRef::from_tab("owner"),
            );
            // A repeated task id keeps the tasks inline in the plan row.
            plan.tasks = if inline {
                vec![probe.clone(), probe]
            } else {
                vec![probe]
            };
            store.save(&mut plan, 3).unwrap();
            if case.status == TaskStatus::Done
                && case.cleanup == Some("\u{3000}")
                && case.lane == (None, Some("lane/a"))
            {
                unicode_blank.push(plan.id.clone());
            }
            let label = format!(
                "inline={inline} {:?} cleanup={:?} lane={:?}",
                case.status, case.cleanup, case.lane
            );
            labels.insert(plan.id, (inline, label));
        }
    }
    drop(store);

    let reopened = PlanStore::open(database.clone()).unwrap();
    let candidates: BTreeSet<String> = reopened
        .plan_ids_with_unsettled_lanes()
        .into_iter()
        .collect();
    let connection = rusqlite::Connection::open(&database).unwrap();
    let mut mismatches = Vec::new();
    let mut hits = [0usize; 2];
    for (plan_id, (inline, label)) in &labels {
        let expected_shape = if *inline {
            (Some("array".to_string()), 0)
        } else {
            (None, 1)
        };
        assert_eq!(
            stored_shape(&connection, plan_id),
            expected_shape,
            "{label}"
        );
        let plan = reopened.get(plan_id).unwrap();
        let rust = !finished_records(&plan).is_empty();
        if rust != candidates.contains(plan_id) {
            mismatches.push(format!("{label}: rust={rust}"));
        }
        hits[usize::from(*inline)] += usize::from(rust);
    }

    assert!(
        mismatches.is_empty(),
        "SQL differs from Rust: {mismatches:#?}"
    );
    assert!(hits.iter().all(|count| *count > 0), "{hits:?}");
    assert!(!candidates.contains(&template.id));
    assert_eq!(unicode_blank.len(), 2);
    assert!(
        unicode_blank
            .iter()
            .all(|plan_id| candidates.contains(plan_id)),
        "a Unicode-blank cleanup status is still unsettled"
    );
}
