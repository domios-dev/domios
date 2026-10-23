use crate::embedder::index;
use crate::embedder::provider::EmbeddingProvider;
use crate::embedder::testing::{
    fixture, note, vector_rows, wait_for, MockProvider, StepClock, DEADLINE,
};
use crate::embedder::{EmbedderEvent, ReembedState, StampedVector};
use crate::SearchQuery;

#[test]
fn recall_reembed_search_keeps_old_vectors_until_atomic_swap() {
    let old = MockProvider::new("mock-old", "old-8", 8);
    let fx = fixture(old.clone(), StepClock::new());
    let bodies = [
        ("Alpha", "alpha apples orchard harvest"),
        ("Bravo", "bravo bananas plantation export"),
        ("Charlie", "charlie cherries market stall"),
    ];
    let ids = bodies
        .iter()
        .map(|(title, body)| fx.mem.save(note(title, body)).unwrap())
        .collect::<Vec<_>>();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let rows = vector_rows(&fx.mem);
    let new = MockProvider::new("mock-new", "new-16", 16);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    for done in 0..3 {
        started.recv_timeout(DEADLINE).unwrap();
        let status = fx.mem.embedder_status();
        assert_eq!(status.reembed.state, ReembedState::Running);
        assert_eq!(status.reembed.done, done);
        assert!(status.degraded);
        assert_eq!(status.index, Some(old.identity()));
        assert_eq!(vector_rows(&fx.mem), rows);
        let hits = fx
            .mem
            .recall(&SearchQuery {
                query: "bravo bananas".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits[0].item.id, ids[1]);
        let wrong = StampedVector {
            identity: new.identity(),
            vector: new.vector("alpha"),
        };
        assert!(fx
            .mem
            .with_conn(|conn| index::nearest(conn, &wrong, 3))
            .unwrap_err()
            .contains("mixed-model"));
        release.send(()).unwrap();
    }
    wait_for(
        &fx.events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if progress.state == ReembedState::Completed),
    );
    assert_eq!(fx.mem.embedder_status().index, Some(new.identity()));
    assert!(!fx.mem.embedder_status().degraded);
    let stale = StampedVector {
        identity: old.identity(),
        vector: old.vector("alpha"),
    };
    assert!(fx
        .mem
        .with_conn(|conn| index::nearest(conn, &stale, 3))
        .is_err());
}
