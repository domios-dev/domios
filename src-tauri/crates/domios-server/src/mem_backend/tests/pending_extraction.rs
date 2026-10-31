use super::*;
use codeterm_mem::backend::{contract, WriteContext};
use codeterm_service_core::{
    mem::worker_pool::WorkerPool, multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};
use std::time::Duration;

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_pending_extraction_current_thread() {
    pending_extraction(false);
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_pending_extraction_multi_thread() {
    pending_extraction(true);
}

fn pending_extraction(multi_thread: bool) {
    let url = std::env::var("DOMIOS_TEST_POSTGRES_URL").expect("disposable Postgres URL");
    let directory = tempfile::tempdir().unwrap();
    let store = codeterm_mem_postgres::open(
        &url,
        codeterm_mem_postgres::PostgresOptions {
            schema: format!("extract_test_{}", uuid::Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .unwrap();
    let ctx = WriteContext::system("pending-startup-fixture");
    let ids = ["pending first", "pending second"]
        .map(|title| store.save(&ctx, &contract::item(title), false).unwrap().id);
    assert_eq!(store.pending_extraction(usize::MAX).unwrap().len(), 2);
    let mem = codeterm_mem::MemManager::with_store(
        store.clone(),
        directory.path(),
        ActiveEmbedder::new(Arc::new(Provider)),
        Arc::new(codeterm_mem::embedder::SystemClock),
        None,
    )
    .unwrap();
    let (mux, _) = MultiplexerState::new();
    let mut fixture = ServiceFixture::new(mux);
    fixture.service.mem = mem;
    let svc = Arc::new(fixture.service.clone());
    let (spawned, mut events) = tokio::sync::mpsc::unbounded_channel();
    let workers = svc.workers.clone();
    fixture.ui.listen("graph-worker-spawned", move |event| {
        let worker_id = event["worker_id"].as_str().unwrap();
        workers.kill(worker_id).unwrap();
        spawned.send(event.clone()).unwrap();
    });
    let mut builder = if multi_thread {
        tokio::runtime::Builder::new_multi_thread()
    } else {
        tokio::runtime::Builder::new_current_thread()
    };
    let runtime = builder.enable_all().build().unwrap();
    runtime.block_on(async {
        let provider = svc.configuration().general_agent.agent_provider;
        let pool = svc.ext::<Arc<WorkerPool>>();
        let _permit = pool.acquire(&provider).await;
        let (queued, count) = tokio::sync::oneshot::channel();
        let replay = svc.clone();
        crate::startup::background_init::spawn_memory_tasks(
            || {},
            move || {
                queued
                    .send(codeterm_service_core::mem_host::resume_extract_pending(
                        replay,
                        usize::MAX,
                    ))
                    .unwrap();
            },
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            assert_eq!(count.await.unwrap(), 2);
            let mut intents = Vec::new();
            for _ in &ids {
                intents.push(
                    events.recv().await.unwrap()["intent"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                );
            }
            for id in &ids {
                assert!(intents.contains(&format!("extract:{id}")));
            }
        })
        .await
        .expect("pending startup extraction reaches worker registration without a panic");
    });
    drop(runtime);
    for id in ids {
        assert!(store.get(&id).unwrap().is_some());
    }
}
