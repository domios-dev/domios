use super::*;
use crate::embedder::{
    provider::{Content, EmbeddingProvider, Modality},
    testing::{fixture, note, MockProvider, StepClock, DEADLINE},
    ActiveEmbedder, StampedVector,
};
use std::sync::{atomic::Ordering, mpsc};

fn pending(cache: &QueryCache, text: &str) -> (Key, Arc<Flight>, bool) {
    match cache.admit(text, Duration::from_secs(1)).unwrap() {
        Admission::Pending { key, flight, start } => (key, flight, start),
        Admission::Cached(_) => panic!("expected a new or joined query"),
    }
}

fn vector() -> StampedVector {
    StampedVector {
        identity: MockProvider::new("mock", "model", 8).identity(),
        vector: vec![0.5; 8],
    }
}

#[test]
fn identical_queries_share_one_flight_and_reuse_completed_semantics_without_another_budget() {
    let cache = QueryCache::default();
    let (key, first, start) = pending(&cache, "same");
    assert!(start);
    let (_, second, start) = pending(&cache, "same");
    assert!(!start);
    assert!(Arc::ptr_eq(&first, &second));
    assert!(first.wait(Duration::ZERO).is_none());
    cache.finish(key, &first, Ok(vector()));
    assert_eq!(second.wait(Duration::ZERO), Some(vector()));
    assert!(
        matches!(cache.admit("same", Duration::ZERO), Some(Admission::Cached(v)) if v == vector())
    );
}

#[test]
fn query_capacity_stays_bounded_until_native_work_finishes_and_failures_are_not_cached() {
    let cache = QueryCache::default();
    let flights: Vec<_> = (0..MAX_FLIGHTS)
        .map(|i| pending(&cache, &i.to_string()))
        .collect();
    assert!(cache.admit("overflow", Duration::from_secs(1)).is_none());
    assert!(!pending(&cache, "0").2);
    let (key, first, _) = &flights[0];
    assert!(first.wait(Duration::ZERO).is_none());
    assert!(cache.admit("overflow", Duration::from_secs(1)).is_none());
    cache.finish(*key, first, Err(EmbedError::Refused("fixture".into())));
    assert!(pending(&cache, "overflow").2);
}

#[test]
fn provider_replacement_discards_cached_and_late_old_vectors() {
    let cache = QueryCache::default();
    let (old, first, _) = pending(&cache, "same");
    cache.invalidate();
    let (new, second, start) = pending(&cache, "same");
    assert!(start);
    assert_ne!(old, new);
    cache.finish(old, &first, Ok(vector()));
    assert!(matches!(
        cache.admit("same", Duration::from_secs(1)),
        Some(Admission::Pending { start: false, .. })
    ));
    cache.finish(new, &second, Ok(vector()));
    cache.invalidate();
    assert!(pending(&cache, "same").2);
}

#[test]
fn repeated_semantic_searches_use_one_query_vector_and_preserve_result_identity() {
    let provider = MockProvider::new("mock", "search", 8);
    let fx = fixture(provider.clone(), StepClock::new());
    let id = fx
        .mem
        .save(note("orchard", "orchard harvest fruit"))
        .unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let before = provider.calls.load(Ordering::SeqCst);
    let ((hits, vectors), degraded) = crate::embedder::recall_priority::with_recall_status(|| {
        let hits = fx
            .mem
            .recall(&crate::SearchQuery {
                query: "orchard harvest".into(),
                ..Default::default()
            })
            .unwrap();
        let vectors = fx.mem.vector_recall("orchard harvest", 1).unwrap();
        (hits, vectors)
    });
    assert!(!degraded);
    assert_eq!(hits[0].item.id, id);
    assert_eq!(vectors[0].0, id);
    assert_eq!(provider.calls.load(Ordering::SeqCst) - before, 1);
}

struct GatedQuery {
    provider: Arc<MockProvider>,
    entered: mpsc::Sender<String>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl EmbeddingProvider for GatedQuery {
    fn id(&self) -> &str {
        self.provider.id()
    }
    fn model(&self) -> &str {
        self.provider.model()
    }
    fn dimension(&self) -> usize {
        self.provider.dimension()
    }
    fn max_tokens(&self) -> usize {
        self.provider.max_tokens()
    }
    fn modalities(&self) -> &[Modality] {
        self.provider.modalities()
    }
    fn remote(&self) -> bool {
        false
    }
    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.provider.embed_passage(content)
    }
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        let Content::Text(text) = content else {
            unreachable!()
        };
        self.entered.send(text.clone()).unwrap();
        self.release.lock().recv_timeout(DEADLINE).unwrap();
        self.provider.embed_query(content)
    }
}

#[test]
fn a_healthy_query_is_not_refused_just_because_another_query_is_in_flight() {
    let provider = MockProvider::new("mock", "concurrent", 8);
    let (entered, starts) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let embedder = ActiveEmbedder::new(Arc::new(GatedQuery {
        provider,
        entered,
        release: Mutex::new(gate),
    }));
    let first = {
        let e = embedder.clone();
        std::thread::spawn(move || e.query("first"))
    };
    assert_eq!(starts.recv_timeout(DEADLINE).unwrap(), "first");
    let second = {
        let e = embedder.clone();
        std::thread::spawn(move || e.query("second"))
    };
    assert_eq!(starts.recv_timeout(DEADLINE).unwrap(), "second");
    release.send(()).unwrap();
    release.send(()).unwrap();
    assert!(first.join().unwrap().is_some());
    assert!(second.join().unwrap().is_some());
    assert!(embedder.is_available());
}

#[test]
fn completed_vectors_are_bounded_and_retired_provider_failures_do_not_degrade_the_replacement() {
    let cache = QueryCache::default();
    for i in 0..MAX_CACHED + 1 {
        let (key, flight, _) = pending(&cache, &i.to_string());
        cache.finish(key, &flight, Ok(vector()));
    }
    assert_eq!(cache.0.lock().ready.len(), MAX_CACHED);
    assert!(pending(&cache, "0").2);
    let (key, flight, _) = pending(&cache, "large");
    let mut large = vector();
    large.vector = vec![0.5; MAX_CACHE_BYTES / size_of::<f32>()];
    cache.finish(key, &flight, Ok(large));
    assert_eq!(cache.0.lock().ready.len(), 1);

    let old: crate::embedder::provider::SharedProvider = MockProvider::new("old", "old", 8);
    let active = ActiveEmbedder::new(old.clone());
    active.replace(MockProvider::new("new", "new", 8));
    active.observe(
        &old,
        &Err::<Vec<f32>, _>(EmbedError::Unavailable("late old failure".into())),
    );
    assert!(active.is_available());
}
