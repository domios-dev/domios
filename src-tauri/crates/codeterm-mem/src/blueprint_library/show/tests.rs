use super::*;
use rusqlite::params;
use serde_json::{json, Value};

const AREA: &str = "blueprints:workspace";

fn callable() -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"constant","inputs":{"n":{"type":"integer"}},"steps":[{"id":"value","code":{"lang":"python","src":"def main(p): return 8"},"in":{"type":"integer"},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"value.out"}})).unwrap()
}

fn caller(id: &str, name: &str) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":name,"inputs":{"n":{"type":"integer"}},"steps":[{"id":"call","call":{"ref":format!("mem:{id}"),"with":{"n":{"input":"n"}}},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"call.out"}})).unwrap()
}

fn save(mem: &MemManager, document: Document) -> String {
    mem.save_blueprint_scoped(
        AREA,
        "team",
        document,
        &["Return fixed data".into()],
        Evidence {
            kept: true,
            successes: 1,
            ..Default::default()
        },
    )
    .unwrap()
    .id
}

fn nested(mem: &MemManager) -> (String, String, String) {
    let leaf = save(mem, callable());
    let middle = save(mem, caller(&leaf, "middle"));
    let root = save(mem, caller(&middle, "root"));
    (root, middle, leaf)
}

fn view(mem: &MemManager, id: &str) -> Value {
    serde_json::to_value(mem.blueprint_solution_show(AREA, id).unwrap()).unwrap()
}

#[test]
fn saved_solution_show_expands_nested_frozen_calls_without_writes() {
    let mem = MemManager::open_in_memory().unwrap();
    let (root, middle, leaf) = nested(&mem);
    let changes = mem.conn.lock().total_changes();
    let shown = view(&mem, &format!("mem:{root}"));
    assert_eq!(shown["root"]["name"], "root");
    assert_eq!(shown["calls"][0]["pin"]["id"], format!("mem:{middle}"));
    assert_eq!(shown["calls"][0]["graph"]["name"], "middle");
    assert_eq!(
        shown["calls"][0]["children"][0]["pin"]["id"],
        format!("mem:{leaf}")
    );
    assert_eq!(
        shown["calls"][0]["children"][0]["graph"]["name"],
        "constant"
    );
    assert!(shown["calls"][0].get("unavailable").is_none());
    assert_eq!(mem.conn.lock().total_changes(), changes);
}

#[test]
fn saved_solution_show_missing_pin_is_typed_and_preserves_available_parent() {
    let mem = MemManager::open_in_memory().unwrap();
    let (root, _, leaf) = nested(&mem);
    mem.conn
        .lock()
        .execute(
            "DELETE FROM blueprint_call_revisions WHERE item_id=?1",
            [&leaf],
        )
        .unwrap();
    let shown = view(&mem, &root);
    assert_eq!(shown["calls"][0]["graph"]["name"], "middle");
    let missing = &shown["calls"][0]["children"][0];
    assert_eq!(missing["unavailable"], "pinned_revision_unavailable");
    assert_eq!(missing["graph"], Value::Null);
    assert_eq!(missing["children"], json!([]));
}

#[test]
fn saved_solution_show_refuses_foreign_workspace_and_archived_root() {
    let mem = MemManager::open_in_memory().unwrap();
    let (root, _, _) = nested(&mem);
    for area in ["", "blueprints:foreign"] {
        assert!(mem.blueprint_solution_show(area, &root).is_err());
    }
    mem.archive_blueprint_solution(AREA, &root).unwrap();
    assert!(mem.blueprint_solution_show(AREA, &root).is_err());
}

#[test]
fn saved_solution_show_never_reads_foreign_team_or_workspace_pin_bytes() {
    for (area, team) in [(AREA, "foreign"), ("blueprints:foreign", "team")] {
        let mem = MemManager::open_in_memory().unwrap();
        let (root, middle, _) = nested(&mem);
        mem.conn
            .lock()
            .execute(
                "UPDATE blueprint_call_scopes SET area=?1,team=?2 WHERE item_id=?3",
                params![area, team, middle],
            )
            .unwrap();
        let shown = view(&mem, &root);
        assert_eq!(
            shown["calls"][0]["unavailable"],
            "pinned_revision_unavailable"
        );
        assert_eq!(shown["calls"][0]["graph"], Value::Null);
    }
}

#[test]
fn saved_solution_show_matches_applied_graph_projection() {
    let mem = MemManager::open_in_memory().unwrap();
    let (root, _, _) = nested(&mem);
    let document = mem
        .blueprint_callable(AREA, "team", &format!("mem:{root}"))
        .unwrap();
    let closure = calls::FrozenClosure::from_document(document).unwrap();
    let expected = calls::projection::graph(&closure).unwrap();
    assert_eq!(view(&mem, &root), serde_json::to_value(expected).unwrap());
}
