use std::sync::atomic::Ordering;

use crate::embedder::local;
use crate::embedder::provider::EmbeddingProvider;
use crate::embedder::testing::{
    fixture_with_docs, note, wait_for, MockProvider, StepClock, DEADLINE,
};
use crate::embedder::{EmbedderEvent, ReembedState};
use crate::{Entity, SearchQuery};

#[test]
fn embedder_default_switch_reembeds_l12_items_entities_and_docs_once() {
    let baseline = &local::MODELS[0];
    assert_eq!(
        baseline.model,
        "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2"
    );
    let old = MockProvider::new(local::ID, baseline.model, baseline.dimension);
    let fixture = fixture_with_docs(old, StepClock::new(), true);
    let deploy = fixture
        .mem
        .save(note(
            "Canary deployment",
            "rolling deployment canary readiness checks",
        ))
        .unwrap();
    fixture
        .mem
        .save(note(
            "Offsite backup",
            "nightly backups copied to another location",
        ))
        .unwrap();
    fixture
        .mem
        .graph_create_entity(&Entity {
            id: "person:migration-owner".into(),
            entity_type: "person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Migration Owner"}),
        })
        .unwrap();
    assert!(fixture.mem.embed_queue.wait_idle(DEADLINE));
    let selected = local::default_model();
    let next = MockProvider::new(local::ID, selected.model, selected.dimension);
    let plan = fixture.mem.switch_embedder(next.clone()).unwrap();
    assert_eq!((plan.items, plan.entities), (2, 1));
    assert_eq!(plan.docs, crate::docs_source::vector_section_count() as u64);
    assert!(plan.docs > 0);
    let event = wait_for(&fixture.events, |event| {
        matches!(event,
        EmbedderEvent::Progress(progress) if progress.state == ReembedState::Completed)
    });
    let EmbedderEvent::Progress(progress) = event else {
        unreachable!()
    };
    assert_eq!((progress.done, progress.failed), (plan.total(), 0));
    assert_eq!(progress.plan, plan);
    assert_eq!(next.calls.load(Ordering::SeqCst) as u64, plan.total());
    let status = fixture.mem.embedder_status();
    assert!(status.index_matches);
    assert_eq!(status.index, Some(next.identity()));
    assert_eq!(
        fixture
            .mem
            .vector_recall("canary deployment readiness", 1)
            .unwrap()[0]
            .0,
        deploy
    );
    assert_eq!(
        fixture
            .mem
            .recall(&SearchQuery {
                query: "canary deployment readiness".into(),
                ..Default::default()
            })
            .unwrap()[0]
            .item
            .id,
        deploy
    );
    let report = fixture.mem.reembed_bulk(true, true).unwrap();
    assert_eq!((report.items, report.entities), (2, 1));
    assert!(report.errors.is_empty());
    println!(
        "SwitchPlan={plan:?}; ReembedProgress={progress:?}; ReembedReport={report:?}; search=ok"
    );
}
