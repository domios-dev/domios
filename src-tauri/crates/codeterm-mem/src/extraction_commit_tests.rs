//! Commit authorization proven at the storage boundary itself: every case here
//! drives the real `MemManager` write methods, and none of them waits on time.

use super::store::{with_write_worker, MemUpdate};
use super::*;
use parking_lot::Mutex;

#[derive(Default)]
struct CancelledWorkers {
    ids: Mutex<Vec<String>>,
}

impl ExtractionCancelSink for CancelledWorkers {
    fn cancel(&self, revoked: &[RevokedGeneration]) {
        let mut ids = self.ids.lock();
        for r in revoked {
            if let Some(w) = r.worker_id.clone() {
                ids.push(w);
            }
        }
    }
}

fn note(title: &str, body: &str) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Note,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: Some("test".into()),
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    }
}

fn derived(title: &str, source_id: &str) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: title.into(),
        summary: Some("derived".into()),
        area: Some("test".into()),
        tags: vec!["auto-extracted".into()],
        body: "a derived body with enough substance to be real".into(),
        source: Some(format!("extracted_from:{source_id}")),
        importance: 2.0,
        source_path: None,
        pinned: false,
    }
}

fn extracted_from(child: &str, source_id: &str) -> Relation {
    Relation {
        from_id: child.into(),
        relation_type: "extracted_from".into(),
        to_id: source_id.into(),
        scope: "global".into(),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
        reason: None,
    }
}

/// Open a generation and bind a worker to it, as the spawn site does once the
/// prompt has been copied out of the source row.
fn hold_generation(mem: &MemManager, source_id: &str, worker_id: &str) -> GenerationId {
    let uid = mem.get(source_id).unwrap().unwrap().uid;
    let generation = mem.extraction_authority().open(source_id, &uid);
    assert_eq!(
        mem.extraction_authority()
            .bind_worker(&generation, worker_id),
        BindOutcome::Active
    );
    generation
}

/// The late case. The generation is bound and authorized — its model execution
/// has already begun — and the delete lands after that. Every write it can
/// attempt is refused by commit authorization, and nothing it wrote lands.
#[test]
fn deleted_source_generation_cannot_commit_any_write() {
    let mem = MemManager::open_in_memory().unwrap();
    let source = mem
        .save(note("race source", "a body about Rust on Eight"))
        .unwrap();
    let generation = hold_generation(&mem, &source, "gw_race");

    // Authorized before the delete: the guard is not simply refusing everything.
    with_write_worker(Some("gw_race"), || {
        mem.save(derived("decision: pre delete", &source))
    })
    .expect("an authorized generation commits normally");

    assert!(mem.delete(&source).unwrap());
    assert!(mem.extraction_authority().is_revoked(&generation));

    let child = with_write_worker(Some("gw_race"), || {
        mem.save(derived("decision: post delete", &source))
    });
    assert!(child.is_err(), "derived item must be refused");
    assert!(child.unwrap_err().contains(&generation.token()));

    let relation = with_write_worker(Some("gw_race"), || {
        mem.graph_relate(&extracted_from("fact_post_delete", &source))
    });
    assert!(relation.is_err(), "extracted_from relation must be refused");

    let entity = with_write_worker(Some("gw_race"), || {
        mem.graph_create_entity(&Entity {
            id: "tech_rust".into(),
            entity_type: "Tech".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        })
    });
    assert!(entity.is_err(), "graph mutation must be refused");

    let update = with_write_worker(Some("gw_race"), || {
        mem.update(
            &source,
            MemUpdate {
                area: Some("hijacked".into()),
                ..Default::default()
            },
        )
    });
    assert!(update.is_err(), "update must be refused");
}

/// No residue: the cancelled generation leaves no derived item and no dangling
/// `extracted_from` edge behind.
#[test]
fn cancelled_generation_leaves_no_residue() {
    let mem = MemManager::open_in_memory().unwrap();
    let source = mem
        .save(note("residue source", "a body worth extracting"))
        .unwrap();
    let generation = hold_generation(&mem, &source, "gw_residue");

    // Committed before the delete, so the delete's cascade has to clean it up.
    with_write_worker(Some("gw_residue"), || {
        mem.save(derived("decision: early", &source))
    })
    .unwrap();
    with_write_worker(Some("gw_residue"), || {
        mem.graph_relate(&extracted_from("fact_decision_early", &source))
    })
    .unwrap();

    assert!(mem.delete(&source).unwrap());

    let _ = with_write_worker(Some("gw_residue"), || {
        mem.save(derived("decision: late", &source))
    });
    let _ = with_write_worker(Some("gw_residue"), || {
        mem.graph_relate(&extracted_from("fact_decision_late", &source))
    });

    assert!(
        mem.get("fact_decision_late").unwrap().is_none(),
        "no derived artifact from the cancelled generation survives"
    );
    let dangling = mem.graph_related(&source, None, None, true).unwrap();
    assert!(
        dangling.is_empty(),
        "no dangling extracted_from relation survives: {:?}",
        dangling
    );
    assert!(
        mem.extraction_authority().is_revoked(&generation),
        "the cancelled generation stays revoked"
    );
}

/// The sibling control. Deleting one source must not disturb an unrelated
/// extraction generation — a fix that sweeps every extraction worker fails here.
#[test]
fn unrelated_generation_keeps_committing_after_a_delete() {
    let mem = MemManager::open_in_memory().unwrap();
    let cancelled = Arc::new(CancelledWorkers::default());
    mem.extraction_authority()
        .set_cancel_sink(cancelled.clone());

    let doomed = mem.save(note("doomed source", "body one")).unwrap();
    let sibling = mem.save(note("sibling source", "body two")).unwrap();
    let doomed_generation = hold_generation(&mem, &doomed, "gw_doomed");
    let sibling_generation = hold_generation(&mem, &sibling, "gw_sibling");

    assert!(mem.delete(&doomed).unwrap());

    assert_eq!(
        cancelled.ids.lock().clone(),
        vec!["gw_doomed".to_string()],
        "only the deleted source's worker is cancelled"
    );
    assert!(mem.extraction_authority().is_revoked(&doomed_generation));
    assert!(!mem.extraction_authority().is_revoked(&sibling_generation));

    with_write_worker(Some("gw_sibling"), || {
        mem.save(derived("decision: sibling still works", &sibling))
    })
    .expect("the unrelated generation keeps its authority and completes");
    with_write_worker(Some("gw_sibling"), || {
        mem.graph_relate(&extracted_from(
            "fact_decision_sibling_still_works",
            &sibling,
        ))
    })
    .expect("the unrelated generation keeps writing relations");
}

/// Batch forget shares the single-delete lifecycle, and reports both numbers.
#[test]
fn area_forget_revokes_only_its_own_areas_generations() {
    let mem = MemManager::open_in_memory().unwrap();
    let cancelled = Arc::new(CancelledWorkers::default());
    mem.extraction_authority()
        .set_cancel_sink(cancelled.clone());

    let mut doomed = note("area source", "body in the doomed area");
    doomed.area = Some("doomed".into());
    let doomed_id = mem.save(doomed).unwrap();
    let keeper_id = mem
        .save(note("keeper source", "body in the test area"))
        .unwrap();

    let doomed_generation = hold_generation(&mem, &doomed_id, "gw_area");
    let keeper_generation = hold_generation(&mem, &keeper_id, "gw_keeper");

    let outcome = mem.delete_by_area("doomed").unwrap();

    assert_eq!(outcome.deleted, 1);
    assert_eq!(
        outcome.cancelled_extractions, 1,
        "cleanup is reported as cancelled generations, not as deleted rows"
    );
    assert_eq!(cancelled.ids.lock().clone(), vec!["gw_area".to_string()]);
    assert!(mem.extraction_authority().is_revoked(&doomed_generation));
    assert!(!mem.extraction_authority().is_revoked(&keeper_generation));

    assert!(with_write_worker(Some("gw_area"), || mem
        .save(derived("decision: area", &doomed_id)))
    .is_err());
    with_write_worker(Some("gw_keeper"), || {
        mem.save(derived("decision: keeper", &keeper_id))
    })
    .expect("a generation outside the forgotten area is untouched");
}

/// Replace destroys the row the in-flight generation copied from, so that
/// generation is superseded — the replace/delete race.
#[test]
fn replace_supersedes_the_generation_built_from_the_old_body() {
    let mem = MemManager::open_in_memory().unwrap();
    let cancelled = Arc::new(CancelledWorkers::default());
    mem.extraction_authority()
        .set_cancel_sink(cancelled.clone());

    let source = mem.save(note("replace race", "the original body")).unwrap();
    let stale = hold_generation(&mem, &source, "gw_stale");

    let replaced = mem
        .save_replace(note("replace race", "a completely rewritten body"))
        .unwrap();
    assert_eq!(replaced, source, "replace reuses the slug id");

    assert!(
        mem.extraction_authority().is_revoked(&stale),
        "the generation built from the old body loses authority"
    );
    assert_eq!(cancelled.ids.lock().clone(), vec!["gw_stale".to_string()]);
    assert!(with_write_worker(Some("gw_stale"), || mem
        .save(derived("decision: stale", &source)))
    .is_err());
}

/// Delete then recreate reuses the slug id. The old generation must stay refused
/// even though a live generation now exists for that same id.
#[test]
fn recreated_source_does_not_revive_the_old_generation_at_the_boundary() {
    let mem = MemManager::open_in_memory().unwrap();
    let source = mem.save(note("recreate race", "first body")).unwrap();
    let old = hold_generation(&mem, &source, "gw_old");
    assert!(mem.delete(&source).unwrap());

    let recreated = mem.save(note("recreate race", "second body")).unwrap();
    assert_eq!(recreated, source, "recreate reuses the slug id");
    let fresh = hold_generation(&mem, &recreated, "gw_fresh");
    assert_ne!(old, fresh);

    assert!(
        with_write_worker(Some("gw_old"), || mem
            .save(derived("decision: from stale run", &source)))
        .is_err(),
        "a live generation on the same id must not re-authorize the old one"
    );
    with_write_worker(Some("gw_fresh"), || {
        mem.save(derived("decision: from fresh run", &recreated))
    })
    .expect("the current generation commits");
}

/// Writes with no worker origin — real user edits — are never caught by the guard.
#[test]
fn user_writes_are_unaffected_by_a_revocation() {
    let mem = MemManager::open_in_memory().unwrap();
    let source = mem.save(note("bystander source", "a body")).unwrap();
    let generation = hold_generation(&mem, &source, "gw_bystander");
    assert!(mem.delete(&source).unwrap());
    assert!(mem.extraction_authority().is_revoked(&generation));

    mem.save(note("a plain user note", "written by the user"))
        .expect("a user write carries no generation and is unaffected");
    with_write_worker(Some("gw_someone_else"), || {
        mem.save(note(
            "another worker's note",
            "written by an unrelated worker",
        ))
    })
    .expect("an unrelated worker is unattributed, not refused");
}
