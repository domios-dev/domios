use super::*;
use crate::store;
use codeterm_blueprint_types::blueprint_document::{calls::CallPin, projection};
use rusqlite::params;
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
    let conn = mem.fixture_conn().lock();
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
fn blueprint_v2_call_keep_preserves_distinct_bodies_with_same_name_and_scope() {
    let mem = MemManager::open_in_memory().unwrap();
    let first = save(&mem, callable(103));
    let error = mem
        .save_blueprint_scoped(
            "blueprints:workspace",
            "team",
            callable(17),
            &["Return seventeen".into()],
            evidence(),
        )
        .unwrap_err();
    assert!(error.starts_with("blueprint_name_conflict:"), "{error}");
    assert!(error.contains("return-seventeen"), "{error}");
    assert_eq!(load(&mem, &first), callable(103));
    let mut seventeen = callable(17);
    seventeen.name = "return-seventeen".into();
    let second = save(&mem, seventeen);
    assert_ne!(first, second);
    let mut renamed = callable(17);
    renamed.name = "Renamed constant".into();
    renamed.note = Some("Presentation only".into());
    assert_eq!(save(&mem, renamed.clone()), second);
    assert_eq!(mem.dream_blueprints().unwrap(), 0);
    let entries = mem
        .list_blueprint_solutions("blueprints:workspace")
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(load(&mem, &first), callable(103));
    assert_eq!(load(&mem, &second), renamed);
}
#[test]
fn blueprint_v2_call_far_purpose_vectors_refuse_keep_and_dream_merges() {
    use super::super::{fakes::WordEmbedder, merge_policy::Purpose};
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let tenfold = vec!["Compute ten times an integer plus three".to_owned()];
    let increment = vec!["Increment a counter by seventeen".to_owned()];
    let first = mem
        .save_blueprint_scoped(
            "blueprints:workspace",
            "team",
            callable(103),
            &tenfold,
            evidence(),
        )
        .unwrap()
        .id;
    let mut renamed = callable(103);
    renamed.name = "increment-seventeen".into();
    let second = mem
        .save_blueprint_scoped(
            "blueprints:workspace",
            "team",
            renamed,
            &increment,
            evidence(),
        )
        .unwrap()
        .id;
    assert_ne!(
        first, second,
        "Even matching code must not merge distant purposes"
    );
    let library = mem
        .list_blueprint_solutions("blueprints:workspace")
        .unwrap();
    assert_eq!(library.len(), 2);
    {
        let conn = mem.fixture_conn().lock();
        let identity = mem.embedder.identity();
        let a = Purpose::stored(
            &crate::backend::sqlite::blueprint::index::read_vectors(
                &conn,
                &library[0].id,
                &identity,
            )
            .unwrap(),
            &identity,
            &library[0],
        )
        .unwrap();
        let b = Purpose::stored(
            &crate::backend::sqlite::blueprint::index::read_vectors(
                &conn,
                &library[1].id,
                &identity,
            )
            .unwrap(),
            &identity,
            &library[1],
        )
        .unwrap();
        assert!(!a.agrees(&b));
    }
    assert_eq!(mem.dream_blueprints().unwrap(), 0);
    assert!(library
        .iter()
        .all(|entry| mem.get(&entry.id).unwrap().unwrap().kind == "skill"));
}
#[test]
fn blueprint_v2_call_same_code_and_purpose_vectors_merge_presentation_variants() {
    use super::super::fakes::WordEmbedder;
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let first = save(&mem, callable(8));
    let mut revised = callable(8);
    revised.name = "Eight".into();
    revised.note = Some("Return eight".into());
    let mut prepared = mem
        .qualify_blueprint(
            "blueprints:workspace",
            revised,
            &["Return fixed data".into()],
            evidence(),
        )
        .unwrap();
    prepared.team = Some("team".into());
    let second = mem
        .persist_blueprint("blueprints:workspace", prepared, false, AREA_CAP)
        .unwrap()
        .id;
    mem.index_blueprint(&second).unwrap();
    assert_ne!(first, second);
    assert_eq!(mem.dream_blueprints().unwrap(), 1);
    assert_eq!(
        mem.list_blueprint_solutions("blueprints:workspace")
            .unwrap()
            .len(),
        1
    );
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
            let conn = mem.fixture_conn().lock();
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
    let directory = crate::test_support::offline_directory();
    let (callee_id, pinned, caller_id) = {
        let mem = MemManager::open_offline(directory.path()).unwrap();
        let callee = save(&mem, callable(8));
        let caller = save(&mem, caller(&callee));
        let pinned = pin(&load(&mem, &caller));
        edit(&mem, &callee, callable(99), "head changed before merge");
        let mut prepared = mem
            .qualify_blueprint(
                "blueprints:workspace",
                callable(99),
                &["Return fixed data".into()],
                evidence(),
            )
            .unwrap();
        prepared.team = Some("team".into());
        let duplicate = mem
            .persist_blueprint("blueprints:workspace", prepared, false, AREA_CAP)
            .unwrap()
            .id;
        assert_ne!(duplicate, callee);
        assert_eq!(
            mem.dream_blueprints().unwrap(),
            1,
            "actual dream merge must occur"
        );
        let kinds = [
            mem.get(&callee).unwrap().unwrap().kind,
            mem.get(&duplicate).unwrap().unwrap().kind,
        ];
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| kind.as_str() == "archived")
                .count(),
            1
        );
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

#[test]
fn blueprint_v2_call_dream_never_merges_across_team_scope() {
    let mem = MemManager::open_in_memory().unwrap();
    let save_team = |team: &str| {
        mem.save_blueprint_scoped(
            "blueprints:workspace",
            team,
            callable(8),
            &["Return fixed data".into()],
            evidence(),
        )
        .unwrap()
        .id
    };
    let first = save_team("first");
    let second = save_team("second");
    assert_ne!(first, second);
    assert_eq!(mem.dream_blueprints().unwrap(), 0);
    for (team, id) in [("first", first), ("second", second)] {
        assert_ne!(mem.get(&id).unwrap().unwrap().kind, "archived");
        assert!(mem
            .blueprint_callable("blueprints:workspace", team, &format!("mem:{id}"))
            .is_ok());
    }
}

fn catalog_caller() -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"catalog-reuse","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"invoke","call":{"ref":"catalog:command-lines","with":{"command":{"lit":"printf fixed"}}},"out":{"$ref":"Lines"}}],"types":{"Lines":{"type":"object","properties":{"count":{"type":"integer"},"lines":{"type":"array","items":{"type":"string"}}},"required":["count","lines"],"additionalProperties":false}},"result":{"to":"notify"}})).unwrap()
}
#[test]
fn blueprint_v2_call_catalog_uses_shared_scoped_immutable_pin_store() {
    let directory = crate::test_support::offline_directory();
    let frozen = {
        let mem = MemManager::open_offline(directory.path()).unwrap();
        let frozen = mem
            .pin_blueprint_calls("blueprints:workspace", "team", catalog_caller())
            .unwrap();
        let pin = pin(&frozen);
        assert_eq!(pin.id, "catalog:command-lines");
        assert_eq!(
            pin.hash,
            projection::identity(&super::super::catalog::document(&pin.id).unwrap()).unwrap()
        );
        let count:i64=mem.fixture_conn().lock().query_row("SELECT count(*) FROM blueprint_call_revisions WHERE item_id=?1 AND hash=?2 AND team='team'",params![pin.id,pin.hash],|r|r.get(0)).unwrap();
        assert_eq!(count, 1);
        frozen
    };
    let mem = MemManager::open_offline(directory.path()).unwrap();
    let pinned = pin(&frozen);
    let retained = mem
        .blueprint_call_revision("blueprints:workspace", "team", &pinned)
        .unwrap();
    assert_eq!(projection::identity(&retained).unwrap(), pinned.hash);
    assert!(mem
        .blueprint_call_revision("blueprints:workspace", "other-team", &pinned)
        .is_err());
    assert_eq!(
        pin(&mem
            .pin_blueprint_calls("blueprints:workspace", "team", frozen)
            .unwrap()),
        pinned
    );
}
#[test]
fn blueprint_v2_call_missing_catalog_hash_never_rebinds_to_embedded_head() {
    let mem = MemManager::open_in_memory().unwrap();
    let frozen = mem
        .pin_blueprint_calls("blueprints:workspace", "team", catalog_caller())
        .unwrap();
    let pin = pin(&frozen);
    mem.fixture_conn()
        .lock()
        .execute(
            "DELETE FROM blueprint_call_revisions WHERE item_id=?1 AND hash=?2",
            params![pin.id, pin.hash],
        )
        .unwrap();
    let failure = mem
        .pin_blueprint_calls("blueprints:workspace", "team", frozen)
        .unwrap_err();
    assert!(failure.contains("Missing pinned hash"), "{failure}");
}

#[test]
fn blueprint_v2_call_review_archive_warns_and_keeps_pins_but_refuses_new_pins() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = save(&mem, callable(8));
    let frozen = mem
        .pin_blueprint_calls("blueprints:workspace", "team", caller(&id))
        .unwrap();
    crate::curation::calibration::archive::archive_item_with_context(
        &MemManager::blueprint_write_context(),
        &mem.fixture_conn().lock(),
        &id,
        "skill",
        "system:blueprint-library",
        "archive",
        Some("blueprint solution area cap"),
    )
    .unwrap();
    let warnings = mem
        .blueprint_call_drift("blueprints:workspace", "team", frozen.clone())
        .unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(serde_json::to_value(&warnings[0]).unwrap()["reason"]
        .as_str()
        .unwrap()
        .contains("cap"));
    assert!(mem
        .pin_blueprint_calls("blueprints:workspace", "team", caller(&id))
        .is_err());
    assert_eq!(
        pin(&mem
            .pin_blueprint_calls("blueprints:workspace", "team", frozen.clone())
            .unwrap()),
        pin(&frozen)
    );
}
#[test]
fn blueprint_v2_call_reviewed_revision_dream_merge_warns_with_keeper_and_diff() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = save(&mem, callable(8));
    let frozen = mem
        .pin_blueprint_calls("blueprints:workspace", "team", caller(&id))
        .unwrap();
    edit(
        &mem,
        &id,
        callable(99),
        "Reviewed revision of the same callable",
    );
    let mut prepared = mem
        .qualify_blueprint(
            "blueprints:workspace",
            callable(99),
            &["Return fixed data".into()],
            Evidence {
                kept: true,
                successes: 100,
                ..Default::default()
            },
        )
        .unwrap();
    prepared.team = Some("team".into());
    let keeper = mem
        .persist_blueprint("blueprints:workspace", prepared, false, AREA_CAP)
        .unwrap()
        .id;
    mem.reuse_blueprints(std::slice::from_ref(&keeper)).unwrap();
    assert_eq!(mem.dream_blueprints().unwrap(), 1);
    assert_eq!(mem.get(&id).unwrap().unwrap().kind, "archived");
    let warnings = mem
        .blueprint_call_drift("blueprints:workspace", "team", frozen.clone())
        .unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(serde_json::to_value(&warnings[0]).unwrap()["reason"]
        .as_str()
        .unwrap()
        .contains(&keeper));
    assert!(warnings[0].diff.changed["value"].contains(&"code".into()));
    assert!(mem
        .blueprint_call_revision("blueprints:workspace", "team", &pin(&frozen))
        .is_ok());
}
#[test]
fn blueprint_v2_call_review_eviction_warns_without_a_body_edit() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = save(&mem, callable(8));
    let frozen = mem
        .pin_blueprint_calls("blueprints:workspace", "team", caller(&id))
        .unwrap();
    let mut replacement = callable(99);
    replacement.name = "return-ninety-nine".into();
    let mut prepared = mem
        .qualify_blueprint(
            "blueprints:workspace",
            replacement,
            &["Return fixed data".into()],
            evidence(),
        )
        .unwrap();
    prepared.team = Some("team".into());
    let saved = mem
        .persist_blueprint("blueprints:workspace", prepared, false, 1)
        .unwrap();
    assert_eq!(saved.evicted_ids, vec![id]);
    let warnings = mem
        .blueprint_call_drift("blueprints:workspace", "team", frozen)
        .unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(serde_json::to_value(&warnings[0]).unwrap()["reason"]
        .as_str()
        .unwrap()
        .contains("cap"));
}

#[test]
fn blueprint_stored_identity_1152_sqlite_revision_and_saved_head_preserve_old_pin() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/blueprint-identities-1.15.2.golden.json"
    )))
    .unwrap();
    let fixture = fixtures
        .as_array()
        .unwrap()
        .iter()
        .find(|fixture| fixture["name"] == "callable")
        .unwrap();
    let document: Document = serde_json::from_value(fixture["document"].clone()).unwrap();
    let mem = MemManager::open_in_memory().unwrap();
    let id = save(&mem, document.clone());
    let recorded = CallPin {
        id: format!("mem:{id}"),
        hash: fixture["hash"].as_str().unwrap().into(),
    };
    {
        let conn = mem.fixture_conn().lock();
        conn.execute("INSERT INTO blueprint_call_revisions(item_id,hash,area,team,document) VALUES(?1,?2,?3,?4,?5)", params![id,recorded.hash,"blueprints:workspace","team",fixture["document"].to_string()]).unwrap();
        conn.execute(
            "DELETE FROM blueprint_call_revisions WHERE item_id=?1 AND hash!=?2",
            params![id, recorded.hash],
        )
        .unwrap();
        store::update(
            &conn,
            &id,
            &store::MemUpdate {
                body: Some(serde_yaml::to_string(&fixture["document"]).unwrap()),
                ..Default::default()
            },
            &mem.blobs_dir,
        )
        .unwrap();
    }
    assert_eq!(
        mem.blueprint_call_revision("blueprints:workspace", "team", &recorded)
            .unwrap(),
        document
    );
    assert_eq!(load(&mem, &id), document);
    assert!(mem
        .list_blueprint_solutions("blueprints:workspace")
        .unwrap()
        .iter()
        .any(|entry| entry.id == id));
    let mut root = caller(&id);
    root.steps[0].call.as_mut().unwrap().arguments.clear();
    root.steps[0].call.as_mut().unwrap().pin = Some(recorded.clone());
    root.steps[0].output = document.result.port.clone();
    let frozen = mem
        .pin_blueprint_calls("blueprints:workspace", "team", root.clone())
        .unwrap();
    assert_eq!(pin(&frozen), recorded);
    assert!(mem
        .blueprint_call_drift("blueprints:workspace", "team", frozen.clone())
        .unwrap()
        .is_empty());
    let mut fresh = root;
    fresh.steps[0].call.as_mut().unwrap().pin = None;
    let fresh = mem
        .pin_blueprint_calls("blueprints:workspace", "team", fresh)
        .unwrap();
    assert_eq!(pin(&fresh).hash, projection::identity(&document).unwrap());
    assert_ne!(pin(&fresh).hash, recorded.hash);
    assert_eq!(
        mem.blueprint_call_revision("blueprints:workspace", "team", &pin(&fresh))
            .unwrap(),
        document
    );
    edit(&mem, &id, document.clone(), "head changed");
    assert_eq!(
        mem.blueprint_call_drift("blueprints:workspace", "team", frozen.clone())
            .unwrap()
            .len(),
        1
    );
    let mut tampered = document;
    tampered.steps[0].run =
        Some(codeterm_blueprint_types::blueprint_document::ValueForm::literal("echo tampered"));
    mem.fixture_conn()
        .lock()
        .execute(
            "UPDATE blueprint_call_revisions SET document=?1 WHERE item_id=?2 AND hash=?3",
            params![serde_json::to_string(&tampered).unwrap(), id, recorded.hash],
        )
        .unwrap();
    assert!(mem
        .blueprint_call_revision("blueprints:workspace", "team", &recorded)
        .unwrap_err()
        .contains("corrupted"));
    assert!(mem
        .blueprint_call_drift("blueprints:workspace", "team", frozen.clone())
        .is_err());
    assert!(mem
        .pin_blueprint_calls("blueprints:workspace", "team", frozen)
        .is_err());
}
