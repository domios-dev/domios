use super::*;
use codeterm_blueprint_types::blueprint_journal::JournalMarker;
fn admissible(
    conn: &Connection,
    id: &str,
    run: &str,
    generation: u64,
    marker: i64,
    parent: Option<&NestedClaimParent>,
) -> Result<bool, StorageError> {
    super::admissible(
        conn,
        id,
        run,
        generation,
        JournalMarker::from_columns(marker, "call").unwrap(),
        parent,
    )
}

fn connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE blueprint_step_journal(entry_id INTEGER PRIMARY KEY AUTOINCREMENT,blueprint_id TEXT,run_id TEXT,step_index INTEGER,step_name TEXT,state TEXT,claim_generation INTEGER,claim_parent_index INTEGER,call_stack_json TEXT);").unwrap();
    conn
}
fn frame() -> CallFrame {
    CallFrame {
        node: "call".into(),
        id: "mem:callee".into(),
        hash: "a".repeat(64),
    }
}
fn parent(marker: u16) -> NestedClaimParent {
    NestedClaimParent {
        marker: JournalMarker::Step(u32::from(marker)),
        frame: frame(),
    }
}
fn add(conn: &Connection, marker: u16, ancestor: Option<u16>, state: &str) {
    conn.execute("INSERT INTO blueprint_step_journal(blueprint_id,run_id,step_index,step_name,state,claim_generation,claim_parent_index,call_stack_json) VALUES('job','run',?1,'call',?2,3,?3,?4)",params![marker,state,ancestor,serde_json::to_string(&vec![frame()]).unwrap()]).unwrap();
}
#[test]
fn blueprint_v2_call_nested_claim_admits_only_verified_parent_and_generation() {
    let conn = connection();
    add(&conn, 0, None, "running");
    assert!(admissible(&conn, "job", "run", 3, 1, Some(&parent(0))).unwrap());
    assert!(!admissible(&conn, "job", "run", 4, 1, Some(&parent(0))).unwrap());
    assert!(!admissible(&conn, "job", "foreign-run", 3, 1, Some(&parent(0))).unwrap());
    assert!(!admissible(&conn, "job", "run", 3, 1, Some(&parent(2))).unwrap());
    let mut wrong = parent(0);
    wrong.frame.hash = "b".repeat(64);
    assert!(!admissible(&conn, "job", "run", 3, 1, Some(&wrong)).unwrap());
    add(&conn, 0, None, "done");
    assert!(!admissible(&conn, "job", "run", 3, 1, Some(&parent(0))).unwrap());
}
#[test]
fn blueprint_v2_call_plain_second_claim_still_refuses() {
    let conn = connection();
    assert!(admissible(&conn, "job", "run", 3, 0, None).unwrap());
    add(&conn, 0, None, "running");
    assert!(!admissible(&conn, "job", "run", 3, 1, None).unwrap());
}
#[test]
fn blueprint_v2_call_claim_ancestor_chain_cannot_skip_active_child() {
    let conn = connection();
    add(&conn, 0, None, "running");
    add(&conn, 1, Some(0), "running");
    assert!(admissible(&conn, "job", "run", 3, 2, Some(&parent(1))).unwrap());
    assert!(!admissible(&conn, "job", "run", 3, 2, Some(&parent(0))).unwrap());
    add(&conn, 1, Some(0), "done");
    assert!(admissible(&conn, "job", "run", 3, 2, Some(&parent(0))).unwrap());
}
#[test]
fn blueprint_v2_call_crash_replay_retains_uncertain_parent_and_refuses_completed_child() {
    let conn = connection();
    add(&conn, 0, None, "running");
    add(&conn, 1, Some(0), "running");
    add(&conn, 1, Some(0), "done");
    assert!(!admissible(&conn, "job", "run", 3, 0, None).unwrap());
    assert!(!admissible(&conn, "job", "run", 3, 1, Some(&parent(0))).unwrap());
    assert!(!admissible(&conn, "job", "reopened", 3, 0, None).unwrap());
    let parent_running: i64 = conn
        .query_row(
            "SELECT count(*) FROM blueprint_step_journal WHERE step_index=0 AND state='running'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(parent_running, 1);
}
#[test]
fn blueprint_v2_call_claim_depth_nine_refuses() {
    let conn = connection();
    for n in 0..8 {
        add(&conn, n, n.checked_sub(1), "running");
    }
    assert!(admissible(&conn, "job", "run", 3, 8, Some(&parent(7))).unwrap());
    add(&conn, 8, Some(7), "running");
    assert!(!admissible(&conn, "job", "run", 3, 9, Some(&parent(8))).unwrap());
}

#[test]
fn plan_lock_run_hold_does_not_consume_the_result_claim_identity() {
    let conn = connection();
    conn.execute("INSERT INTO blueprint_step_journal(blueprint_id,run_id,step_index,step_name,state) VALUES('job','run',-1,'$run','held')", []).unwrap();
    assert!(super::admissible(&conn, "job", "run", 3, JournalMarker::Result, None).unwrap());
    assert!(!super::admissible(&conn, "job", "run", 3, JournalMarker::Run, None).unwrap());
    conn.execute("INSERT INTO blueprint_step_journal(blueprint_id,run_id,step_index,step_name,state) VALUES('job','run',-1,'$result','done')", []).unwrap();
    assert!(!super::admissible(&conn, "job", "run", 3, JournalMarker::Result, None).unwrap());
}
