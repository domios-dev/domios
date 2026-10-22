use crate::embedder::index;
use crate::embedder::provider::EmbeddingProvider;
use crate::embedder::testing::{
    fixture, note, vector_rows, wait_for, MockProvider, StepClock, DEADLINE,
};
use crate::embedder::{EmbedderEvent, ReembedProgress, ReembedState, StampedVector};
use crate::SearchQuery;

fn progress(event: EmbedderEvent) -> Option<ReembedProgress> {
    match event {
        EmbedderEvent::Progress(progress) => Some(progress),
        _ => None,
    }
}

fn next_progress(fx: &crate::embedder::testing::Fixture) -> ReembedProgress {
    progress(wait_for(&fx.events, |event| {
        matches!(event, EmbedderEvent::Progress(_))
    }))
    .unwrap()
}

fn fts_ids(mem: &crate::MemManager, query: &str) -> Vec<String> {
    let q = SearchQuery {
        query: query.into(),
        ..SearchQuery::default()
    };
    mem.search(&q)
        .unwrap()
        .into_iter()
        .map(|hit| hit.item.id)
        .collect()
}

#[test]
fn search_stays_usable_and_sees_only_new_model_vectors_while_the_reembed_runs() {
    let old = MockProvider::new("mock-old", "old-8", 8);
    let fx = fixture(old.clone(), StepClock::new());
    let bodies = [
        ("Alpha", "alpha apples orchard harvest"),
        ("Bravo", "bravo bananas plantation export"),
        ("Charlie", "charlie cherries market stall"),
    ];
    let ids: Vec<String> = bodies
        .iter()
        .map(|(title, body)| fx.mem.save(note(title, body)).unwrap())
        .collect();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    assert_eq!(vector_rows(&fx.mem).len(), 3);

    let new = MockProvider::new("mock-new", "new-16", 16);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    let running = next_progress(&fx);
    assert_eq!(running.state, ReembedState::Running);
    assert_eq!((running.total, running.done), (3, 0));
    assert_eq!(running.started_at, Some(StepClock::at(0)));

    let first_text = started.recv_timeout(DEADLINE).unwrap();
    assert!(vector_rows(&fx.mem).is_empty(), "old vectors never served");
    for (index, (_, body)) in bodies.iter().enumerate() {
        let word = body.split_whitespace().next().unwrap();
        assert_eq!(fts_ids(&fx.mem, word), vec![ids[index].clone()]);
    }
    let hybrid = fx
        .mem
        .recall(&SearchQuery {
            query: "bravo bananas".into(),
            ..SearchQuery::default()
        })
        .unwrap();
    assert_eq!(hybrid[0].item.id, ids[1]);

    release.send(()).unwrap();
    let one = next_progress(&fx);
    assert_eq!((one.done, one.state), (1, ReembedState::Running));
    let landed = vector_rows(&fx.mem);
    assert_eq!(landed.len(), 1);
    let first_index = bodies
        .iter()
        .position(|(title, _)| first_text.contains(&title.to_lowercase()))
        .unwrap();
    assert_eq!(landed[0], ids[first_index]);
    let vector_hits = fx.mem.vector_recall(bodies[first_index].1, 3).unwrap();
    assert_eq!(
        vector_hits
            .iter()
            .map(|hit| hit.0.clone())
            .collect::<Vec<_>>(),
        vec![ids[first_index].clone()]
    );
    let stale = StampedVector {
        identity: old.identity(),
        vector: old.vector("alpha"),
    };
    assert!(fx
        .mem
        .with_conn(|conn| index::nearest(conn, &stale, 3))
        .is_err());

    started.recv_timeout(DEADLINE).unwrap();
    release.send(()).unwrap();
    assert_eq!(next_progress(&fx).done, 2);
    started.recv_timeout(DEADLINE).unwrap();
    release.send(()).unwrap();
    assert_eq!(next_progress(&fx).done, 3);
    let completed = next_progress(&fx);
    assert_eq!(completed.state, ReembedState::Completed);
    assert_eq!(completed.started_at, Some(StepClock::at(0)));
    assert_eq!(completed.finished_at, Some(StepClock::at(4)));
    assert_eq!(fx.mem.embedder_status().reembed, completed);
    assert_eq!(vector_rows(&fx.mem).len(), 3);
    assert!(fx.events.try_recv().is_err());
}
