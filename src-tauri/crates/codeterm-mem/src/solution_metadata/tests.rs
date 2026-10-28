use super::*;
use crate::backend::sqlite::blueprint::metadata::initialize;
use crate::{MemKind, MemScope};
use rusqlite::params;

fn item() -> NewMemItem {
    NewMemItem {
        kind: MemKind::Skill,
        scope: MemScope::Global,
        title: "Watch CI".into(),
        summary: None,
        area: Some("project".into()),
        tags: vec![],
        body: "Use the repository input to observe CI.".into(),
        source: None,
        importance: 0.5,
        source_path: None,
        pinned: false,
    }
}

fn proof(round: u64) -> CuratorEvidence {
    CuratorEvidence {
        plan: "pl_test".into(),
        round,
        episodes: vec!["task:a".into(), "task:b".into()],
        granted: false,
    }
}

#[test]
fn old_solution_shapes_are_deleted_without_conversion() {
    let mem = MemManager::open_in_memory().unwrap();
    let mut old = item();
    old.source = Some("blueprint-library".into());
    old.tags = vec!["failures:3".into(), "explicitly-kept".into()];
    let id = mem.insert_raw(&old).unwrap();
    initialize(&mem.conn.lock()).unwrap();
    assert!(mem.get(&id).unwrap().is_none());
    assert!(mem.solution_metadata(&id).unwrap().is_none());
}

#[test]
fn malformed_metadata_fails_instead_of_defaulting() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem.insert_raw(&item()).unwrap();
    mem.conn
        .lock()
        .execute(
            "INSERT INTO solution_metadata(item_id,metadata_json) VALUES (?1,?2)",
            params![
                id,
                r#"{"blueprint":{"successes":"bad"},"curator":null,"evidence":{}}"#
            ],
        )
        .unwrap();
    assert!(mem
        .solution_metadata(&id)
        .unwrap_err()
        .contains("invalid solution metadata"));
}

#[test]
fn curator_skill_metadata_and_item_commit_together_and_preserve_uid() {
    let mem = MemManager::open_in_memory().unwrap();
    let body = item().body;
    let id = mem.save_curator_skill(item(), None, proof(1), 100).unwrap();
    let uid = mem.get(&id).unwrap().unwrap().uid;
    assert_eq!(
        mem.solution_metadata(&id)
            .unwrap()
            .unwrap()
            .curator
            .unwrap()
            .confidence,
        Confidence::Low
    );
    assert!(mem.curator_skill_covers(&id, &body).unwrap());
    assert_eq!(
        mem.save_curator_skill(item(), Some(&id), proof(2), 200)
            .unwrap(),
        id
    );
    let saved = mem.get(&id).unwrap().unwrap();
    assert_eq!(saved.uid, uid);
    assert!(saved.body.contains("task:a"));
    assert!(saved.tags.iter().all(|tag| !tag.contains(':')));
    let metadata = mem
        .solution_metadata(&id)
        .unwrap()
        .unwrap()
        .curator
        .unwrap();
    assert_eq!(metadata.confidence, Confidence::Medium);
    assert_eq!(metadata.observed_at, 200);
    assert_eq!(
        metadata.episodes,
        BTreeSet::from(["task:a".into(), "task:b".into()])
    );
}
