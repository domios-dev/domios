//! App shim — re-exports the `codeterm-mem` domain crate plus app wiring.

#[cfg(feature = "embeddings")]
pub use codeterm_mem::compose_embed_text;
pub mod dream_runs;

#[cfg(test)]
mod dream_status_tests;

pub use codeterm_mem::dream_status;
pub use codeterm_mem::{
    activity, body_is_embeddable, decision, docs_source, embed_queue, embeddings, file_sync, graph,
    graph_layout, graph_repair, ids, layers, learnings, ports, prompt_text, rerank, search, seed,
    store, supervisor, worker_pool, Entity, MemItem, MemKind, MemManager, MemScope, MemStats,
    NewMemItem, Relation, SearchHit, SearchQuery, WakeUpOptions, MIN_EMBED_BODY_CHARS,
};

pub mod prompts {
    pub use codeterm_mem::prompt_text::*;
}

pub mod auto_extract {
    pub use crate::mem_host::{run_extract_pending, spawn_extraction_worker};
    pub use codeterm_mem::auto_extract::*;
}

pub mod dreaming {
    pub use crate::mem_host::spawn_rem_worker;
    pub use codeterm_mem::dreaming::*;
}

pub mod workers {
    pub use codeterm_mem::workers::*;
    pub fn spawn_headless(
        svc: &std::sync::Arc<crate::service::ServiceContext>,
        intent: String,
        task_prompt: String,
        spawned_by: String,
        agent_provider: String,
        model: String,
    ) -> Result<String, String> {
        crate::mem_host::spawn_headless(svc, intent, task_prompt, spawned_by, agent_provider, model)
    }
}
