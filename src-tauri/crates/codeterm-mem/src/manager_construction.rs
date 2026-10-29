use crate::{
    backend::{MemStore, SqliteMemStore, WriteContext},
    embed_queue, embedder, graph_layout, ExtractionAuthority, MemManager, HEAVY_OPS_PERMITS,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Semaphore;

impl MemManager {
    pub fn open(data_dir: &Path) -> Result<Arc<Self>, String> {
        let mem_dir = data_dir.join("mem");
        let store = Arc::new(SqliteMemStore::open(
            &mem_dir.join("knowledge.db"),
            &mem_dir.join("blobs"),
        )?);
        Self::open_store(store, data_dir)
    }
    pub fn open_store(store: Arc<dyn MemStore>, data_dir: &Path) -> Result<Arc<Self>, String> {
        let manager = Self::with_store(
            store,
            data_dir,
            embedder::ActiveEmbedder::global(),
            Arc::new(embedder::SystemClock),
            Some(data_dir.to_path_buf()),
        )?;
        crate::docs_source::start_background_build(data_dir);
        crate::spawn_entity_embedding_backfill(manager.store.clone(), manager.embedder.clone());
        Ok(manager)
    }
    pub fn open_with(
        data_dir: &Path,
        embedder: Arc<embedder::ActiveEmbedder>,
        clock: Arc<dyn embedder::Clock>,
        docs_dir: Option<PathBuf>,
    ) -> Result<Arc<Self>, String> {
        let mem_dir = data_dir.join("mem");
        let store = Arc::new(SqliteMemStore::open(
            &mem_dir.join("knowledge.db"),
            &mem_dir.join("blobs"),
        )?);
        Self::construct(store, data_dir, &mem_dir, embedder, clock, docs_dir, true)
    }
    pub fn with_store(
        store: Arc<dyn MemStore>,
        data_dir: &Path,
        embedder: Arc<embedder::ActiveEmbedder>,
        clock: Arc<dyn embedder::Clock>,
        docs_dir: Option<PathBuf>,
    ) -> Result<Arc<Self>, String> {
        Self::construct(
            store,
            data_dir,
            &data_dir.join("mem"),
            embedder,
            clock,
            docs_dir,
            true,
        )
    }
    pub fn open_in_memory() -> Result<Arc<Self>, String> {
        let tmp = std::env::temp_dir().join(format!("mem-test-{}", ulid::Ulid::new()));
        #[cfg(feature = "test-support")]
        let store = Arc::new(SqliteMemStore::open_fixture_memory(&tmp.join("blobs"))?);
        #[cfg(not(feature = "test-support"))]
        let store = Arc::new(SqliteMemStore::open_in_memory(&tmp.join("blobs"))?);
        Self::construct(
            store,
            &tmp,
            &tmp,
            Self::offline_embedder(true),
            Arc::new(embedder::SystemClock),
            None,
            false,
        )
    }
    pub fn open_offline(data_dir: &Path) -> Result<Arc<Self>, String> {
        let store = Arc::new(SqliteMemStore::open(
            &data_dir.join("knowledge.db"),
            &data_dir.join("blobs"),
        )?);
        let _ = store.init_vectors(
            &WriteContext::system("offline"),
            &embedder::ActiveEmbedder::global().identity(),
        );
        Self::construct(
            store,
            data_dir,
            data_dir,
            Self::offline_embedder(false),
            Arc::new(embedder::SystemClock),
            None,
            false,
        )
    }
    fn offline_embedder(down: bool) -> Arc<embedder::ActiveEmbedder> {
        #[cfg(test)]
        {
            let model = embedder::local::default_model();
            let provider = embedder::testing::MockProvider::new(
                embedder::local::ID,
                model.model,
                model.dimension,
            );
            provider.set_down(down);
            embedder::ActiveEmbedder::new(provider)
        }
        #[cfg(not(test))]
        {
            let _ = down;
            embedder::ActiveEmbedder::global()
        }
    }
    fn construct(
        store: Arc<dyn MemStore>,
        data_dir: &Path,
        mem_dir: &Path,
        embedder: Arc<embedder::ActiveEmbedder>,
        clock: Arc<dyn embedder::Clock>,
        docs_dir: Option<PathBuf>,
        background: bool,
    ) -> Result<Arc<Self>, String> {
        std::fs::create_dir_all(mem_dir.join("blobs")).map_err(|e| e.to_string())?;
        if background {
            if let Err(error) =
                store.init_vectors(&WriteContext::system("startup"), &embedder.identity())
            {
                log::warn!("[mem] vector index init failed: {error} — semantic recall disabled");
            }
        }
        let heavy_ops_permits = Arc::new(Semaphore::new(HEAVY_OPS_PERMITS));
        let layout = graph_layout::LayoutState::load(mem_dir);
        let embed_queue = if background {
            embed_queue::EmbedQueue::start(
                store.background_store()?,
                heavy_ops_permits.clone(),
                layout.cache.clone(),
                embedder.clone(),
            )
        } else {
            embed_queue::EmbedQueue::disabled()
        };
        let manager = Arc::new(Self {
            blueprint: Default::default(),
            store,
            db_path: mem_dir.join("knowledge.db"),
            blobs_dir: mem_dir.join("blobs"),
            skill_dir: data_dir.join("skills"),
            embed_queue,
            embedder,
            reembed: embedder::switch::ReembedTracker::new(clock),
            docs_dir,
            #[cfg(test)]
            docs_sections: Default::default(),
            heavy_ops_permits,
            layout,
            extraction: Arc::new(ExtractionAuthority::new()),
        });
        if background {
            manager
                .reembed
                .restore(mem_dir.join("reembed-progress.json"));
            manager.subscribe_recovery();
        }
        Ok(manager)
    }
}
