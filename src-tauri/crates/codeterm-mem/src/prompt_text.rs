//! Mem-owned prompt templates — loaded from `prompts/*.md` at compile time.

pub const WORKER_TASK_PREAMBLE: &str = include_str!("../../../prompts/worker_task_preamble.md");
pub const MEM_EXTRACTION: &str = include_str!("../../../prompts/mem_extraction.md");
pub const MEM_RERANK: &str = include_str!("../../../prompts/mem_rerank.md");
pub const DREAMING_REM: &str = include_str!("../../../prompts/dreaming_rem.md");
pub const BLUEPRINT_ENRICHMENT: &str = include_str!("../../../prompts/blueprint_enrichment.md");
