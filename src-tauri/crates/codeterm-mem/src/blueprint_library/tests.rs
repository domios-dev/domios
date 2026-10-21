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
    assert_eq!(mem.get(&id).unwrap().unwrap().recurrence_count, 3);
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
    assert_eq!(mem.get(&id).unwrap().unwrap().recurrence_count, 4);
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
    assert_eq!(item.title, "Watch CI failure");
    assert_eq!(item.recurrence_count, 2);
    assert!(!item.body.contains("different-instance"));
    assert!(!item.body.contains("echo ready"));
    let yaml = model::parse_document(&item.body).unwrap();
    assert_eq!(yaml.inputs["watch_probe"].default, None);
    assert!(mem.blueprint_recipe("project-b", &id).is_err());
    let bound = mem
        .blueprint_recipe("project-a", &id)
        .unwrap()
        .bind(&["watch_probe=echo rebound".into()])
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
    assert_eq!(mem.dream_blueprints().unwrap(), 1);
    let kinds = [
        mem.get(&id).unwrap().unwrap().kind,
        mem.get(&other).unwrap().unwrap().kind,
    ];
    assert_eq!(kinds.iter().filter(|kind| *kind == "archived").count(), 1);
    assert_eq!(mem.recall_blueprints("project", "", 5).unwrap().len(), 1);
    assert_eq!(mem.dream_blueprints().unwrap(), 0);
}
