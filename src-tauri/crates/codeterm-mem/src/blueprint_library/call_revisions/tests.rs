use super::*;
use codeterm_blueprint_types::blueprint_document::{calls::CallPin, projection};
use serde_json::json;

fn callable(n: i64) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"constant","inputs":{"n":{"type":"integer"}},"steps":[{"id":"value","code":{"lang":"python","src":format!("def main(p): return {n}")},"in":{"type":"integer"},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"value.out"}})).unwrap()
}
fn caller(id: &str) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"caller","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"call","call":{"ref":format!("mem:{id}"),"with":{"n":{"lit":7}}},"out":{"type":"integer"}}],"result":{"to":"notify"}})).unwrap()
}
fn evidence() -> Evidence {
    Evidence {
        kept: true,
        successes: 1,
        ..Default::default()
    }
}
fn save(mem: &MemManager, doc: Document) -> String {
    mem.save_blueprint_scoped(
        "blueprints:workspace",
        "team",
        doc,
        &["Return fixed data".into()],
        evidence(),
    )
    .unwrap()
    .id
}
fn load(mem: &MemManager, id: &str) -> Document {
    mem.blueprint_callable("blueprints:workspace", "team", &format!("mem:{id}"))
        .unwrap()
}
fn pin(document: &Document) -> CallPin {
    document.steps[0]
        .call
        .as_ref()
        .unwrap()
        .pin
        .clone()
        .unwrap()
}
fn edit(mem: &MemManager, id: &str, mut document: Document, note: &str) {
    document.note = Some(note.into());
    let conn = mem.conn.lock();
    store::update(
        &conn,
        id,
        &store::MemUpdate {
            body: Some(document.yaml().unwrap()),
            ..Default::default()
        },
        &mem.blobs_dir,
    )
    .unwrap();
}
#[test]
fn blueprint_v2_call_save_pins_id_hash_and_resave_does_not_repin() {
    let mem = MemManager::open_in_memory().unwrap();
    let callee = save(&mem, callable(8));
    let caller = save(&mem, caller(&callee));
    let before = load(&mem, &caller);
    let original = pin(&before);
    assert_eq!(original.id, format!("mem:{callee}"));
    assert_eq!(
        original.hash,
        projection::identity(&load(&mem, &callee)).unwrap()
    );
    assert_eq!(before.dependencies.len(), 1);
    edit(&mem, &callee, callable(99), "edited head");
    let saved = save(&mem, before.clone());
    let after = load(&mem, &saved);
    assert_eq!(pin(&after), original);
    let warnings = mem
        .blueprint_call_drift("blueprints:workspace", "team", after)
        .unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].diff.changed["value"].contains(&"code".into()));
    assert!(warnings[0].diff.document_fields.contains(&"note".into()));
    let pinned = mem
        .blueprint_call_revision("blueprints:workspace", "team", &original)
        .unwrap();
    assert!(pinned.steps[0]
        .code
        .as_ref()
        .unwrap()
        .src
        .as_ref()
        .unwrap()
        .contains("return 8"));
}
#[test]
fn blueprint_v2_call_missing_pinned_hash_and_missing_callee_refuse_without_repins() {
    for missing_callee in [false, true] {
        let mem = MemManager::open_in_memory().unwrap();
        let callee = save(&mem, callable(8));
        let caller = save(&mem, caller(&callee));
        let before = load(&mem, &caller);
        let original = pin(&before);
        {
            let conn = mem.conn.lock();
            if missing_callee {
                conn.execute("DELETE FROM mem_items WHERE id=?1", [&callee])
                    .unwrap();
            } else {
                conn.execute(
                    "DELETE FROM blueprint_call_revisions WHERE item_id=?1 AND hash=?2",
                    params![callee, original.hash],
                )
                .unwrap();
            }
        }
        assert!(mem
            .pin_blueprint_calls("blueprints:workspace", "team", before.clone())
            .is_err());
        assert!(mem
            .blueprint_call_drift("blueprints:workspace", "team", before)
            .is_err());
    }
}
#[test]
fn blueprint_v2_call_cross_team_and_workspace_resolution_refuses() {
    let mem = MemManager::open_in_memory().unwrap();
    let callee = save(&mem, callable(8));
    for (area, team) in [
        ("blueprints:elsewhere", "team"),
        ("blueprints:workspace", "foreign"),
    ] {
        assert!(mem
            .save_blueprint_scoped(
                area,
                team,
                caller(&callee),
                &["Return fixed data".into()],
                evidence()
            )
            .unwrap_err()
            .contains("outside caller Team/workspace"));
        assert!(mem
            .blueprint_callable(area, team, &format!("mem:{callee}"))
            .is_err());
    }
}
#[test]
fn blueprint_v2_call_pin_failure_rolls_back_saved_caller() {
    let mem = MemManager::open_in_memory().unwrap();
    let before = mem
        .list_blueprint_solutions("blueprints:workspace")
        .unwrap()
        .len();
    assert!(mem
        .save_blueprint_scoped(
            "blueprints:workspace",
            "team",
            caller("missing"),
            &["Return fixed data".into()],
            evidence()
        )
        .is_err());
    assert_eq!(
        mem.list_blueprint_solutions("blueprints:workspace")
            .unwrap()
            .len(),
        before
    );
}
#[test]
fn blueprint_v2_call_transitive_snapshot_drift_cannot_evade_parent_pin() {
    let mem = MemManager::open_in_memory().unwrap();
    let leaf = save(&mem, callable(8));
    let mut middle = caller(&leaf);
    middle.name = "middle".into();
    middle.inputs.insert(
        "n".into(),
        serde_json::from_value(json!({"type":"integer"})).unwrap(),
    );
    middle.steps[0].call.as_mut().unwrap().arguments.insert(
        "n".into(),
        codeterm_blueprint_types::blueprint_document::ValueForm::input("n"),
    );
    middle.trigger = None;
    middle.result.to = None;
    middle.result.port = Some(serde_json::from_value(json!({"type":"integer"})).unwrap());
    middle.result.from = Some("call.out".into());
    let middle_id = save(&mem, middle);
    let outer = save(&mem, caller(&middle_id));
    let before = load(&mem, &outer);
    let original = pin(&before);
    assert_eq!(before.dependencies.len(), 2);
    edit(&mem, &leaf, callable(99), "dream edit");
    let warnings = mem
        .blueprint_call_drift("blueprints:workspace", "team", before.clone())
        .unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].pin.id, format!("mem:{leaf}"));
    assert_eq!(
        pin(&mem
            .pin_blueprint_calls("blueprints:workspace", "team", before)
            .unwrap()),
        original
    );
}
#[test]
fn blueprint_v2_call_immutable_revision_survives_merge_dream_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let (callee_id, pinned, caller_id) = {
        let mem = MemManager::open_offline(directory.path()).unwrap();
        let callee = save(&mem, callable(8));
        let caller = save(&mem, caller(&callee));
        let pinned = pin(&load(&mem, &caller));
        edit(&mem, &callee, callable(99), "head changed before merge");
        let _ = mem.dream_blueprints().unwrap();
        (callee, pinned, caller)
    };
    let mem = MemManager::open_offline(directory.path()).unwrap();
    let document = load(&mem, &caller_id);
    assert_eq!(pin(&document), pinned);
    let revision = mem
        .blueprint_call_revision("blueprints:workspace", "team", &pinned)
        .unwrap();
    assert!(revision.steps[0]
        .code
        .as_ref()
        .unwrap()
        .src
        .as_ref()
        .unwrap()
        .contains("return 8"));
    assert_eq!(
        mem.blueprint_call_drift("blueprints:workspace", "team", document)
            .unwrap()[0]
            .pin
            .id,
        format!("mem:{callee_id}")
    );
}
