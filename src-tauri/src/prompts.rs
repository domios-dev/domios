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

/// One-time archive line prepended before [`PLAN_RETROSPECTIVE_NUDGE`] for complete active plans.
pub const PLAN_ARCHIVE_SUGGESTION: &str =
    "This plan is complete. Archive it with `codeterm plan archive <plan>` when wrap-up is finished.";

/// Pinned verbatim by the Planner — do not reword.
pub const PLAN_RETROSPECTIVE_NUDGE: &str = "This plan is complete. Before archiving, write its retrospective:\n\
`codeterm plan retrospective <plan> --body \"...\"` — what worked, what failed and why, which plan skills proved out, and which deserve promotion to machine memory (`codeterm mem skill`). Honest beats flattering: the discarded approaches are the most useful part.";

/// Planner role line — attached specialist, not a co-owner.
pub const PEER_PLANNER_CHARTER_LINE: &str = "Planner is the attached planning specialist: \
     draft GOAL, Spec, Plan structure, task briefs, and decision options when the \
     Orchestrator delegates planning work.";

/// Orchestrator authority line when a planner is bound.
pub const PEER_ORCH_DEFERENCE_LINE: &str =
    "The Orchestrator is the single owner of the plan and the execution authority: \
     delegate planning work to the Planner, then accept, revise, or reject it.";

/// Planner-pairing mandate block. `{spawn_cmd}` is replaced at runtime with the pane-spawn command.
pub const PEER_PAIRING_MANDATE_BLOCK: &str = "\
## Planner attachment

This plan requires an attached Planner specialist. Spawn it with `{spawn_cmd}` if it is not live. \
The Orchestrator remains the single authority for plan structure, execution, and acceptance.";

/// Planning-phase delegation line. `{planner}` is filled with the resolved
/// planner label. Shared by next-actions, the plan nudge, and prompts.
pub const PEER_ORCH_DEFER_STRUCTURE_LINE: &str =
    "Planning is not approved yet — delegate GOAL/spec/plan/task-graph drafting to {planner}, \
     then review and approve the structure as Orchestrator.";

/// Fills the `{planner}` placeholder in [`PEER_ORCH_DEFER_STRUCTURE_LINE`].
pub fn orch_defer_structure_line(planner: &str) -> String {
    PEER_ORCH_DEFER_STRUCTURE_LINE.replace("{planner}", planner)
}

/// Hint shown when no planner is bound and no waiver is set.
pub const PEER_WAIVER_HINT: &str = "No Planner specialist is bound. \
     Bind one with `codeterm plan update --planner <PANE_ID>` or waive with \
     `codeterm plan update --planner none --note <reason>`.";

/// Orchestrator's opening greeting on a fresh pair spawn — rendered verbatim
/// as the pane's first assistant message (never model-improvised), so the
/// user always lands on a ready, invitational state. `{planner}` is filled
/// with the Planner's display name.
pub const PEER_ORCH_GREETING: &str =
    "Ready — {planner} is attached as my Planner specialist. Tell me the goal; I will own the plan and delegate drafting to the Planner where useful. Until you state a goal, we hold — no scanning the project for work.";

/// Fills the `{planner}` placeholder in [`PEER_ORCH_GREETING`].
pub fn peer_orch_greeting(planner: &str) -> String {
    PEER_ORCH_GREETING.replace("{planner}", planner)
}

/// Planner's opening greeting — the mirror of [`PEER_ORCH_GREETING`] for the
/// subordinate side, rendered verbatim as the Planner pane's first assistant
/// message so it never boots into silence. Identity framing only; the
/// subordination prose is reused from the shared charter/deference constants.
pub fn peer_planner_greeting(orchestrator_label: &str, orchestrator_pane_id: &str) -> String {
    format!(
        "Planner specialist reporting for this plan — I work under Orchestrator {orchestrator_label} (pane {orchestrator_pane_id}). {}\n{}",
        PEER_PLANNER_CHARTER_LINE, PEER_ORCH_DEFERENCE_LINE,
    )
}

/// Fills the `{spawn_cmd}` placeholder in [`PEER_PAIRING_MANDATE_BLOCK`].
pub fn peer_pairing_mandate_block(spawn_cmd: &str) -> String {
    PEER_PAIRING_MANDATE_BLOCK.replace("{spawn_cmd}", spawn_cmd)
}

/// The mandatory-next-step text delivered on Orchestrator promotion/claim:
/// the filled mandate block plus the single-authority planner attachment lines.
pub fn peer_pairing_mandate_full(spawn_cmd: &str) -> String {
    format!(
        "{}\n\n{PEER_PLANNER_CHARTER_LINE}\n{PEER_ORCH_DEFERENCE_LINE}",
        peer_pairing_mandate_block(spawn_cmd)
    )
}

/// Five emergent plan-skill principles. Single source shared by the orchestrator
/// and planner suffixes (via the `{plan_skills_principles}` placeholder) so the
/// behavior emerges from why+when phrasing rather than a re-typed checklist.
pub const PLAN_SKILLS_PRINCIPLES: &str = "\
Plan skills — the plan's own know-how:
- Learned something reusable in this effort (a test recipe, a seeding script, a gotcha)? Record it: `codeterm plan skill new <slug>`. Improve an existing skill before writing a new one.
- A skill names what is FIXED (the oracle: the command or observation that proves success) and what is MUTABLE. Never edit the oracle to make results look better.
- Prefer skills with a --check command; cite the check when keeping or discarding an approach.
- Simplicity wins: a marginal gain that adds hacky steps is a loss; a deletion that preserves results is a win.
- Every skill edit logs keep/discard/superseded + why. Discarded approaches are data, not embarrassment.";

/// Appended to the worker charter only when the plan already carries >=1 skill.
pub const PLAN_SKILLS_WORKER_LINE: &str = "This plan has skills — procedures earlier agents proved out. Check before inventing: `codeterm plan skill list`. Learned something reusable yourself? `codeterm plan skill new <slug>` — future workers inherit it.";

/// Orchestrator-heartbeat registry line template. `{n}` is the skill count and
/// `{slug1}, {slug2}, …` is the placeholder slug region, both filled at render.
pub const PLAN_SKILLS_REGISTRY_LINE: &str =
    "Plan skills ({n}): {slug1}, {slug2}, … — codeterm plan skill show <slug> | new | edit.";

/// Planner testing methodology, part 1 — oracle-first (inserted after workflow
/// item 5 via the `{planner_testing_methodology}` placeholder).
pub const PLANNER_TESTING_ORACLE_FIRST: &str = "Test plans start from the oracle: name the command or observation that PROVES the task done before choosing the method. Then pick the method by the work: TDD for code paths, fixtures for parsers and formats, live dev walkthrough for UX, seeded eval cases for prompt or behavior changes. If the oracle is expensive (full build, broad suite, e2e), route it through the QA queue instead of per-lane gates. A criterion without an oracle is an opinion.";

/// Planner testing methodology, part 2 — executable test plans (extends the
/// oracle-first paragraph; enforced mechanically by the doctor + QA claim).
pub const PLANNER_TEST_PLAN_EXECUTABLE: &str = "Test plans are EXECUTABLE, not descriptive. A test plan names: (1) the exact command(s) to run, (2) the exact surfaces touched (testids, selectors, files, endpoints), (3) numbered steps with the observable pass condition for each. If the executing worker would have to invent a selector, a step, or an expected value, the plan is incomplete — write the scenario, not the intention. \"Component tests + manual check\" is not a test plan.";

/// Oracle-first + executable paragraphs, joined — fills `{planner_testing_methodology}`.
pub fn planner_testing_methodology() -> String {
    format!("{PLANNER_TESTING_ORACLE_FIRST}\n\n{PLANNER_TEST_PLAN_EXECUTABLE}")
}

/// Orchestrator suffix with the shared plan-skills principles substituted in.
pub fn orchestrator_suffix() -> String {
    ORCHESTRATOR_SUFFIX.replace("{plan_skills_principles}", PLAN_SKILLS_PRINCIPLES)
}

/// Planner suffix with the testing methodology and plan-skills principles substituted in.
pub fn planner_suffix() -> String {
    PLANNER_SUFFIX
        .replace(
            "{planner_testing_methodology}",
            &planner_testing_methodology(),
        )
        .replace("{plan_skills_principles}", PLAN_SKILLS_PRINCIPLES)
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
    fn peer_pairing_mandate_block_fills_spawn_cmd() {
        let filled = peer_pairing_mandate_block("codeterm agent spawn claude --role Planner");
        assert!(filled.contains("codeterm agent spawn claude --role Planner"));
        assert!(!filled.contains("{spawn_cmd}"));
        assert!(PEER_WAIVER_HINT.contains("--planner none --note"));
    }

    #[test]
    fn peer_pairing_mandate_full_fills_spawn_cmd_and_carries_charter() {
        let full = peer_pairing_mandate_full("codeterm agent spawn claude --role Planner");
        assert!(full.contains("codeterm agent spawn claude --role Planner"));
        assert!(!full.contains("{spawn_cmd}"));
        assert!(full.contains(PEER_PLANNER_CHARTER_LINE));
        assert!(full.contains(PEER_ORCH_DEFERENCE_LINE));
        assert!(full.contains("single authority"));
        assert!(!full.contains("Peer contract"));
        assert!(!full.contains("Planner acknowledgment"));
    }

    #[test]
    fn peer_planner_greeting_names_orchestrator_and_reuses_subordination_constants() {
        let g = peer_planner_greeting("Opus Orchestrator", "pane_orch_9");
        assert!(g.contains("pane_orch_9"), "names the orchestrator pane id");
        assert!(
            g.contains("Opus Orchestrator"),
            "names the orchestrator label"
        );
        assert!(
            g.contains("work under Orchestrator"),
            "planner is subordinate"
        );
        assert!(
            g.contains(PEER_PLANNER_CHARTER_LINE),
            "reuses the charter constant"
        );
        assert!(
            g.contains(PEER_ORCH_DEFERENCE_LINE),
            "reuses the deference constant"
        );
    }

    #[test]
    fn orchestrator_suffix_carries_plan_skills_principles() {
        let rendered = orchestrator_suffix();
        assert!(rendered.contains(PLAN_SKILLS_PRINCIPLES));
        assert!(!rendered.contains("{plan_skills_principles}"));
        // Emergent phrasing (why + when), not a bare checklist.
        assert!(PLAN_SKILLS_PRINCIPLES.contains("the oracle"));
        assert!(PLAN_SKILLS_PRINCIPLES.contains("codeterm plan skill new"));
    }

    #[test]
    fn planner_suffix_carries_testing_methodology_and_principles() {
        let rendered = planner_suffix();
        assert!(rendered.contains(PLANNER_TESTING_ORACLE_FIRST));
        assert!(rendered.contains(PLANNER_TEST_PLAN_EXECUTABLE));
        assert!(rendered.contains(PLAN_SKILLS_PRINCIPLES));
        assert!(!rendered.contains("{planner_testing_methodology}"));
        assert!(!rendered.contains("{plan_skills_principles}"));
    }

    #[test]
    fn planner_testing_methodology_is_oracle_first_then_executable() {
        let methodology = planner_testing_methodology();
        let oracle = methodology.find(PLANNER_TESTING_ORACLE_FIRST).unwrap();
        let executable = methodology.find(PLANNER_TEST_PLAN_EXECUTABLE).unwrap();
        assert!(
            oracle < executable,
            "oracle-first paragraph must come first"
        );
    }

    #[test]
    fn plan_skills_worker_and_registry_lines_are_pointers() {
        assert!(PLAN_SKILLS_WORKER_LINE.contains("codeterm plan skill list"));
        assert!(PLAN_SKILLS_WORKER_LINE.contains("codeterm plan skill new"));
        assert!(PLAN_SKILLS_REGISTRY_LINE.contains("{n}"));
        assert!(PLAN_SKILLS_REGISTRY_LINE.contains("codeterm plan skill show <slug> | new | edit."));
    }
}
