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
    let id = fx
        .mem
        .save(note("Orchard", "apples orchard harvest fruit"))
        .unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let calls = provider.calls.load(Ordering::SeqCst);
    let plan = fx.mem.switch_embedder(provider.clone()).unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        fx.mem
            .vector_recall("apples orchard harvest fruit", 5)
            .unwrap()[0]
            .0,
        id
    );
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

#[test]
fn recall_reembed_dropped_provider_records_error_without_losing_old_index() {
    use crate::embedder::testing::wait_for;
    use crate::embedder::EmbedderEvent;
    let fx = fixture(MockProvider::new("old", "old", 384), StepClock::new());
    let id = fx
        .mem
        .save(note("Orchard", "apples orchard harvest fruit"))
        .unwrap();
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    drop(release);
    let event = wait_for(
        &fx.events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if progress.state == ReembedState::Failed),
    );
    let EmbedderEvent::Progress(progress) = event else {
        unreachable!()
    };
    assert!(progress.last_error.unwrap().contains("gate closed"));
    assert_eq!(
        fx.mem
            .vector_recall("apples orchard harvest fruit", 5)
            .unwrap()[0]
            .0,
        id
    );
}

#[test]
fn recall_reembed_1k_items_need_one_provider_and_no_enrichment() {
    use crate::embedder::testing::wait_for;
    use crate::embedder::EmbedderEvent;
    let old = MockProvider::new("old", "old", 384);
    let mem = crate::MemManager::open_in_memory().unwrap();
    mem.embedder.replace(old.clone());
    crate::embedder::index::recreate(&mem.conn.lock(), &old.identity()).unwrap();
    crate::blueprint_library::initialize(&mem.conn.lock()).unwrap();
    let (sender, events) = std::sync::mpsc::channel();
    mem.embedder.subscribe(Arc::new(move |event| {
        let _ = sender.send(event.clone());
    }));
    for item in 0..1000 {
        crate::store::insert(
            &mem.conn.lock(),
            &note(
                &format!("Fixture {item}"),
                "an ordinary memory body for throughput proof",
            ),
            &mem.blobs_dir,
        )
        .unwrap();
    }
    let new = MockProvider::new("new", "new", 768);
    let enricher = Arc::new(crate::blueprint_library::fakes::RecordingEnricher::default());
    mem.set_blueprint_enricher(Some(enricher.clone()));
    mem.switch_embedder(new.clone()).unwrap();
    let event = wait_for(
        &events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if matches!(progress.state, ReembedState::Completed | ReembedState::Failed | ReembedState::Stalled)),
    );
    let EmbedderEvent::Progress(progress) = event else {
        unreachable!()
    };
    assert_eq!(
        progress.state,
        ReembedState::Completed,
        "{:?}",
        progress.last_error
    );
    assert_eq!(progress.done, 1000);
    assert_eq!(new.calls.load(Ordering::SeqCst), 1000);
    assert!(enricher.jobs.lock().is_empty());
}

#[test]
fn recall_reembed_restart_resumes_checkpointed_units() {
    use crate::embedder::testing::wait_for;
    use crate::embedder::EmbedderEvent;
    let fx = fixture(MockProvider::new("old", "old", 384), StepClock::new());
    for title in ["One", "Two"] {
        fx.mem
            .save(note(title, "apples orchard harvest fruit"))
            .unwrap();
    }
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    release.send(()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    drop(release);
    wait_for(
        &fx.events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if progress.state == ReembedState::Failed),
    );
    assert_eq!(new.calls.load(Ordering::SeqCst), 1);
    new.ungate();
    let active = ActiveEmbedder::new(fx.embedder.provider());
    let (sender, events) = std::sync::mpsc::channel();
    active.subscribe(Arc::new(move |event| {
        let _ = sender.send(event.clone());
    }));
    let resumed =
        crate::MemManager::open_with(fx._dir.path(), active, StepClock::new(), None).unwrap();
    assert_eq!(resumed.embedder_status().reembed.done, 1);
    resumed.reconcile_embedder(new.clone()).unwrap();
    wait_for(
        &events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if matches!(progress.state, ReembedState::Completed | ReembedState::Failed)),
    );
    assert_eq!(
        resumed.embedder_status().reembed.state,
        ReembedState::Completed
    );
    assert_eq!(new.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn recall_reembed_deleted_item_cannot_survive_in_checkpoint_storage() {
    use crate::embedder::testing::wait_for;
    use crate::embedder::EmbedderEvent;
    let fx = fixture(MockProvider::new("old", "old", 384), StepClock::new());
    for title in ["One", "Two"] {
        fx.mem
            .save(note(title, "apples orchard harvest fruit"))
            .unwrap();
    }
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    release.send(()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let (key, id): (String, String) = fx
        .mem
        .conn
        .lock()
        .query_row(
            "SELECT unit_key, json_extract(checkpoint,'$.unit.id') FROM mem_reembed_shadow",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    fx.mem
        .conn
        .lock()
        .execute("DELETE FROM mem_items WHERE id=?1", [&id])
        .unwrap();
    let remaining: i64 = fx
        .mem
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM mem_reembed_shadow WHERE unit_key=?1",
            [key],
            |row| row.get(0),
        )
        .unwrap();
    drop(release);
    wait_for(
        &fx.events,
        |event| matches!(event,EmbedderEvent::Progress(progress) if progress.state==ReembedState::Failed),
    );
    assert_eq!(remaining, 0);
}

#[test]
fn recall_reembed_status_observer_does_not_reenter_publication_lock() {
    let provider = MockProvider::new("old", "old", 384);
    let fx = fixture(provider.clone(), StepClock::new());
    let manager = Arc::downgrade(&fx.mem);
    fx.embedder.subscribe(Arc::new(move |_| {
        let _ = manager.upgrade().unwrap().embedder_status();
    }));
    let _publication = fx.mem.vector_transition.write();
    provider.set_down(true);
    assert!(fx.embedder.passage("provider outage").is_err());
}
