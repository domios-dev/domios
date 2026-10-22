use super::*;
use crate::{MemKind, MemScope, NewMemItem};
use codeterm_blueprint_types::blueprint_memory::{curator, CuratorEvidence};

fn candidate() -> NewMemItem {
    NewMemItem {
        kind: MemKind::Skill,
        scope: MemScope::Global,
        title: "Restore database snapshots".into(),
        summary: None,
        area: Some("project".into()),
        tags: vec![],
        body: "Check snapshot integrity before restoring the database".into(),
        source: Some("curator-skills".into()),
        importance: 0.5,
        source_path: None,
        pinned: false,
    }
}

fn proof() -> CuratorEvidence {
    CuratorEvidence {
        plan: "plan-test".into(),
        round: 1,
        episodes: vec![],
        granted: true,
    }
}

#[test]
fn curator_skill_save_ignores_unrelated_populated_memory() {
    let mem = MemManager::open_in_memory().unwrap();
    for index in 0..12 {
        let mut fact = candidate();
        fact.kind = MemKind::Fact;
        fact.title = format!("Restore database snapshots fact {index}");
        fact.body = "Use an isolated server for experimentation".into();
        mem.insert_raw(&fact).unwrap();
    }
    let mut foreign = candidate();
    foreign.area = Some("other-project".into());
    foreign.body = "Different database restoration procedure".into();
    mem.insert_raw(&foreign).unwrap();
    let mut other_source = candidate();
    other_source.source = Some("blueprint-library".into());
    mem.insert_raw(&other_source).unwrap();
    let mut unrelated = candidate();
    unrelated.title = "Restore garden furniture".into();
    mem.insert_raw(&unrelated).unwrap();
    let item = candidate();
    let relations = mem.curator_skill_relations(&item).unwrap();
    assert!(relations
        .iter()
        .all(|relation| *relation == Relation::Unrelated));
    assert_eq!(
        curator::decide(&Default::default(), true, false, &relations).unwrap(),
        curator::Decision::Save
    );
    let id = mem.save_curator_skill(item, None, proof(), 1).unwrap();
    assert!(mem
        .get(&id)
        .unwrap()
        .unwrap()
        .body
        .contains("Check snapshot integrity"));
}

#[test]
fn curator_skill_refuses_same_intent_with_different_structure_and_covers_same_body() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save_curator_skill(candidate(), None, proof(), 1)
        .unwrap();
    let covers = mem.curator_skill_relations(&candidate()).unwrap();
    assert_eq!(
        curator::decide(&Default::default(), true, false, &covers).unwrap(),
        curator::Decision::Improve(id.clone())
    );
    let mut conflicting = candidate();
    conflicting.body =
        "Replace the running database immediately without checking the snapshot".into();
    let relations = mem.curator_skill_relations(&conflicting).unwrap();
    assert_eq!(
        curator::decide(&Default::default(), true, false, &relations),
        Err(format!("conflict with {id}"))
    );
    assert_eq!(
        mem.get(&id)
            .unwrap()
            .unwrap()
            .body
            .matches("Curator evidence:")
            .count(),
        1
    );
}

#[test]
fn conflict_requires_relevance_and_same_intent() {
    assert_eq!(relation("id".into(), 0.1, true), Relation::Unrelated);
    assert_eq!(relation("id".into(), 0.7, false), Relation::Unrelated);
    assert_eq!(
        relation("id".into(), 0.9, false),
        Relation::Conflict("id".into())
    );
    assert_eq!(
        relation("id".into(), 0.9, true),
        Relation::Covers("id".into())
    );
}
