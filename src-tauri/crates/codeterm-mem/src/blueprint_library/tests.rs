use super::*;
use serde_json::json;

fn document(bucket: &str) -> Document {
    Document::from_definition(&json!({"create":{"name":"ci-watch","note":"Watch CI failure","probe":"echo ready","question":"Ready?","buckets":[bucket],"every":"20s","matches":{(bucket):"ready"},"routes":{(bucket):"notify"},"until":bucket,"timeout":"1h"}})).unwrap()
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
        .save_blueprint("project", document("done"), "Watch CI failure", proof(1))
        .unwrap()
        .unwrap();
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
        .save_blueprint("project", document("done"), "Watch CI failure", proof(2))
        .unwrap()
        .unwrap();
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
        .save_blueprint("project", document("done"), "Watch CI failure", evidence())
        .unwrap()
        .unwrap();
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
        mem.save_blueprint("project", document("failed"), "Watch CI failure", proof),
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
            "Watch CI failure",
            proof.clone(),
        )
        .unwrap()
        .unwrap();
    mem.save_blueprint(
        "project",
        document("done"),
        "Watch CI failure",
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
        "Watch CI failure",
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
            "Watch CI failure",
            Evidence::default()
        )
        .is_err());
    let id = mem
        .save_blueprint(
            "project-a",
            document("done"),
            "Watch CI failure",
            evidence(),
        )
        .unwrap()
        .unwrap();
    let mut changed = document("done");
    changed.watch.as_mut().unwrap()["probe"] = json!("echo different-instance");
    let again = mem
        .save_blueprint("project-a", changed, "Watch CI failure", evidence())
        .unwrap()
        .unwrap();
    assert_eq!(id, again);
    let item = mem.get(&id).unwrap().unwrap();
    assert_eq!(item.kind, "skill");
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .blueprint
            .unwrap()
            .when_to_use,
        "Watch CI failure"
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
    assert_eq!(bound["create"]["probe"], "echo rebound");
    assert!(mem
        .save_blueprint(
            "project-a",
            document("done"),
            "token=do-not-store",
            evidence()
        )
        .is_err());
}

#[test]
fn blueprint_library_archives_lowest_score_and_preserves_yaml() {
    let mem = MemManager::open_in_memory().unwrap();
    let popular = mem
        .save_blueprint(
            "project",
            document("popular"),
            "Watch CI failure",
            evidence(),
        )
        .unwrap()
        .unwrap();
    mem.reuse_blueprints(std::slice::from_ref(&popular))
        .unwrap();
    for index in 0..50 {
        mem.save_blueprint(
            "project",
            document(&format!("bucket{index}")),
            "Watch CI failure",
            evidence(),
        )
        .unwrap();
    }
    let conn = mem.conn.lock();
    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_items WHERE kind='skill' AND source=?1",
            [SOURCE],
            |row| row.get(0),
        )
        .unwrap();
    let archived:i64=conn.query_row("SELECT COUNT(*) FROM mem_items WHERE kind='archived' AND (body_inline IS NOT NULL OR body_blob_path IS NOT NULL)",[],|row|row.get(0)).unwrap();
    assert_eq!(active, 50);
    assert_eq!(archived, 1);
    assert_eq!(
        store::get(&conn, &popular, &mem.blobs_dir)
            .unwrap()
            .unwrap()
            .kind,
        "skill"
    );
}

#[test]
fn blueprint_dreamer_only_merges_existing_solutions_and_preserves_stats() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint("project", document("done"), "Watch CI failure", evidence())
        .unwrap()
        .unwrap();
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
    metadata::write(&mem.conn.lock(), &other, &record).unwrap();
    assert_eq!(mem.dream_blueprints().unwrap(), 1);
    let kinds = [
        mem.get(&id).unwrap().unwrap().kind,
        mem.get(&other).unwrap().unwrap().kind,
    ];
    assert_eq!(kinds.iter().filter(|kind| *kind == "archived").count(), 1);
    assert_eq!(mem.recall_blueprints("project", "", 5).unwrap().len(), 1);
    assert_eq!(mem.dream_blueprints().unwrap(), 0);
}

#[test]
fn owner_list_does_not_count_recall_and_archive_is_project_scoped() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint("project-a", document("done"), "Watch CI", evidence())
        .unwrap()
        .unwrap();
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
    let definition = json!({"create":{"name":"ci-watch","probe":"echo ready","question":"Ready?","buckets":["done"],"every":"20s","routes":{"done":"notify"},"until":"done","timeout":"1h"}});
    assert!(mem
        .enqueue_blueprint(
            "project".into(),
            definition.clone(),
            "token=private-token".into(),
            evidence()
        )
        .unwrap_err()
        .contains("unsafe_template"));
    assert!(mem
        .enqueue_blueprint(
            "project".into(),
            definition,
            "Watch CI".into(),
            Evidence::default()
        )
        .unwrap_err()
        .contains("no_success"));
    assert!(mem.blueprint_queue.lock().is_none());
    assert!(mem.list_blueprint_solutions("project").unwrap().is_empty());
}

#[test]
fn memory_access_counters_do_not_change_blueprint_statistics() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint("project", document("done"), "Watch CI", evidence())
        .unwrap()
        .unwrap();
    mem.conn.lock().execute("UPDATE mem_items SET access_count=999,recurrence_count=888,title='Search title' WHERE id=?1",[&id]).unwrap();
    let entries = mem.list_blueprint_solutions("project").unwrap();
    assert_eq!(entries[0].uses, 0);
    assert_eq!(entries[0].successes, 1);
    assert_eq!(entries[0].when_to_use, "Watch CI");
    mem.reuse_blueprints(&[id.clone()]).unwrap();
    assert_eq!(mem.list_blueprint_solutions("project").unwrap()[0].uses, 1);
}

#[test]
fn kept_solution_round_trips_through_recall_and_memory_recipe() {
    let mem = std::sync::Arc::new(MemManager::open_in_memory().unwrap());
    for routed in [false, true] {
        let mut step = json!({"name":"Echo ready","input":"text","kind":{"kind":"command","command":"Write-Output ready"}});
        if routed {
            step["on"] = json!({"done":"stop","failed":"stop"});
        }
        let original = Document::from_definition(&json!({"spec":{
            "name":"echo-test", "note":"Print readiness and stop", "owner":"team",
            "trigger":{"type":"schedule","every":"1m"}, "steps":[step], "result":{"to":"notify"}
        }}))
        .unwrap();
        let expected = original.bind(&[]).unwrap();
        let area = if routed { "routed" } else { "unrouted" };
        let id = mem
            .save_blueprint(area, original, "echo-test readiness", evidence())
            .unwrap()
            .unwrap();
        let offered = mem.recall_blueprints(area, "echo-test", 3).unwrap();
        assert_eq!(offered[0].id, id);
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
            assert!(restored.pipeline.as_ref().unwrap()["steps"][0]
                .get("on")
                .is_none());
        }
    }
}

fn echo_document() -> Document {
    Document::from_definition(&json!({"spec":{
        "name":"echo-test", "note":"Print readiness", "owner":"team",
        "trigger":{"type":"schedule","every":"1m"},
        "steps":[{"name":"Echo","input":"text","kind":{"kind":"command","command":"Write-Output ready"}}],
        "result":{"to":"notify"}
    }})).unwrap()
}

fn corrupt_saved_echo(mem: &MemManager, id: &str) {
    let mut broken = model::abstract_document(echo_document());
    broken.pipeline.as_mut().unwrap()["steps"][0]["on"] = serde_json::Value::Null;
    broken.inputs.insert(
        "pipeline_steps_0_run".into(),
        broken.inputs["command"].clone(),
    );
    assert_eq!(
        model::structure(&broken),
        model::structure(&echo_document())
    );
    let conn = mem.conn.lock();
    store::update(
        &conn,
        id,
        &store::MemUpdate {
            body: Some(broken.yaml().unwrap()),
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
        .save_blueprint("project", echo_document(), "Old phrase", evidence())
        .unwrap()
        .unwrap();
    corrupt_saved_echo(&mem, &id);
    let again = mem
        .save_blueprint(
            "project",
            echo_document(),
            "Brand new echo phrase",
            evidence(),
        )
        .unwrap()
        .unwrap();
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
    assert_eq!(stats.when_to_use, "Brand new echo phrase");
}

#[test]
fn recall_archives_invalid_recipes_without_hiding_valid_ones() {
    let mem = std::sync::Arc::new(MemManager::open_in_memory().unwrap());
    let id = mem
        .save_blueprint("project", echo_document(), "Echo readiness", evidence())
        .unwrap()
        .unwrap();
    corrupt_saved_echo(&mem, &id);
    let valid = mem
        .save_blueprint(
            "project",
            document("done"),
            "Echo valid watcher",
            evidence(),
        )
        .unwrap()
        .unwrap();
    let recalled = mem.recall_blueprints("project", "Echo", 3).unwrap();
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].id, valid);
    assert_archived(&mem, &id);
    assert!(mem.blueprint_recipe("project", &id).is_err());
    let conn = mem.conn.lock();
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
        .conn
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
        "Watch CI failure",
        evidence(),
    )
    .unwrap();
    mem.save_blueprint(
        "project",
        document("unrelated"),
        "Watch garden soil moisture",
        evidence(),
    )
    .unwrap();
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            "Watch CI failure",
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
        .unwrap();
    assert_eq!(
        mem.get(&id).unwrap().unwrap().source.as_deref(),
        Some(SOURCE)
    );
    assert_eq!(mem.list_blueprint_solutions("project").unwrap().len(), 2);
}

#[cfg(feature = "embeddings")]
#[test]
fn blueprint_recall_uses_vectors_without_shared_words_and_scopes_before_limit() {
    let mem = MemManager::open_in_memory().unwrap();
    let intent = "Detect stalled deployments";
    let query = "Identify frozen releases";
    assert!(!intent.split_whitespace().any(|word| query.contains(word)));
    let id = mem
        .save_blueprint("project", document("done"), intent, evidence())
        .unwrap()
        .unwrap();
    let foreign = mem
        .save_blueprint("elsewhere", document("failed"), intent, evidence())
        .unwrap()
        .unwrap();
    let mut vector = vec![0.0f32; 384];
    vector[0] = 1.0;
    let conn = mem.conn.lock();
    crate::embeddings::init_extension(&conn).unwrap();
    let encoded = crate::embeddings::inner::encode_vector(&vector);
    for saved in [&id, &foreign] {
        conn.execute(
            "INSERT INTO mem_vectors(mem_id, embedding) VALUES (?1,?2)",
            params![format!("{saved}_chunk_1"), &encoded],
        )
        .unwrap();
    }
    for index in 0..300 {
        let fact = store::insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: format!("Unrelated record {index}"),
                summary: None,
                area: Some("project".into()),
                tags: vec![],
                body: "unrelated populated memory".into(),
                source: None,
                importance: 1.0,
                source_path: None,
                pinned: false,
            },
            &mem.blobs_dir,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mem_vectors(mem_id, embedding) VALUES (?1,?2)",
            params![fact, &encoded],
        )
        .unwrap();
    }
    drop(conn);
    assert!(mem
        .search(&crate::SearchQuery {
            query: query.into(),
            ..Default::default()
        })
        .unwrap()
        .is_empty());
    let hits = mem
        .recall_blueprints_with_embedding("project", query, 1, Some(&vector))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id);
    let all = mem
        .recall_blueprints_with_embedding("", query, 5, Some(&vector))
        .unwrap();
    assert_eq!(all.len(), 2);
}

#[cfg(feature = "embeddings")]
#[test]
fn blueprint_usage_breaks_equal_retrieval_scores() {
    let mem = MemManager::open_in_memory().unwrap();
    let first = mem
        .save_blueprint("project", document("done"), "Watch CI failure", evidence())
        .unwrap()
        .unwrap();
    let popular = mem
        .save_blueprint(
            "project",
            document("failed"),
            "Watch CI failure",
            evidence(),
        )
        .unwrap()
        .unwrap();
    mem.reuse_blueprints(std::slice::from_ref(&popular))
        .unwrap();
    let mut vector = vec![0.0f32; 384];
    vector[0] = 1.0;
    let conn = mem.conn.lock();
    crate::embeddings::init_extension(&conn).unwrap();
    for id in [first, popular.clone()] {
        conn.execute(
            "INSERT INTO mem_vectors(mem_id, embedding) VALUES (?1,?2)",
            params![id, crate::embeddings::inner::encode_vector(&vector)],
        )
        .unwrap();
    }
    drop(conn);
    let hits = mem
        .recall_blueprints_with_embedding("project", "Identify frozen releases", 1, Some(&vector))
        .unwrap();
    assert_eq!(hits[0].id, popular);
}

#[cfg(feature = "embeddings")]
#[test]
fn blueprint_recall_refuses_unrelated_vectors_even_when_the_only_solution_is_kept() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            "Detect stalled deployments",
            evidence(),
        )
        .unwrap()
        .unwrap();
    let mut saved = vec![0.0f32; 384];
    saved[0] = 1.0;
    let mut unrelated = vec![0.0f32; 384];
    unrelated[1] = 1.0;
    let conn = mem.conn.lock();
    crate::embeddings::init_extension(&conn).unwrap();
    conn.execute(
        "INSERT INTO mem_vectors(mem_id, embedding) VALUES (?1,?2)",
        params![
            format!("{id}_chunk_1"),
            crate::embeddings::inner::encode_vector(&saved)
        ],
    )
    .unwrap();
    drop(conn);
    for query in [
        "tell me when every worker in the team has finished",
        "rotate the log files each night and compress them",
    ] {
        assert!(mem
            .recall_blueprints_with_embedding("project", query, 3, Some(&unrelated))
            .unwrap()
            .is_empty());
    }
    assert_eq!(
        mem.recall_blueprints_with_embedding(
            "project",
            "Identify frozen releases",
            3,
            Some(&saved)
        )
        .unwrap()[0]
            .id,
        id
    );
}

#[test]
fn blueprint_recall_rejects_incidental_keyword_overlap() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.save_blueprint(
        "project",
        document("done"),
        "Detect stalled deployments",
        evidence(),
    )
    .unwrap();
    assert!(mem
        .recall_blueprints_with_embedding(
            "project",
            "rotate deployments log files each night and compress them",
            3,
            None
        )
        .unwrap()
        .is_empty());
}
