use super::*;

#[test]
fn live_queue_status_reports_backlog_in_flight_failures_and_unavailability() {
    let shared = Arc::new(Shared::default());
    let queue = EmbedQueue {
        handle: Some(Arc::new(Handle(shared.clone()))),
    };
    for i in 0..410 {
        shared
            .state
            .lock()
            .schedule
            .submit(&i.to_string(), vec!["passage".into()], QUEUE_CAPACITY);
    }
    assert_eq!(queue.status(false).queued, 410);
    let unit = next_unit(&shared).unwrap();
    let status = queue.status(false);
    assert_eq!(
        (status.queued, status.in_flight, status.failed),
        (409, 1, 0)
    );
    let json = serde_json::to_value(status).unwrap();
    assert_eq!(json["queued"], 409);
    assert_eq!(json["in_flight"], 1);
    {
        let mut state = shared.state.lock();
        state.schedule.finish(&unit);
        state.busy = false;
        state.failed = 2;
        state.closed = true;
    }
    let status = queue.status(false);
    assert_eq!(
        (status.queued, status.in_flight, status.failed),
        (409, 0, 2)
    );
    assert!(status.unavailable);
    assert!(EmbedQueue::disabled().status(false).unavailable);
}

#[test]
fn enqueue_when_disabled_is_noop() {
    let q = EmbedQueue::disabled();
    q.enqueue("abc", "hello world");
    q.cancel("abc");
    assert!(q.wait_idle(Duration::from_secs(1)));
}

#[cfg(feature = "embeddings")]
mod worker {
    use super::super::*;
    use crate::store::{MemKind, MemScope, NewMemItem};
    use crate::MemManager;
    use std::sync::mpsc::{Receiver, Sender};

    const DEADLINE: Duration = Duration::from_secs(30);

    /// An embedder that reports each chunk it receives and then blocks until
    /// the test releases it, so the test steps the worker one unit at a time.
    struct Gate {
        started: Receiver<String>,
        release: Sender<()>,
    }

    impl Gate {
        fn new() -> (Self, Arc<ActiveEmbedder>) {
            let provider = crate::embedder::testing::MockProvider::new("mock", "gate-384", 384);
            let (started, release) = provider.gated();
            (Self { started, release }, ActiveEmbedder::new(provider))
        }

        fn next_started(&self) -> String {
            self.started
                .recv_timeout(DEADLINE)
                .expect("worker never reached the embedder")
        }

        fn release(&self) {
            self.release.send(()).expect("worker gate dropped");
        }
    }

    fn note(title: &str, body: &str) -> NewMemItem {
        NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: title.into(),
            summary: None,
            area: None,
            tags: vec![],
            body: body.into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        }
    }

    fn long_body(words: usize) -> String {
        (0..words)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn vector_count(mem: &MemManager, id: &str) -> i64 {
        mem.with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
                rusqlite::params![id, format!("{id}_chunk_%")],
                |r| r.get(0),
            )
            .unwrap()
        })
    }

    fn orphan_vectors(mem: &MemManager) -> Vec<String> {
        mem.with_conn(|conn| crate::graph_repair::repair_orphan_vectors(conn, true, "test"))
            .unwrap()
            .orphan_ids
    }

    #[test]
    fn graph_embedding_is_queued_without_holding_the_database_lock() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, embedder) = Gate::new();
        let mem = MemManager::open_with(
            dir.path(),
            embedder,
            Arc::new(crate::embedder::SystemClock),
            None,
        )
        .unwrap();
        mem.graph_create_entity(&crate::Entity {
            id: "queued-person".into(),
            entity_type: "person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Ada"}),
        })
        .unwrap();
        assert!(gate.next_started().contains("ada"));
        assert!(mem.conn.try_lock().is_some());
        gate.release();
        assert!(mem.embed_queue.wait_idle(DEADLINE));
        assert_eq!(vector_count(&mem, "queued-person"), 1);
    }

    #[test]
    fn embed_queue_fair_small_item_not_starved() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, embedder) = Gate::new();
        let mem = MemManager::open_with(
            dir.path(),
            embedder,
            Arc::new(crate::embedder::SystemClock),
            None,
        )
        .unwrap();

        let big = mem
            .insert_raw(&note("fair big item", "placeholder body for the big item"))
            .unwrap();
        let backlog: Vec<String> = (0..3000).map(|i| format!("big-chunk-{i}")).collect();
        mem.embed_queue.submit_chunks(&big, backlog);
        assert_eq!(gate.next_started(), "big-chunk-0");

        let small = mem
            .save(note("fair small item", "smallmarker one line note"))
            .unwrap();
        gate.release();

        assert!(gate.next_started().contains("smallmarker"));
        gate.release();

        assert_eq!(gate.next_started(), "big-chunk-1");
        assert_eq!(vector_count(&mem, &small), 1);
        assert_eq!(vector_count(&mem, &big), 1);

        assert!(mem.delete(&big).unwrap());
        gate.release();
        assert!(mem.embed_queue.wait_idle(DEADLINE));
        assert!(gate.started.try_recv().is_err());
    }

    #[test]
    fn delete_cancels_pending_chunks_no_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, embedder) = Gate::new();
        let mem = MemManager::open_with(
            dir.path(),
            embedder,
            Arc::new(crate::embedder::SystemClock),
            None,
        )
        .unwrap();

        let doomed = mem
            .save(note("doomed flush item", &long_body(40_000)))
            .unwrap();
        gate.next_started();
        assert!(mem.delete(&doomed).unwrap());
        gate.release();

        assert!(mem.embed_queue.wait_idle(DEADLINE));
        assert!(
            gate.started.try_recv().is_err(),
            "cancelled chunks were still embedded"
        );
        assert_eq!(vector_count(&mem, &doomed), 0);
        assert!(orphan_vectors(&mem).is_empty());
    }

    #[test]
    fn delete_outside_the_queue_still_leaves_no_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, embedder) = Gate::new();
        let mem = MemManager::open_with(
            dir.path(),
            embedder,
            Arc::new(crate::embedder::SystemClock),
            None,
        )
        .unwrap();

        let raced = mem
            .save(note("raced delete item", &long_body(40_000)))
            .unwrap();
        gate.next_started();
        assert!(mem
            .with_conn(|conn| crate::store::delete(conn, &raced))
            .unwrap());
        gate.release();

        assert!(mem.embed_queue.wait_idle(DEADLINE));
        assert!(
            gate.started.try_recv().is_err(),
            "chunks of a vanished item were still embedded"
        );
        assert_eq!(vector_count(&mem, &raced), 0);
        assert!(orphan_vectors(&mem).is_empty());
    }

    #[test]
    fn replace_supersedes_pending_chunks_of_the_old_body() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, embedder) = Gate::new();
        let mem = MemManager::open_with(
            dir.path(),
            embedder,
            Arc::new(crate::embedder::SystemClock),
            None,
        )
        .unwrap();

        let id = mem.save(note("replaced item", &long_body(40_000))).unwrap();
        gate.next_started();
        let replaced = mem
            .save_replace(note("replaced item", "replacementmarker short body"))
            .unwrap();
        assert_eq!(replaced, id);
        gate.release();

        assert!(gate.next_started().contains("replacementmarker"));
        gate.release();
        assert!(mem.embed_queue.wait_idle(DEADLINE));
        assert_eq!(vector_count(&mem, &id), 1);
    }
}
