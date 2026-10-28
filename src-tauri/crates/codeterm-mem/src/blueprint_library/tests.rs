use super::*;
use crate::backend::sqlite::blueprint::{cleanup, metadata};
use crate::{store, MemKind, MemScope, NewMemItem};
use rusqlite::params;
mod changes;
use serde_json::json;
use std::collections::BTreeMap;

fn document(bucket: &str) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"watch","name":"ci-watch","note":"Watch CI failure","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"echo ready"}},"every":{"lit":"20s"},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":[bucket],"matches":{(bucket):{"lit":"ready"}},"until":bucket,"routes":{(bucket):"alert"}},{"id":"alert","uses":"notify"}]})).unwrap()
}
fn when(text: &str) -> Vec<String> {
    vec![text.into()]
}
fn evidence() -> Evidence {
    Evidence {
        kept: true,
        successes: 1,
        ..Evidence::default()
    }
}

#[test]
fn curator_save_preserves_uid_and_bumps_confidence_in_later_round() {
    let mem = MemManager::open_in_memory().unwrap();
    let proof = |round| Evidence {
        curator: Some(model::CuratorEvidence {
            plan: "pl_test".into(),
            round,
            episodes: vec!["blueprint:a".into(), "blueprint:b".into()],
            granted: false,
        }),
        ..evidence()
    };
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            proof(1),
        )
        .unwrap()
        .id;
    let first = mem.get(&id).unwrap().unwrap();
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .curator
            .unwrap()
            .confidence,
        crate::solution_metadata::Confidence::Low
    );
    assert!(first.tags.contains(&"curator".into()));
    assert!(mem
        .solution_metadata(&id)
        .unwrap()
        .unwrap()
        .curator
        .unwrap()
        .episodes
        .contains("blueprint:a"));
    let again = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            proof(2),
        )
        .unwrap()
        .id;
    assert_eq!(again, id);
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .curator
            .unwrap()
            .confidence,
        crate::solution_metadata::Confidence::Medium
    );
}

#[test]
fn curator_conflicting_graph_refuses_without_writing() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            evidence(),
        )
        .unwrap()
        .id;
    let proof = Evidence {
        curator: Some(model::CuratorEvidence {
            plan: "pl_test".into(),
            round: 1,
            episodes: vec![],
            granted: true,
        }),
        ..evidence()
    };
    assert_eq!(
        mem.save_blueprint(
            "project",
            document("failed"),
            &when("Watch CI failure"),
            proof
        ),
        Err(format!("conflict with {id}"))
    );
    assert_eq!(mem.list_blueprint_solutions("project").unwrap().len(), 1);
}

#[test]
fn repeated_keep_counts_new_evidence_only() {
    let mem = MemManager::open_in_memory().unwrap();
    let proof = Evidence {
        origin: Some("graph-version".into()),
        kept: true,
        successes: 3,
        ..Evidence::default()
    };
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            proof.clone(),
        )
        .unwrap()
        .id;
    mem.save_blueprint(
        "project",
        document("done"),
        &when("Watch CI failure"),
        proof.clone(),
    )
    .unwrap();
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .blueprint
            .unwrap()
            .successes,
        3
    );
    mem.save_blueprint(
        "project",
        document("done"),
        &when("Watch CI failure"),
        Evidence {
            successes: 4,
            ..proof
        },
    )
    .unwrap();
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .blueprint
            .unwrap()
            .successes,
        4
    );
}

#[test]
fn blueprint_library_persists_generalised_skills_and_dedupes_per_project() {
    let mem = MemManager::open_in_memory().unwrap();
    assert!(mem
        .save_blueprint(
            "project-a",
            document("done"),
            &when("Watch CI failure"),
            Evidence::default()
        )
        .is_err());
    let id = mem
        .save_blueprint(
            "project-a",
            document("done"),
            &when("Watch CI failure"),
            evidence(),
        )
        .unwrap()
        .id;
    let mut changed = document("done");
    changed.steps[0].params.insert(
        "command".into(),
        codeterm_blueprint_types::blueprint_document::ValueForm::literal("echo different-instance"),
    );
    let again = mem
        .save_blueprint("project-a", changed, &when("Watch CI failure"), evidence())
        .unwrap()
        .id;
    assert_eq!(id, again);
    let item = mem.get(&id).unwrap().unwrap();
    assert_eq!(item.kind, "skill");
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .blueprint
            .unwrap()
            .intents,
        vec!["Watch CI failure".to_owned()]
    );
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .blueprint
            .unwrap()
            .successes,
        2
    );
    assert_eq!(item.recurrence_count, 0);
    assert!(item.tags.iter().all(|tag| !tag.contains(':')));
    assert!(!item.body.contains("different-instance"));
    assert!(!item.body.contains("echo ready"));
    let yaml = model::parse_document(&item.body).unwrap();
    assert_eq!(
        yaml.inputs.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["command"]
    );
    assert_eq!(yaml.inputs["command"].default, None);
    assert!(mem.blueprint_recipe("project-b", &id).is_err());
    let bound = mem
        .blueprint_recipe("project-a", &id)
        .unwrap()
        .bind(&["command=echo rebound".into()])
        .unwrap();
    assert_eq!(bound["steps"][0]["with"]["command"]["lit"], "echo rebound");
    assert!(mem
        .save_blueprint(
            "project-a",
            document("done"),
            &when("token=do-not-store"),
            evidence()
        )
        .is_err());
}

#[test]
fn blueprint_library_save_protects_current_entry_and_reports_evicted_ids() {
    let mem = MemManager::open_in_memory().unwrap();
    let save = |bucket: &str, successes| {
        let prepared = mem
            .qualify_blueprint(
                "project",
                document(bucket),
                &when("Watch CI failure"),
                Evidence {
                    successes,
                    ..evidence()
                },
            )
            .unwrap();
        mem.persist_blueprint("project", prepared, true, 2).unwrap()
    };
    let first = save("first", 10);
    let popular = save("popular", 20);
    assert!(first.evicted_ids.is_empty());
    assert!(popular.evicted_ids.is_empty());
    let saved = save("new", 1);
    assert_eq!(saved.evicted_ids, vec![first.id.clone()]);
    assert_archived(&mem, &first.id);
    assert_eq!(mem.get(&saved.id).unwrap().unwrap().kind, "skill");
    assert_eq!(mem.get(&popular.id).unwrap().unwrap().kind, "skill");
    assert_eq!(mem.list_blueprint_solutions("project").unwrap().len(), 2);
    assert!(model::parse_document(&mem.get(&first.id).unwrap().unwrap().body).is_ok());
}

#[test]
fn blueprint_library_storage_and_both_recall_paths_retain_more_than_fifty() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(fakes::WordEmbedder::new(true));
    let mut ids = std::collections::BTreeSet::new();
    for index in 0..60 {
        let saved = mem
            .save_blueprint(
                "project",
                document(&format!("bucket{index}")),
                &when("Watch CI failure"),
                evidence(),
            )
            .unwrap();
        assert!(saved.evicted_ids.is_empty());
        ids.insert(saved.id);
    }
    assert_eq!(ids.len(), 60);
    assert_eq!(mem.list_blueprint_solutions("project").unwrap().len(), 60);
    for intent in ["", "Watch CI failure"] {
        let hits = mem.recall_blueprints("project", intent, 60).unwrap();
        assert_eq!(
            hits.iter()
                .map(|hit| hit.entry.id.clone())
                .collect::<std::collections::BTreeSet<_>>(),
            ids
        );
    }
}

#[test]
fn blueprint_dreamer_only_merges_existing_solutions_and_preserves_stats() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            evidence(),
        )
        .unwrap()
        .id;
    let existing = mem.get(&id).unwrap().unwrap();
    let other = mem
        .insert_raw(&NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "duplicate".into(),
            summary: existing.summary,
            area: Some("project".into()),
            tags: vec![],
            body: existing.body,
            source: Some(SOURCE.into()),
            importance: 0.5,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    let record = mem.solution_metadata(&id).unwrap().unwrap();
    metadata::write(&mem.fixture_conn().lock(), &other, &record).unwrap();
    let events = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let observed = events.clone();
    let weak = std::sync::Arc::downgrade(&mem);
    mem.set_blueprint_library_listener(std::sync::Arc::new(move |area, revision| {
        assert!(weak.upgrade().unwrap().fixture_conn().try_lock().is_some());
        observed.lock().push((area.to_owned(), revision));
    }));
    assert_eq!(mem.dream_blueprints().unwrap(), 1);
    assert_eq!(*events.lock(), vec![("project".into(), 2)]);
    let kinds = [
        mem.get(&id).unwrap().unwrap().kind,
        mem.get(&other).unwrap().unwrap().kind,
    ];
    assert_eq!(kinds.iter().filter(|kind| *kind == "archived").count(), 1);
    assert_eq!(mem.recall_blueprints("project", "", 5).unwrap().len(), 1);
    assert_eq!(mem.dream_blueprints().unwrap(), 0);
    assert_eq!(events.lock().len(), 1);
}

#[test]
fn owner_list_does_not_count_recall_and_archive_is_project_scoped() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint("project-a", document("done"), &when("Watch CI"), evidence())
        .unwrap()
        .id;
    let before = mem.list_blueprint_solutions("project-a").unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(
        mem.list_blueprint_solutions("project-a").unwrap()[0].uses,
        before[0].uses
    );
    assert!(mem.archive_blueprint_solution("project-b", &id).is_err());
    assert_eq!(mem.list_blueprint_solutions("project-a").unwrap().len(), 1);
    mem.archive_blueprint_solution("project-a", &id).unwrap();
    assert!(mem
        .list_blueprint_solutions("project-a")
        .unwrap()
        .is_empty());
}

#[test]
fn enqueue_refuses_qualification_before_starting_the_worker() {
    let mem = MemManager::open_in_memory().unwrap();
    let definition = serde_json::to_value(document("done")).unwrap();
    assert!(mem
        .enqueue_blueprint(
            "project".into(),
            definition.clone(),
            vec!["token=private-token".into()],
            evidence()
        )
        .unwrap_err()
        .contains("unsafe_template"));
    assert!(mem
        .enqueue_blueprint(
            "project".into(),
            definition,
            vec!["Watch CI".into()],
            Evidence::default()
        )
        .unwrap_err()
        .contains("no_success"));
    assert!(mem.blueprint.queue.lock().is_none());
    assert!(mem.list_blueprint_solutions("project").unwrap().is_empty());
}

#[test]
fn memory_access_counters_do_not_change_blueprint_statistics() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint("project", document("done"), &when("Watch CI"), evidence())
        .unwrap()
        .id;
    mem.fixture_conn().lock().execute("UPDATE mem_items SET access_count=999,recurrence_count=888,title='Search title' WHERE id=?1",[&id]).unwrap();
    let entries = mem.list_blueprint_solutions("project").unwrap();
    assert_eq!(entries[0].uses, 0);
    assert_eq!(entries[0].successes, 1);
    assert_eq!(entries[0].intents, vec!["Watch CI".to_owned()]);
    mem.reuse_blueprints(&[id.clone()]).unwrap();
    assert_eq!(mem.list_blueprint_solutions("project").unwrap()[0].uses, 1);
}

#[test]
fn kept_solution_round_trips_through_recall_and_memory_recipe() {
    let mem = std::sync::Arc::new(MemManager::open_in_memory().unwrap());
    for routed in [false, true] {
        let mut original = echo_document();
        original.note = Some("Print readiness and stop".into());
        if routed {
            original.steps[0].routes = BTreeMap::from([
                ("done".into(), "@stop".into()),
                ("failed".into(), "@stop".into()),
            ]);
        }
        let expected = original.bind(&[]).unwrap();
        let area = if routed { "routed" } else { "unrouted" };
        let id = mem
            .save_blueprint(area, original, &when("echo-test readiness"), evidence())
            .unwrap()
            .id;
        let offered = mem.recall_blueprints(area, "echo-test", 3).unwrap();
        assert_eq!(offered[0].entry.id, id);
        let recipe = mem.blueprint_recipe(area, &id).unwrap();
        assert_eq!(
            recipe.inputs.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["command"]
        );
        let exported = recipe.yaml().unwrap();
        let restored = model::parse_document(&exported).unwrap();
        let applied = restored
            .bind(&["command=Write-Output ready".into()])
            .unwrap();
        assert_eq!(applied, expected);
        assert!(
            codeterm_blueprint_types::blueprint_document::definition_errors(&applied).is_empty()
        );
        if !routed {
            assert!(restored.steps[0].routes.is_empty());
        }
    }
}

fn echo_document() -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"echo-test","note":"Print readiness","owner":"team","trigger":{"type":"schedule","every":{"lit":"1m"}},"steps":[{"id":"echo","run":{"lit":"Write-Output ready"}}],"result":{"to":"notify"}})).unwrap()
}

fn corrupt_saved_echo(mem: &MemManager, id: &str) {
    let mut broken = model::abstract_document(echo_document());
    broken.steps[0].run =
        Some(codeterm_blueprint_types::blueprint_document::ValueForm::input("undeclared"));
    assert_eq!(
        model::structure(&broken),
        model::structure(&echo_document())
    );
    let conn = mem.fixture_conn().lock();
    store::update(
        &conn,
        id,
        &store::MemUpdate {
            body: Some(serde_yaml::to_string(&broken).unwrap()),
            ..Default::default()
        },
        &mem.blobs_dir,
    )
    .unwrap();
    let mut record = metadata::read(&conn, id).unwrap().unwrap();
    record.blueprint.as_mut().unwrap().uses = 7;
    metadata::write(&conn, id, &record).unwrap();
}

#[test]
fn explicit_keep_replaces_broken_equivalent_recipe_preserving_id_and_statistics() {
    let mem = std::sync::Arc::new(MemManager::open_in_memory().unwrap());
    let id = mem
        .save_blueprint("project", echo_document(), &when("Old phrase"), evidence())
        .unwrap()
        .id;
    corrupt_saved_echo(&mem, &id);
    let again = mem
        .save_blueprint(
            "project",
            echo_document(),
            &when("Brand new echo phrase"),
            evidence(),
        )
        .unwrap()
        .id;
    assert_eq!(again, id);
    let recipe = mem.blueprint_recipe("project", &id).unwrap();
    assert_eq!(recipe.inputs.len(), 1);
    assert!(recipe.inputs.contains_key("command"));
    assert_eq!(
        recipe.bind(&["command=Write-Output ready".into()]).unwrap(),
        echo_document().bind(&[]).unwrap()
    );
    let stats = mem
        .solution_metadata(&id)
        .unwrap()
        .unwrap()
        .blueprint
        .unwrap();
    assert_eq!(stats.successes, 2);
    assert_eq!(stats.uses, 7);
    assert_eq!(stats.intents, vec!["Brand new echo phrase".to_owned()]);
}

#[test]
fn recall_archives_invalid_recipes_without_hiding_valid_ones() {
    let mem = std::sync::Arc::new(MemManager::open_in_memory().unwrap());
    let id = mem
        .save_blueprint(
            "project",
            echo_document(),
            &when("Echo readiness"),
            evidence(),
        )
        .unwrap()
        .id;
    corrupt_saved_echo(&mem, &id);
    let valid = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Echo valid watcher"),
            evidence(),
        )
        .unwrap()
        .id;
    let recalled = mem.recall_blueprints("project", "Echo", 3).unwrap();
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entry.id, valid);
    assert_archived(&mem, &id);
    assert!(mem.blueprint_recipe("project", &id).is_err());
    let conn = mem.fixture_conn().lock();
    store::update(
        &conn,
        &valid,
        &store::MemUpdate {
            body: Some("not: [valid YAML".into()),
            ..Default::default()
        },
        &mem.blobs_dir,
    )
    .unwrap();
    drop(conn);
    assert!(mem
        .recall_blueprints("project", "Echo", 3)
        .unwrap()
        .is_empty());
    assert_archived(&mem, &valid);
}

fn assert_archived(mem: &MemManager, id: &str) {
    let kind: String = mem
        .fixture_conn()
        .lock()
        .query_row("SELECT kind FROM mem_items WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(kind, "archived");
}

#[test]
fn curator_saves_blueprint_among_unrelated_facts_and_other_scopes() {
    let mem = MemManager::open_in_memory().unwrap();
    for index in 0..12 {
        mem.insert_raw(&NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: format!("Watch CI failure unrelated fact {index}"),
            summary: None,
            area: Some("project".into()),
            tags: vec![],
            body: "CI infrastructure uses a remote build server".into(),
            source: None,
            importance: 1.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    }
    mem.save_blueprint(
        "other-project",
        document("failed"),
        &when("Watch CI failure"),
        evidence(),
    )
    .unwrap();
    mem.save_blueprint(
        "project",
        document("unrelated"),
        &when("Watch garden soil moisture"),
        evidence(),
    )
    .unwrap();
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            Evidence {
                curator: Some(model::CuratorEvidence {
                    plan: "pl_test".into(),
                    round: 1,
                    episodes: vec![],
                    granted: true,
                }),
                ..evidence()
            },
        )
        .unwrap()
        .id;
    assert_eq!(
        mem.get(&id).unwrap().unwrap().source.as_deref(),
        Some(SOURCE)
    );
    assert_eq!(mem.list_blueprint_solutions("project").unwrap().len(), 2);
}

mod recall_regressions;

#[test]
fn blueprint_v2_check_v1_cleanup_keeps_history_and_runs_once() {
    let mem = MemManager::open_in_memory().unwrap();
    let conn = mem.fixture_conn().lock();
    let obsolete = "schema_version: 1\nname: obsolete\nwatch: {}\n";
    let id = store::insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "Obsolete Blueprint".into(),
            summary: None,
            area: Some("project".into()),
            tags: vec![],
            body: obsolete.into(),
            source: Some(SOURCE.into()),
            importance: 0.5,
            source_path: None,
            pinned: false,
        },
        &mem.blobs_dir,
    )
    .unwrap();
    assert_eq!(cleanup::remove_v1(&conn, &mem.blobs_dir).unwrap(), 1);
    let history: String = conn
        .query_row(
            "SELECT old_body FROM mem_item_history WHERE item_id=?1 ORDER BY id DESC LIMIT 1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(history, obsolete);
    assert_eq!(cleanup::remove_v1(&conn, &mem.blobs_dir).unwrap(), 0);
    assert_eq!(
        conn.query_row(
            "SELECT removed_count FROM blueprint_v1_cleanup_receipt",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

fn code_pipeline(name: &str, body: &str, text: &str) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":name,"owner":"tab","trigger":{"type":"input","kind":{"lit":"text"},"source":{"lit":{"text":text}}},"steps":[{"id":"run","code":{"lang":"python","src":body},"in":{"type":"integer"},"out":{"type":"object"}}],"result":{"from":"run.out"}})).unwrap()
}

#[test]
fn admission_twins_respect_code_bodies_and_literals() {
    use dedupe::{compare_documents, Verdict};
    let probe = code_pipeline(
        "qa-auth",
        "def main(value):\n    import subprocess\n    return {'rc': 0}",
        "7",
    );
    let minimal = code_pipeline("qa-min-d", "def main(value): return {'t': value}", "7");
    let renamed = code_pipeline("qa-min-copy", "def main(value): return {'t': value}", "7");
    let other_text = code_pipeline("qa-min-e", "def main(value): return {'t': value}", "8");
    assert_eq!(
        compare_documents(&probe, &minimal).verdict,
        Verdict::Distinct
    );
    assert_eq!(
        compare_documents(&minimal, &other_text).verdict,
        Verdict::Distinct
    );
    assert_eq!(
        compare_documents(&minimal, &renamed).verdict,
        Verdict::Exact
    );
}

#[test]
fn blueprint_catalogue_tags_survive_save_merge_and_reuse() {
    let mem = MemManager::open_in_memory().unwrap();
    let mut proof = evidence();
    proof.tags = vec!["ci".into(), "monitoring".into()];
    let saved = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            proof,
        )
        .unwrap();
    assert_eq!(
        mem.blueprint_tags(&saved.id).unwrap(),
        vec!["ci", "monitoring"]
    );
    mem.reuse_blueprints(&[saved.id.clone()]).unwrap();
    assert_eq!(
        mem.blueprint_tags(&saved.id).unwrap(),
        vec!["ci", "monitoring"]
    );
    let mut proof = evidence();
    proof.tags = vec!["github".into()];
    let again = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Watch CI failure"),
            proof,
        )
        .unwrap();
    assert_eq!(again.id, saved.id);
    assert_eq!(mem.blueprint_tags(&saved.id).unwrap(), vec!["github"]);
    assert!(mem
        .get(&saved.id)
        .unwrap()
        .unwrap()
        .tags
        .contains(&"blueprint-library".into()));
    for entry in catalog::ENTRIES {
        let tags = mem.blueprint_tags(entry.id).unwrap();
        codeterm_blueprint_types::blueprint_tags::validate(&tags).unwrap();
    }
}
