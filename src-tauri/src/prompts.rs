//! Agent-facing prompt templates — loaded from `prompts/*.md` at compile time.
//!
//! Edit the .md files in `src-tauri/prompts/` to change instructions.
//! Rust code uses `include_str!()` so changes require recompilation.

/// Appended to Worker tasks — tells them how to report back.
pub const WORKER_SUFFIX: &str = include_str!("../prompts/worker_suffix.md");

/// Shared worker testing discipline fragment.
pub const TESTING_DISCIPLINE: &str = include_str!("../prompts/testing_discipline.md");

/// Shared QA queue discipline fragment.
pub const QA_PIPELINE_DISCIPLINE: &str = include_str!("../prompts/qa_pipeline_discipline.md");

/// Worker charter heading for loop-specific execution context.
pub const WORKER_LOOP_CONTEXT_TITLE: &str = "Loop context:";

/// Worker charter reminder to report loop iteration evidence explicitly.
pub const WORKER_LOOP_EVIDENCE_RULE: &str =
    "When you report, include this iteration's evidence, whether it made progress, and whether the loop should continue or stop.";

/// Worker charter heading for competitive-attempt isolation.
pub const WORKER_COMPETITIVE_CONTEXT_TITLE: &str = "Competitive context:";

/// Competitive worker-attempt isolation rules shared by charter renderers and tests.
pub const WORKER_COMPETITIVE_ISOLATION_RULES: [&str; 3] = [
    "Work only in your own assigned lane, branch, and worktree.",
    "Do not inspect, tail, or reuse another live attempt's pane, worktree, or branch.",
    "The Orchestrator compares the attempts and decides the winner or next follow-up.",
];

/// Orchestrator heartbeat heading for live concurrent attempts.
pub const HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE: &str = "Competitive attempts:";

pub fn worker_suffix() -> String {
    format!("{TESTING_DISCIPLINE}\n{QA_PIPELINE_DISCIPLINE}\n{WORKER_SUFFIX}")
}

/// Appended to Planner-profile Worker tasks.
pub const PLANNER_SUFFIX: &str = include_str!("../prompts/planner_suffix.md");

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

/// Wrapper for plugin-spawned agents (chatBackend NPCs, etc). Unlike memory
/// workers they own their whole prompt and act through plugin verbs, so they
/// must NOT inherit the graph-worker identity. Placeholder: `{task}`.
pub const PLUGIN_AGENT_TASK_PREAMBLE: &str =
    include_str!("../prompts/plugin_agent_task_preamble.md");

/// Auto-extraction prompt for the haiku graph worker. Placeholders: `{id}`, `{title}`, `{body}`.
pub const MEM_EXTRACTION: &str = include_str!("../prompts/mem_extraction.md");

/// Search-result reranking prompt sent directly to the model API. Placeholders: `{query}`, `{items}`.
pub const MEM_RERANK: &str = include_str!("../prompts/mem_rerank.md");

/// REM-phase dreaming prompt — optimization steps for the headless dream worker. No placeholders.
pub const DREAMING_REM: &str = include_str!("../prompts/dreaming_rem.md");

/// Planner owns GOAL, Spec, and Plan structure — injected into orchestrator charters.
pub const PEER_PLANNER_CHARTER_LINE: &str = "Planner owns GOAL, Spec, and Plan structure. \
     The orchestrator defers structural decisions (sections, tasks, sequencing) \
     to the Planner peer.";

/// Orchestrator deference rule when a planner peer is bound.
pub const PEER_ORCH_DEFERENCE_LINE: &str =
    "As orchestrator, defer to the Planner peer for structural decisions. \
     Do not add sections, reorganize tasks, or change the plan skeleton \
     without Planner acknowledgment.";

/// Peer-pairing mandate block. `{spawn_cmd}` is replaced at runtime with the pane-spawn command.
pub const PEER_PAIRING_MANDATE_BLOCK: &str = "\
## Peer contract

This plan has a Planner peer. Spawn the Planner with `{spawn_cmd}` if it is not live. \
The orchestrator is the execution authority; the Planner is the structural authority. \
Structural changes require Planner acknowledgment.";

/// Hint shown when no planner is bound and no waiver is set.
pub const PEER_WAIVER_HINT: &str = "No planner peer is bound. \
     Bind one with `codeterm plan update --planner <PANE_ID>` or waive with \
     `codeterm plan update --planner none --note <reason>`.";

/// Fills the `{spawn_cmd}` placeholder in [`PEER_PAIRING_MANDATE_BLOCK`].
pub fn peer_pairing_mandate_block(spawn_cmd: &str) -> String {
    PEER_PAIRING_MANDATE_BLOCK.replace("{spawn_cmd}", spawn_cmd)
}

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
        assert!(!TESTING_DISCIPLINE.trim().is_empty());
        assert!(worker_suffix().contains(TESTING_DISCIPLINE.trim()));
        assert!(worker_suffix().contains(QA_PIPELINE_DISCIPLINE.trim()));
        assert!(!WORKER_LOOP_EVIDENCE_RULE.trim().is_empty());
        assert!(WORKER_COMPETITIVE_ISOLATION_RULES
            .iter()
            .all(|rule| !rule.trim().is_empty()));
        assert!(!HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE.trim().is_empty());
        assert!(BOOTSTRAP_INSTRUCTIONS_TEMPLATE.contains("{general_dir}/bootstrap.md"));
        assert!(!BOOTSTRAP_INSTRUCTIONS_TEMPLATE.contains("rm bootstrap.md"));
        assert!(BRIDGE_MEM.contains("provider skill named `codeterm`"));
        assert!(BRIDGE_MEM.contains("mem seed item"));
        assert!(!MEM_EXTRACTION.contains("mem search returns BOTH"));
        assert!(!DREAMING_REM.contains("mem search returns BOTH"));
    }

    #[test]
    fn orchestration_prompts_separate_capability_from_mode() {
        assert!(GENERAL_AGENT_TEMPLATE.contains("Orchestrator is an explicit mode"));
        assert!(GENERAL_AGENT_TEMPLATE.contains("Until then, stay the machine-level General Agent"));
        assert!(CODETERM_CLI.contains("You are not automatically an Orchestrator"));
        assert!(CODETERM_CLI.contains("Make Orchestrator"));
        assert!(CODETERM_CLI.contains("codeterm docs ORCHESTRATION"));
        assert!(ORCHESTRATOR_SUFFIX.contains("CodeTerm planner is the source of truth"));
    }

    #[test]
    fn planner_prompt_and_docs_keep_planner_contract() {
        const ORCHESTRATION_DOC: &str = include_str!("../../docs/ORCHESTRATION.md");

        assert!(PLANNER_SUFFIX.contains("CodeTerm planner is the source of truth"));
        assert!(PLANNER_SUFFIX.contains("Do not create standalone markdown implementation plans"));
        assert!(PLANNER_SUFFIX.contains("Discovery/research is an intermediate wave"));
        assert!(PLANNER_SUFFIX.contains("autonomously build the remaining implementation"));
        assert!(PLANNER_SUFFIX.contains("role_guidance"));
        assert!(PLANNER_SUFFIX.contains("Do not invent a third structural AgentRole"));
        assert!(ORCHESTRATION_DOC.contains("--role Planner"));
        assert!(ORCHESTRATION_DOC.contains("--model fable"));
        assert!(ORCHESTRATION_DOC.contains("fall back to `opus`"));
        assert!(ORCHESTRATION_DOC.contains("Planner-authored `role_guidance`"));
        assert!(ORCHESTRATION_DOC.contains("CodeTerm planner"));
    }

    #[test]
    fn peer_pairing_mandate_block_fills_spawn_cmd_and_carries_charter() {
        let filled = peer_pairing_mandate_block("codeterm agent spawn claude --role Planner");
        assert!(filled.contains("codeterm agent spawn claude --role Planner"));
        assert!(!filled.contains("{spawn_cmd}"));
        assert!(filled.contains(PEER_PLANNER_CHARTER_LINE));
        assert!(filled.contains(PEER_ORCH_DEFERENCE_LINE));
        assert!(PEER_WAIVER_HINT.contains("--planner none --note"));
    }
}
