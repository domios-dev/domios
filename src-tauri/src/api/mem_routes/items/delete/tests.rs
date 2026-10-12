use super::*;
use codeterm_mem::{ExtractionCancelSink, MemKind, MemScope, NewMemItem, RevokedGeneration};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Default)]
struct RuntimeCancellation(AtomicUsize);

impl ExtractionCancelSink for RuntimeCancellation {
    fn cancel(&self, revoked: &[RevokedGeneration]) {
        tauri::async_runtime::block_on(async {
            self.0.fetch_add(revoked.len(), Ordering::SeqCst);
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn memory_deletion_finishes_runtime_cancellation_before_returning() {
    for by_area in [false, true] {
        let mem = Arc::new(crate::mem::MemManager::open_in_memory().unwrap());
        let cancelled = Arc::new(RuntimeCancellation::default());
        mem.extraction_authority()
            .set_cancel_sink(cancelled.clone());
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Note,
                scope: MemScope::Global,
                title: "runtime source".into(),
                summary: None,
                area: Some("runtime".into()),
                tags: vec![],
                body: "source body".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        let uid = mem.get(&id).unwrap().unwrap().uid;
        let generation = mem.extraction_authority().open(&id, &uid);
        assert_eq!(
            mem.extraction_authority()
                .bind_worker(&generation, "worker"),
            codeterm_mem::BindOutcome::Active
        );
        if by_area {
            assert_eq!(
                delete_area(mem.clone(), "runtime".into())
                    .await
                    .unwrap()
                    .deleted,
                1
            );
        } else {
            assert!(delete_item(mem.clone(), id.clone(), "test".into())
                .await
                .unwrap());
        }
        assert!(mem.get(&id).unwrap().is_none());
        assert!(mem.extraction_authority().is_revoked(&generation));
        assert_eq!(cancelled.0.load(Ordering::SeqCst), 1);
    }
}
