use super::*;

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
    use std::sync::mpsc::{self, Receiver, Sender};

    const DEADLINE: Duration = Duration::from_secs(30);

    /// An embedder that reports each chunk it receives and then blocks until
    /// the test releases it, so the test steps the worker one unit at a time.
    struct Gate {
        started: Receiver<String>,
        release: Sender<()>,
    }

    impl Gate {
        fn new() -> (Self, Embedder) {
            let (started_tx, started) = mpsc::channel::<String>();
            let (release, release_rx) = mpsc::channel::<()>();
            let started_tx = Mutex::new(started_tx);
            let release_rx = Mutex::new(release_rx);
            let embedder: Embedder = Arc::new(move |text: &str| {
                let _ = started_tx.lock().send(text.to_string());
                release_rx
                    .lock()
                    .recv()
                    .map_err(|_| "gate closed".to_string())?;
                Ok(vec![0.25; 384])
            });
            (Self { started, release }, embedder)
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
    fn embed_queue_fair_small_item_not_starved() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, embedder) = Gate::new();
        let mem = MemManager::open_with_embedder(dir.path(), embedder).unwrap();

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
        let mem = MemManager::open_with_embedder(dir.path(), embedder).unwrap();

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
        let mem = MemManager::open_with_embedder(dir.path(), embedder).unwrap();

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
        let mem = MemManager::open_with_embedder(dir.path(), embedder).unwrap();

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
