//! Agent-facing prompt templates — loaded from `prompts/*.md` at compile time.
//!
//! Edit the .md files in `src-tauri/prompts/` to change instructions.
//! Rust code uses `include_str!()` so changes require recompilation.

/// Appended to Worker tasks — tells them how to report back.
pub const WORKER_SUFFIX: &str = include_str!("../prompts/worker_suffix.md");

/// Appended to Orchestrator tasks — no-sleep rule.
pub const ORCHESTRATOR_SUFFIX: &str = include_str!("../prompts/orchestrator_suffix.md");

/// Appended to cron tasks when creator_pane_id is set.
pub const CRON_REPORT_HINT: &str = include_str!("../prompts/cron_report_hint.md");

/// Appended to every cron task — completion instruction. `{}` is replaced with run_id.
pub const CRON_COMPLETE_SUFFIX: &str = include_str!("../prompts/cron_complete_suffix.md");

/// CodeTerm rule installed to ~/.claude/rules/codeterm.md.
pub const CODETERM_RULE: &str = include_str!("../prompts/codeterm_rule.md");

/// Mem bridge rule payload installed per provider via `install_bridge_rule`.
pub const BRIDGE_MEM: &str = include_str!("../prompts/bridge_mem.md");

/// General Agent CLAUDE.md template (uses format!() placeholders).
pub const GENERAL_AGENT_TEMPLATE: &str = include_str!("../prompts/general_agent.md");

/// Shared CodeTerm CLI reference — role-neutral. Injected into both the GA
/// prompt (via {codeterm_cli} placeholder) and the codeterm skill SKILL.md.
/// Single source of truth so command syntax stays in sync.
pub const CODETERM_CLI: &str = include_str!("../prompts/codeterm_cli.md");

/// Bootstrap instructions template — becomes the agent's personal config file after first run.
pub const BOOTSTRAP_INSTRUCTIONS_TEMPLATE: &str =
    include_str!("../prompts/bootstrap_instructions.md");

/// Wrapper applied to every headless memory worker task. Placeholders: `{intent}`, `{worker_id}`, `{task}`.
pub const WORKER_TASK_PREAMBLE: &str = include_str!("../prompts/worker_task_preamble.md");

/// Auto-extraction prompt for the haiku graph worker. Placeholders: `{id}`, `{title}`, `{body}`.
pub const MEM_EXTRACTION: &str = include_str!("../prompts/mem_extraction.md");

/// Search-result reranking prompt sent directly to the model API. Placeholders: `{query}`, `{items}`.
pub const MEM_RERANK: &str = include_str!("../prompts/mem_rerank.md");

/// REM-phase dreaming prompt — optimization steps for the headless dream worker. No placeholders.
pub const DREAMING_REM: &str = include_str!("../prompts/dreaming_rem.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_do_not_reintroduce_dead_general_agent_spec() {
        const GENERAL_AGENT_DOC: &str = include_str!("../../docs/GENERAL_AGENT.md");

        assert!(!GENERAL_AGENT_DOC.contains("CODETERM-AUTO-START"));
        assert!(!GENERAL_AGENT_DOC.contains("CODETERM-AUTO-END"));
        assert!(!GENERAL_AGENT_DOC.contains("EXTRA_INSTRUCTIONS.md"));
        assert!(!GENERAL_AGENT_DOC.contains("{connected_services}"));
        assert!(!GENERAL_AGENT_DOC.contains("secrets/*.md"));
    }

    #[test]
    fn prompt_docs_match_extraction_contract() {
        const PROMPTS_DOC: &str = include_str!("../../docs/PROMPTS.md");

        assert!(PROMPTS_DOC.contains("50,000 chars"));
        assert!(PROMPTS_DOC.contains("excludes only `MemKind::Summary`"));
        assert!(!PROMPTS_DOC.contains("4000 chars"));
        assert!(!PROMPTS_DOC.contains("items longer than ~80 chars"));
    }

    #[test]
    fn prompt_surface_uses_delay_only_jitter_wording() {
        const MEMORY_DOC: &str = include_str!("../../docs/MEMORY.md");
        const CONFIG_DOC: &str = include_str!("../../docs/CONFIG.md");

        for text in [CODETERM_CLI, MEMORY_DOC, CONFIG_DOC] {
            assert!(!text.contains("±30 min jitter"));
            assert!(!text.contains("±90 min jitter"));
            assert!(!text.contains("5:30–6:30"));
        }
        assert!(CODETERM_CLI.contains("delay-only"));
        assert!(MEMORY_DOC.contains("0..30 min delay-only jitter"));
        assert!(CONFIG_DOC.contains("0..30 min delay-only jitter"));
    }

    #[test]
    fn operational_prompts_keep_clarity_fixes() {
        assert!(WORKER_SUFFIX.contains("STATUS: done | blocked | partial"));
        assert!(WORKER_SUFFIX.contains("BUILD/TYPECHECK/TESTS:"));
        assert!(BOOTSTRAP_INSTRUCTIONS_TEMPLATE.contains("{general_dir}/bootstrap.md"));
        assert!(!BOOTSTRAP_INSTRUCTIONS_TEMPLATE.contains("rm bootstrap.md"));
        assert!(BRIDGE_MEM.contains("provider skill named `codeterm`"));
        assert!(BRIDGE_MEM.contains("mem seed item"));
        assert!(!MEM_EXTRACTION.contains("mem search returns BOTH"));
        assert!(!DREAMING_REM.contains("mem search returns BOTH"));
    }
}
