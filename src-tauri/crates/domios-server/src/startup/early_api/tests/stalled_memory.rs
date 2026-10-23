use super::*;
use codeterm_mem::embedder::{
    ActiveEmbedder, Content, EmbedError, EmbeddingProvider, Modality, SystemClock,
};
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::oneshot;

const DEADLINE: Duration = Duration::from_secs(2);

struct StalledEmbedder {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl EmbeddingProvider for StalledEmbedder {
    fn id(&self) -> &str {
        "fixture"
    }
    fn model(&self) -> &str {
        "stalled"
    }
    fn dimension(&self) -> usize {
        384
    }
    fn max_tokens(&self) -> usize {
        512
    }
    fn modalities(&self) -> &[Modality] {
        &[Modality::Text]
    }
    fn remote(&self) -> bool {
        false
    }

    fn embed_passage(&self, _: &Content) -> Result<Vec<f32>, EmbedError> {
        self.entered.lock().take().unwrap().send(()).unwrap();
        let _ = self.release.lock().recv_timeout(DEADLINE);
        Err(EmbedError::Unavailable("fixture released".into()))
    }

    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed_passage(content)
    }
}

#[tokio::test]
async fn startup_reaches_ready_while_embedder_and_pending_memory_work_are_stalled() {
    tokio::time::timeout(DEADLINE, async {
        let directory = tempfile::tempdir().unwrap();
        let (entered, embedding_entered) = oneshot::channel();
        let (release, released) = mpsc::channel();
        let active = ActiveEmbedder::new(Arc::new(StalledEmbedder {
            entered: Mutex::new(Some(entered)),
            release: Mutex::new(released),
        }));
        let mem = codeterm_mem::MemManager::open_with(
            directory.path(),
            active.clone(),
            Arc::new(SystemClock),
            None,
        )
        .unwrap();
        let embedding_mem = mem.clone();
        let embedding = tokio::task::spawn_blocking(move || {
            embedding_mem.with_conn(|_| active.passage("a fixture memory awaiting embedding"))
        });
        embedding_entered.await.unwrap();

        let (blueprint_started, blueprint_entered) = oneshot::channel();
        let (blueprint_finished, mut blueprint_done) = oneshot::channel();
        let (extract_started, extract_entered) = oneshot::channel();
        let (extract_finished, mut extract_done) = oneshot::channel();
        let blueprint_mem = mem.clone();
        crate::startup::background_init::spawn_memory_tasks(
            move || {
                blueprint_started.send(()).unwrap();
                blueprint_mem.backfill_blueprints().unwrap();
                blueprint_finished.send(()).unwrap();
            },
            move || {
                extract_started.send(()).unwrap();
                codeterm_mem::auto_extract::find_pending_items(&mem, usize::MAX).unwrap();
                extract_finished.send(()).unwrap();
            },
        );
        blueprint_entered.await.unwrap();
        extract_entered.await.unwrap();
        assert_eq!(
            blueprint_done.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        assert_eq!(
            extract_done.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );

        let api = EarlyApi::new(None);
        complete_service(&api);
        api.complete("integrity");
        api.activate(Router::new().route("/api/probe", axum::routing::get(|| async { "ready" })))
            .unwrap();
        assert_eq!(api.changes.borrow().state, PhaseState::Ready);
        let response = api
            .router()
            .oneshot(request(Method::GET, "/api/probe"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), "ready");
        assert_eq!(
            extract_done.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );

        release.send(()).unwrap();
        embedding.await.unwrap().unwrap_err();
        blueprint_done.await.unwrap();
        extract_done.await.unwrap();
    })
    .await
    .expect("startup must reach ready without releasing memory work");
}
