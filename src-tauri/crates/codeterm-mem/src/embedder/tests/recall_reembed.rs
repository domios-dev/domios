use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::embedder::provider::EmbeddingProvider;
use crate::embedder::testing::{
    fixture, note, vector_rows, wait_for, Fixture, MockProvider, StepClock, DEADLINE,
};
use crate::embedder::{
    index, pending, EmbedError, EmbedderEvent, EmbedderIdentity, ReembedProgress, ReembedState,
};

fn seeded(bodies: &[&str]) -> (Fixture, Arc<MockProvider>, Vec<String>) {
    let old = MockProvider::new("old", "old", 384);
    let fx = fixture(old.clone(), StepClock::new());
    let ids = bodies
        .iter()
        .map(|body| {
            fx.mem
                .save(note(body, &format!("{body} kept as a fixture memory body")))
                .unwrap()
        })
        .collect::<Vec<String>>();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        bodies.len()
    );
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    (fx, old, ids)
}

fn settled(fx: &Fixture, state: ReembedState) -> ReembedProgress {
    let EmbedderEvent::Progress(progress) = wait_for(
        &fx.events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if progress.state == state),
    ) else {
        unreachable!()
    };
    progress
}

fn stamp(fx: &Fixture) -> Option<EmbedderIdentity> {
    index::stamp(&fx.mem.conn.lock()).unwrap()
}

fn top(fx: &Fixture, query: &str) -> Option<String> {
    fx.mem
        .vector_recall(query, 5)
        .unwrap()
        .first()
        .map(|hit| hit.0.clone())
}

#[test]
fn recall_reembed_retains_old_semantic_index_until_completion() {
    let (fx, _, ids) = seeded(&["apples orchard harvest fruit"]);
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let during = top(&fx, "apples orchard harvest fruit");
    release.send(()).unwrap();
    settled(&fx, ReembedState::Completed);
    assert_eq!(during.as_ref(), Some(&ids[0]));
    assert_eq!(stamp(&fx), Some(new.identity()));
    assert_eq!(fx.embedder.identity(), new.identity());
    assert_eq!(
        top(&fx, "apples orchard harvest fruit").as_ref(),
        Some(&ids[0])
    );
}

#[test]
fn recall_reembed_running_build_keeps_recall_healthy_and_reports_progress() {
    let (fx, _, _) = seeded(&["apples orchard harvest fruit"]);
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let status = fx.mem.embedder_status();
    release.send(()).unwrap();
    settled(&fx, ReembedState::Completed);
    assert!(!status.degraded);
    assert_eq!(status.reembed.state, ReembedState::Running);
    assert_eq!(status.reembed.target, Some(new.identity()));
    assert_eq!((status.reembed.done, status.reembed.total), (0, 1));
}

#[test]
fn recall_reembed_same_model_does_not_start_a_rebuild() {
    let (fx, old, ids) = seeded(&["apples orchard harvest fruit"]);
    let calls = old.calls.load(Ordering::SeqCst);
    let plan = fx.mem.switch_embedder(old.clone()).unwrap();
    assert_eq!(old.calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        top(&fx, "apples orchard harvest fruit").as_ref(),
        Some(&ids[0])
    );
    assert_eq!(plan.total(), 0);
    assert_eq!(fx.mem.embedder_status().reembed.state, ReembedState::Idle);
}

#[test]
fn recall_reembed_refused_item_is_counted_and_the_run_publishes() {
    let (fx, _, ids) = seeded(&["apples orchard", "secret token value", "pears basket"]);
    let new = MockProvider::new("new", "new", 768);
    new.fail_on(
        "token",
        EmbedError::Refused("credential filter blocked text".into()),
    );
    fx.mem.switch_embedder(new.clone()).unwrap();
    let progress = settled(&fx, ReembedState::Completed);
    assert_eq!((progress.done, progress.failed), (3, 1));
    assert!(progress.last_error.unwrap().contains(&ids[1]));
    assert_eq!(stamp(&fx), Some(new.identity()));
    assert!(!vector_rows(&fx.mem)
        .iter()
        .any(|row| row.starts_with(&ids[1])));
    assert!(pending::ids(&fx.mem.conn.lock()).unwrap().contains(&ids[1]));
    assert_eq!(top(&fx, "pears basket").as_ref(), Some(&ids[2]));
}

#[test]
fn recall_reembed_single_rate_limited_item_does_not_abort_the_run() {
    let (fx, _, ids) = seeded(&["apples orchard", "flaky pears", "plums"]);
    let new = MockProvider::new("new", "new", 768);
    new.fail_on("flaky", EmbedError::Unavailable("429 rate limited".into()));
    fx.mem.switch_embedder(new.clone()).unwrap();
    let progress = settled(&fx, ReembedState::Completed);
    assert_eq!(progress.failed, 1);
    assert_eq!(stamp(&fx), Some(new.identity()));
    assert_eq!(top(&fx, "apples orchard").as_ref(), Some(&ids[0]));
}

#[test]
fn recall_reembed_provider_outage_fails_truthfully_and_the_next_switch_recovers() {
    let bodies = ["one", "two", "three", "four", "five", "six"];
    let (fx, old, ids) = seeded(&bodies);
    let new = MockProvider::new("new", "new", 768);
    new.set_down(true);
    fx.mem.switch_embedder(new.clone()).unwrap();
    let progress = settled(&fx, ReembedState::Failed);
    assert!(progress.last_error.unwrap().contains("consecutive"));
    let status = fx.mem.embedder_status();
    assert!(!status.degraded);
    assert_eq!(status.reembed.state, ReembedState::Failed);
    assert_eq!(stamp(&fx), Some(old.identity()));
    assert_eq!(top(&fx, "three").as_ref(), Some(&ids[2]));

    new.set_down(false);
    fx.mem.switch_embedder(new.clone()).unwrap();
    let progress = settled(&fx, ReembedState::Completed);
    assert_eq!((progress.done, progress.failed), (6, 0));
    assert!(!fx.mem.embedder_status().degraded);
    assert_eq!(top(&fx, "three").as_ref(), Some(&ids[2]));
}

#[test]
fn recall_reembed_checkpoints_are_binary_hashed_rows_outside_knowledge_db() {
    let (fx, _, _) = seeded(&["apples orchard", "pears basket"]);
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    release.send(()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let (row, triggers, main_tables, shadow) = {
        let conn = fx.mem.conn.lock();
        let row: (String, i64, String, i64) = conn
            .query_row(
                "SELECT typeof(vectors), length(vectors), typeof(text_hash), length(text_hash) FROM reembed.shadow",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let count = |sql: &str| conn.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap();
        let shadow: String = conn
            .query_row(
                "SELECT file FROM pragma_database_list WHERE name='reembed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        (
            row,
            count(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'reembed%'",
            ),
            count("SELECT COUNT(*) FROM main.sqlite_master WHERE name LIKE '%reembed%'"),
            shadow,
        )
    };
    release.send(()).unwrap();
    settled(&fx, ReembedState::Completed);
    assert_eq!(row, ("blob".into(), 768 * 4, "blob".into(), 32));
    assert_eq!((triggers, main_tables), (0, 0));
    assert!(!std::path::Path::new(&shadow).exists());
    let attached: i64 = fx
        .mem
        .conn
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM pragma_database_list WHERE name='reembed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attached, 0);
}

#[test]
fn recall_reembed_changes_during_build_resume_without_a_manual_retry() {
    let (fx, _, ids) = seeded(&["apples orchard", "pears basket"]);
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    release.send(()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let added = fx.mem.save(note("Late", "plums cellar")).unwrap();
    assert!(fx.mem.delete(&ids[0]).unwrap());
    new.ungate();
    release.send(()).unwrap();
    let progress = settled(&fx, ReembedState::Completed);
    assert_eq!(new.calls.load(Ordering::SeqCst), 3);
    assert_eq!((progress.done, progress.total), (3, 3));
    let rows = vector_rows(&fx.mem);
    assert!(rows.iter().any(|row| row.starts_with(&added)));
    assert!(rows.iter().any(|row| row.starts_with(&ids[1])));
    assert!(!rows.iter().any(|row| row.starts_with(&ids[0])));
    assert_eq!(top(&fx, "plums cellar").as_ref(), Some(&added));
}

#[test]
fn recall_reembed_choosing_the_published_model_cancels_and_frees_the_switch() {
    let (fx, old, _) = seeded(&["apples orchard"]);
    let abandoned = MockProvider::new("abandoned", "b", 768);
    let (b_started, b_release) = abandoned.gated();
    fx.mem.switch_embedder(abandoned.clone()).unwrap();
    b_started.recv_timeout(DEADLINE).unwrap();

    assert_eq!(fx.mem.switch_embedder(old.clone()).unwrap().total(), 0);
    let status = fx.mem.embedder_status();
    assert_eq!(status.reembed.state, ReembedState::Cancelled);
    assert!(status.reembed.last_error.unwrap().contains("old/old"));

    let next = MockProvider::new("next", "c", 512);
    let (c_started, c_release) = next.gated();
    let plan = fx.mem.switch_embedder(next.clone()).unwrap();
    c_started.recv_timeout(DEADLINE).unwrap();
    let other = MockProvider::new("other", "d", 256);
    assert!(fx
        .mem
        .switch_embedder(other)
        .unwrap_err()
        .contains("another re-embed"));
    assert_eq!(fx.mem.switch_embedder(next.clone()).unwrap(), plan);
    c_release.send(()).unwrap();
    settled(&fx, ReembedState::Completed);
    drop(b_release);
    assert_eq!(stamp(&fx), Some(next.identity()));
    assert_eq!(fx.embedder.identity(), next.identity());
    assert_eq!(abandoned.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn recall_reembed_restart_resumes_checkpointed_units() {
    let bodies = ["one", "two", "three", "four", "five", "six"];
    let (fx, _, _) = seeded(&bodies);
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    release.send(()).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    new.set_down(true);
    new.ungate();
    release.send(()).unwrap();
    settled(&fx, ReembedState::Failed);
    assert_eq!(new.calls.load(Ordering::SeqCst), 1);

    new.set_down(false);
    let active = crate::embedder::ActiveEmbedder::new(fx.embedder.provider());
    let (sender, events) = std::sync::mpsc::channel();
    active.subscribe(Arc::new(move |event| {
        let _ = sender.send(event.clone());
    }));
    let resumed =
        crate::MemManager::open_with(fx._dir.path(), active, StepClock::new(), None).unwrap();
    resumed.reconcile_embedder(new.clone()).unwrap();
    wait_for(
        &events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if progress.state == ReembedState::Completed),
    );
    assert_eq!(new.calls.load(Ordering::SeqCst), 6);
    assert_eq!(resumed.embedder_status().reembed.done, 6);
}

#[test]
fn recall_reembed_status_observer_during_publication_does_not_deadlock() {
    let (fx, _, _) = seeded(&["apples orchard"]);
    let manager = Arc::downgrade(&fx.mem);
    fx.embedder.subscribe(Arc::new(move |_| {
        let _ = manager.upgrade().unwrap().embedder_status();
    }));
    let new = MockProvider::new("new", "new", 768);
    fx.mem.switch_embedder(new.clone()).unwrap();
    settled(&fx, ReembedState::Completed);
    assert_eq!(fx.mem.embedder_status().index, Some(new.identity()));
}

#[test]
fn recall_reembed_1k_items_need_one_provider_and_no_enrichment() {
    let old = MockProvider::new("old", "old", 384);
    let mem = crate::MemManager::open_in_memory().unwrap();
    mem.embedder.replace(old.clone());
    index::recreate(&mem.conn.lock(), &old.identity()).unwrap();
    crate::backend::sqlite::blueprint::initialize(&mem.conn.lock()).unwrap();
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
    let EmbedderEvent::Progress(progress) = wait_for(
        &events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if matches!(progress.state, ReembedState::Completed | ReembedState::Failed)),
    ) else {
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
fn recall_reembed_watchdog_cannot_fail_a_claimed_publication() {
    let provider = MockProvider::new("old", "old", 384);
    let active = crate::embedder::ActiveEmbedder::new(provider.clone());
    let tracker = crate::embedder::switch::ReembedTracker::new(StepClock::new());
    let generation = tracker.start(
        provider.identity(),
        crate::embedder::SwitchPlan::default(),
        &active,
    );
    assert!(tracker.claim(generation));
    tracker.fail(generation, "worker stopped advancing".into(), &active);
    assert_eq!(tracker.snapshot().state, ReembedState::Running);
    assert!(!tracker.cancel("late cancel".into(), &active));
    tracker.finish(generation, &active);
    assert_eq!(tracker.snapshot().state, ReembedState::Completed);
}

#[test]
fn recall_reembed_worker_beats_around_each_diff_and_publication() {
    let (fx, _, _) = seeded(&["apples orchard"]);
    let new = MockProvider::new("new", "new", 768);
    fx.mem.prepare_shadow(&new.identity()).unwrap();
    let generation =
        fx.mem
            .reembed
            .start(new.identity(), fx.mem.reembed_plan().unwrap(), &fx.embedder);
    let (beat, beats) = std::sync::mpsc::channel();
    fx.mem
        .run_shadow(
            generation,
            crate::embedder::ActiveEmbedder::new(new.clone()),
            &beat,
        )
        .unwrap();
    assert_eq!(beats.try_iter().count(), 7);
    assert_eq!(stamp(&fx), Some(new.identity()));
}

#[test]
fn recall_reembed_unavailable_unit_is_retried_once_before_publication() {
    let (fx, _, ids) = seeded(&["apples orchard", "flaky pears", "plums"]);
    let new = MockProvider::new("new", "new", 768);
    new.fail_once("flaky", EmbedError::Unavailable("timeout".into()));
    fx.mem.switch_embedder(new.clone()).unwrap();
    let progress = settled(&fx, ReembedState::Completed);
    assert_eq!(progress.failed, 0);
    assert!(vector_rows(&fx.mem)
        .iter()
        .any(|row| row.starts_with(&ids[1])));
    assert!(!pending::ids(&fx.mem.conn.lock()).unwrap().contains(&ids[1]));
}

#[test]
fn recall_reembed_startup_removes_a_shadow_left_by_a_crash() {
    let (fx, old, _) = seeded(&["apples orchard"]);
    let main: String = fx
        .mem
        .conn
        .lock()
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name='main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let orphan = format!("{main}.reembed");
    std::fs::write(&orphan, b"left by a crash").unwrap();
    assert_eq!(fx.mem.reconcile_embedder(old).unwrap(), None);
    assert!(!std::path::Path::new(&orphan).exists());
}

#[test]
fn shadow_residency_failed_run_can_be_discarded_then_retried_cleanly() {
    let (fx, old, ids) = seeded(&["one", "two", "three", "four", "five", "six"]);
    let new = MockProvider::new("new", "new", 768);
    new.set_down(true);
    fx.mem.switch_embedder(new.clone()).unwrap();
    settled(&fx, ReembedState::Failed);
    let shadow: String = fx
        .mem
        .conn
        .lock()
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name='reembed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(std::path::Path::new(&shadow).exists());
    assert_eq!(fx.mem.embedder_status().model, old.identity().model);
    assert_eq!(
        fx.mem.embedder_status().reembed.target,
        Some(new.identity())
    );
    fx.mem.switch_embedder(old.clone()).unwrap();
    assert!(!std::path::Path::new(&shadow).exists());
    assert_eq!(
        fx.mem.embedder_status().reembed.state,
        ReembedState::Cancelled
    );
    assert_eq!(stamp(&fx), Some(old.identity()));
    assert_eq!(top(&fx, "three").as_ref(), Some(&ids[2]));
    new.set_down(false);
    fx.mem.switch_embedder(new.clone()).unwrap();
    settled(&fx, ReembedState::Completed);
    assert_eq!(stamp(&fx), Some(new.identity()));
    assert!(!std::path::Path::new(&shadow).exists());
    assert_eq!(top(&fx, "three").as_ref(), Some(&ids[2]));
}

#[test]
fn recall_reembed_progress_observers_can_read_status_without_holding_connection() {
    let (fx, _, ids) = seeded(&["orchard harvest"]);
    let mem = Arc::downgrade(&fx.mem);
    let changed = std::sync::atomic::AtomicBool::new(false);
    let (observed, observations) = std::sync::mpsc::channel();
    fx.embedder.subscribe(Arc::new(move |event| {
        let EmbedderEvent::Progress(progress) = event else {
            return;
        };
        let mem = mem.upgrade().unwrap();
        let unlocked = mem.conn.try_lock().is_some();
        if unlocked {
            let _ = mem.embedder_status();
            if progress.done == 1 && !changed.swap(true, Ordering::SeqCst) {
                mem.with_conn(|conn| {
                    conn.execute(
                        "UPDATE mem_items SET body_inline = body_inline || ' changed' WHERE id=?1",
                        [&ids[0]],
                    )
                    .unwrap()
                });
            }
        }
        observed.send(unlocked).unwrap();
    }));
    let new = MockProvider::new("new", "new", 768);
    fx.mem.prepare_shadow(&new.identity()).unwrap();
    let generation =
        fx.mem
            .reembed
            .start(new.identity(), fx.mem.reembed_plan().unwrap(), &fx.embedder);
    let (beat, _) = std::sync::mpsc::channel();
    fx.mem
        .run_shadow(generation, crate::embedder::ActiveEmbedder::new(new), &beat)
        .unwrap();
    let observations: Vec<_> = observations.try_iter().collect();
    assert!(!observations.is_empty());
    assert!(observations.iter().all(|unlocked| *unlocked));
}

#[test]
fn recall_reembed_switch_refuses_a_busy_connection_without_waiting() {
    let (fx, old, _) = seeded(&["orchard harvest"]);
    let new = MockProvider::new("new", "new", 768);
    let guard = fx.mem.conn.lock();
    let refusal = fx.mem.try_switch_embedder(new).unwrap_err();
    assert!(refusal.contains("memory is busy"));
    assert_eq!(fx.mem.reembed_progress().state, ReembedState::Idle);
    assert_eq!(fx.embedder.identity(), old.identity());
    drop(guard);
}

#[test]
fn recall_reembed_large_item_reports_batches_before_the_item_completes() {
    let body = "orchard harvest fruit ".repeat(800);
    let (fx, _, _) = seeded(&[&body]);
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new.clone()).unwrap();
    let total = loop {
        match fx.events.recv_timeout(DEADLINE).unwrap() {
            EmbedderEvent::Progress(progress) if progress.current_item.is_some() => {
                let item = progress.current_item.unwrap();
                assert_eq!(item.done, 0);
                break item.total;
            }
            _ => {}
        }
    };
    assert!(total > 8);
    for _ in 0..8 {
        started.recv_timeout(DEADLINE).unwrap();
        release.send(()).unwrap();
    }
    started.recv_timeout(DEADLINE).unwrap();
    let event = wait_for(&fx.events, |event| {
        matches!(event, EmbedderEvent::Progress(progress)
            if progress.current_item.as_ref().is_some_and(|item| item.done == 8))
    });
    let EmbedderEvent::Progress(progress) = event else {
        unreachable!()
    };
    assert_eq!(progress.done, 0);
    assert_eq!(progress.current_item.unwrap().total, total);
    assert!(progress.updated_at > progress.started_at);
    new.ungate();
    release.send(()).unwrap();
    settled(&fx, ReembedState::Completed);
}

#[test]
fn recall_reembed_failed_or_cancelled_shadow_keeps_published_status_and_semantic_recall() {
    for cancel in [false, true] {
        let (fx, old, ids) = seeded(&["one", "two", "three", "four", "five", "six"]);
        let target = MockProvider::new("new", "target", 768);
        if cancel {
            let (started, release) = target.gated();
            fx.mem.switch_embedder(target.clone()).unwrap();
            started.recv_timeout(DEADLINE).unwrap();
            fx.mem.switch_embedder(old.clone()).unwrap();
            target.ungate();
            release.send(()).unwrap();
            assert_eq!(fx.mem.reembed_progress().state, ReembedState::Cancelled);
        } else {
            target.set_down(true);
            fx.mem.switch_embedder(target.clone()).unwrap();
            settled(&fx, ReembedState::Failed);
        }
        target.set_down(true);
        let retired_target = crate::embedder::ActiveEmbedder::new(target);
        assert!(retired_target.passage("late shadow request").is_err());
        assert!(!retired_target.is_available());

        let status = fx.mem.embedder_status();
        assert!(status.available);
        assert!(!status.degraded);
        assert_eq!(status.index, Some(old.identity()));
        let (hits, degraded) = crate::embedder::recall_priority::with_recall_status(|| {
            fx.mem.vector_recall("three", 1).unwrap()
        });
        assert!(!degraded);
        assert_eq!(hits[0].0, ids[2]);
    }
}
