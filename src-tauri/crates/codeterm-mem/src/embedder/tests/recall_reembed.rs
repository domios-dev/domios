use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use crate::embedder::provider::EmbeddingProvider;
use crate::embedder::switch::ReembedTracker;
use crate::embedder::testing::{fixture, note, MockProvider, StepClock, DEADLINE};
use crate::embedder::{ActiveEmbedder, Clock, ReembedState, SwitchPlan};

struct ManualClock(AtomicI64);

impl Clock for ManualClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        StepClock::at(self.0.load(Ordering::SeqCst))
    }
}

#[test]
fn recall_reembed_retains_old_semantic_index_until_completion() {
    let old = MockProvider::new("old", "old", 384);
    let fx = fixture(old, StepClock::new());
    let id = fx
        .mem
        .save(note("Orchard", "apples orchard harvest fruit"))
        .unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let hits = fx
        .mem
        .vector_recall("apples orchard harvest fruit", 5)
        .unwrap();
    release.send(()).unwrap();
    assert_eq!(hits.first().map(|hit| hit.0.as_str()), Some(id.as_str()));
}

#[test]
fn recall_reembed_running_status_exposes_degradation() {
    let fx = fixture(MockProvider::new("old", "old", 384), StepClock::new());
    fx.mem
        .save(note("Orchard", "apples orchard harvest fruit"))
        .unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let status = serde_json::to_value(fx.mem.embedder_status()).unwrap();
    release.send(()).unwrap();
    assert_eq!(status["degraded"], true);
    assert_eq!(status["reembed"]["done"], 0);
    assert_eq!(status["reembed"]["total"], 1);
}

#[test]
fn recall_reembed_same_model_does_not_start_a_rebuild() {
    let provider = MockProvider::new("old", "old", 384);
    let fx = fixture(provider.clone(), StepClock::new());
    let plan = fx.mem.switch_embedder(provider).unwrap();
    assert_eq!(plan.total(), 0);
    assert_eq!(fx.mem.embedder_status().reembed.state, ReembedState::Idle);
}

#[test]
fn recall_reembed_hung_item_becomes_visible_after_bound() {
    let clock = Arc::new(ManualClock(AtomicI64::new(0)));
    let fx = fixture(MockProvider::new("old", "old", 384), clock.clone());
    fx.mem
        .save(note("Orchard", "apples orchard harvest fruit"))
        .unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    clock.0.store(3600, Ordering::SeqCst);
    let status = serde_json::to_value(fx.mem.embedder_status()).unwrap();
    release.send(()).unwrap();
    assert_ne!(status["reembed"]["state"], "running");
    assert!(status["reembed"]["last_error"].as_str().is_some());
}

#[test]
fn recall_reembed_dropped_driver_cannot_remain_running_forever() {
    let provider = MockProvider::new("old", "old", 384);
    let active = ActiveEmbedder::new(provider.clone());
    let clock = Arc::new(ManualClock(AtomicI64::new(0)));
    let tracker = Arc::new(ReembedTracker::new(clock.clone()));
    let worker_tracker = tracker.clone();
    std::thread::spawn(move || {
        worker_tracker.start(
            provider.identity(),
            SwitchPlan {
                items: 1,
                ..Default::default()
            },
            &active,
        );
    })
    .join()
    .unwrap();
    clock.0.store(3600, Ordering::SeqCst);
    let status = serde_json::to_value(tracker.snapshot()).unwrap();
    assert_ne!(status["state"], "running");
    assert!(status["last_error"].as_str().is_some());
}
