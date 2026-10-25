use std::sync::Arc;

use crate::embedder::index;
use crate::embedder::local;
use crate::embedder::provider::EmbeddingProvider;
use crate::embedder::testing::{
    connection, fixture_with_docs, note, vector_rows, wait_for, MockProvider, StepClock, DEADLINE,
};
use crate::embedder::{EmbedderEvent, ReembedState, StampedVector};
use crate::{Entity, SearchQuery};

fn l12_mock() -> Arc<MockProvider> {
    let l12 = local::default_model();
    MockProvider::new(local::ID, l12.model, l12.dimension)
}

fn blob_len(mem: &crate::MemManager, id: &str) -> usize {
    mem.with_conn(|conn| {
        conn.query_row(
            "SELECT length(embedding) FROM mem_vectors WHERE mem_id = ?1",
            [id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap() as usize
    })
}

#[test]
fn switch_recreates_the_index_at_the_new_dimension_and_reembeds_items_entities_and_docs() {
    let old = l12_mock();
    let fx = fixture_with_docs(old.clone(), StepClock::new(), true);
    let mut ids = Vec::new();
    for (title, body) in [
        ("Deploy notes", "rolling deploy with canary checks enabled"),
        (
            "Backup plan",
            "nightly backups are copied offsite every day",
        ),
        ("Archive tip", "compress the logs before archiving them"),
    ] {
        ids.push(fx.mem.save(note(title, body)).unwrap());
    }
    for name in ["alice", "bob"] {
        fx.mem
            .graph_create_entity(&Entity {
                id: format!("person:{name}"),
                entity_type: "person".into(),
                scope: "global".into(),
                properties: serde_json::json!({ "name": name }),
            })
            .unwrap();
        ids.push(format!("person:{name}"));
    }
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let seeded = vector_rows(&fx.mem);
    for id in &ids {
        assert!(seeded.contains(id), "seeded L12 vector for {id}");
        assert_eq!(blob_len(&fx.mem, id), 384 * 4);
    }
    let l12_identity = old.identity();
    assert_eq!(fx.mem.embedder_status().index, Some(l12_identity.clone()));

    let plan = fx.mem.reembed_plan().unwrap();
    assert_eq!((plan.items, plan.entities), (3, 2));
    assert_eq!(plan.docs, 3);
    assert!(plan.docs > 0);

    let wide = MockProvider::new("mock-remote", "wide-1536", 1536);
    let started = fx.mem.switch_embedder(wide.clone()).unwrap();
    assert_eq!(started, plan);
    let done = wait_for(
        &fx.events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if progress.state == ReembedState::Completed),
    );
    let EmbedderEvent::Progress(progress) = done else {
        unreachable!()
    };
    assert_eq!(progress.total, plan.total());
    assert_eq!((progress.done, progress.failed), (plan.total(), 0));

    let status = fx.mem.embedder_status();
    assert_eq!(status.index, Some(wide.identity()));
    assert!(status.index_matches);
    assert_eq!(status.dimension, 1536);
    let rows = vector_rows(&fx.mem);
    for id in &ids {
        assert!(rows.contains(id), "re-embedded {id}");
        assert_eq!(blob_len(&fx.mem, id), 1536 * 4);
    }

    let stale = StampedVector {
        identity: l12_identity,
        vector: old.vector("rolling deploy"),
    };
    let refused = fx
        .mem
        .with_conn(|conn| index::insert_if_live(conn, &ids[0], 0, &stale));
    assert!(refused.unwrap_err().contains("mixed-model vectors refused"));
    let refused = fx.mem.with_conn(|conn| index::nearest(conn, &stale, 3));
    assert!(refused.unwrap_err().contains("mixed-model vectors refused"));
    assert_eq!(vector_rows(&fx.mem), rows);

    let fresh = fx.mem.vector_recall("rolling deploy canary", 1).unwrap();
    assert_eq!(fresh[0].0, ids[0]);
    let hits = fx
        .mem
        .recall(&SearchQuery {
            query: "backups offsite".into(),
            ..SearchQuery::default()
        })
        .unwrap();
    assert_eq!(hits[0].item.id, ids[1]);
}

#[test]
fn reembed_plan_counts_real_docs_without_embedding() {
    let provider = l12_mock();
    let dir = tempfile::tempdir().unwrap();
    let mem = crate::MemManager::open_with(
        dir.path(),
        crate::embedder::ActiveEmbedder::new(provider.clone()),
        StepClock::new(),
        Some(dir.path().to_path_buf()),
    )
    .unwrap();
    let plan = mem.reembed_plan().unwrap();
    assert_eq!(
        plan.docs,
        crate::docs_source::corpus_sections().len() as u64
    );
    assert!(plan.docs > 3);
    assert_eq!((plan.items, plan.entities, plan.blueprints), (0, 0, 0));
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn recall_reembed_adopts_unstamped_l12_without_relabelling() {
    let conn = connection();
    let legacy = crate::embedder::legacy_identity();
    index::recreate(&conn, &legacy).unwrap();
    conn.execute("DELETE FROM mem_embedder", []).unwrap();
    let other = MockProvider::new("mock-remote", "wide", 16).identity();
    assert_eq!(index::ensure(&conn, &other, &legacy).unwrap(), legacy);
    assert_eq!(index::stamp(&conn).unwrap(), Some(legacy.clone()));
    assert!(index::check(&conn, &other).is_err());
    assert!(index::check(&conn, &legacy).is_ok());
}
