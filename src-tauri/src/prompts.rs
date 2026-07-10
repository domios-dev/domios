//! Agent-facing prompt templates — loaded from `prompts/*.md` at compile time.
//!
//! Edit the .md files in `src-tauri/prompts/` to change instructions.
//! Rust code uses `include_str!()` so changes require recompilation.

pub use crate::service::plan::prompts_plan::{
    milestone_gate_suggestion, orch_defer_structure_line, reflection_block, worker_suffix,
    HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE, MILESTONE_GATE_HEARTBEAT_TITLE,
    MILESTONE_GATE_SUGGESTION, PEER_ORCH_DEFER_STRUCTURE_LINE, PEER_PLANNER_CHARTER_LINE,
    PEER_WAIVER_HINT, PLAN_ARCHIVE_SUGGESTION, PLAN_RETROSPECTIVE_NUDGE, PLAN_SKILLS_PRINCIPLES,
    PLAN_SKILLS_REGISTRY_LINE, PLAN_SKILLS_WORKER_LINE, QA_PIPELINE_DISCIPLINE, REFLECTION_BLOCK,
    REFLECTION_HEARTBEAT_TITLE, REGRESSION_MILESTONE_KIND, TESTING_DISCIPLINE,
    WORKER_COMPETITIVE_CONTEXT_TITLE, WORKER_COMPETITIVE_ISOLATION_RULES,
    WORKER_LOOP_CONTEXT_TITLE, WORKER_LOOP_EVIDENCE_RULE, WORKER_SUFFIX,
};

/// The three-tier testing ladder. Single source shared by next-actions/heartbeat prose
/// and asserted verbatim in the ORCHESTRATION doc so the wording never drifts.
pub const MILESTONE_GATE_LADDER: &str = "Testing ladder — per-task = focused filters only; per-group (milestone) = one full regression gate (cargo suite + ts-unit + playwright golden, serialized in the warm checkout); per-release = the farm matrix across every platform.";

/// Appended to Planner-profile Worker tasks.
pub const PLANNER_SUFFIX: &str = include_str!("../prompts/planner_suffix.md");

/// Shared bounce-unwinding contract rendered into Planner and Orchestrator suffixes.
pub const BOUNCE_UNWINDING_CONTRACT: &str = include_str!("../prompts/bounce_unwinding_contract.md");

/// Canonical role_guidance for tasks whose primary deliverable is tests.
pub const TEST_AUTHOR_GUIDANCE: &str = "You are a test-author. docs/contributing/test-authoring.md is your operating charter — read it fully before writing the first test. Non-negotiables: oracle-first, red-green demo in your report, condition-waits never sleeps, agent-readiness gates, bounded everything, smoke-run every harness script against the real CLI.";

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
pub use crate::mem::prompts::WORKER_TASK_PREAMBLE;

/// Wrapper for plugin-spawned agents (chatBackend NPCs, etc). Unlike memory
/// workers they own their whole prompt and act through plugin verbs, so they
/// must NOT inherit the graph-worker identity. Placeholder: `{task}`.
pub const PLUGIN_AGENT_TASK_PREAMBLE: &str =
    include_str!("../prompts/plugin_agent_task_preamble.md");

/// Auto-extraction prompt for the haiku graph worker. Placeholders: `{id}`, `{title}`, `{body}`.
pub use crate::mem::prompts::MEM_EXTRACTION;

/// Search-result reranking prompt sent directly to the model API. Placeholders: `{query}`, `{items}`.
pub use crate::mem::prompts::MEM_RERANK;

/// REM-phase dreaming prompt — optimization steps for the headless dream worker. No placeholders.
pub use crate::mem::prompts::DREAMING_REM;

/// Orchestrator authority line when a planner is bound.
pub const PEER_ORCH_DEFERENCE_LINE: &str =
    "The Orchestrator is the single owner of the plan and the execution authority: \
     delegate planning work to the Planner, then accept, revise, or reject it.";

/// Planner-pairing mandate block. `{spawn_cmd}` is replaced at runtime with the pane-spawn command.
pub const PEER_PAIRING_MANDATE_BLOCK: &str = "\
## Planner attachment

This plan requires an attached Planner specialist. Spawn it with `{spawn_cmd}` if it is not live. \
The Orchestrator remains the single authority for plan structure, execution, and acceptance.";

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

/// Short user-facing hint shown in the tab for a freshly-promoted Orchestrator
/// (display only — the detailed onboarding is delivered separately as real input).
pub fn orchestrator_display_hint() -> String {
    "Orchestrator for this tab — I own the plan's execution: I dispatch tasks to workers and accept or bounce their results. Tell me the goal; until then, we hold.".to_string()
}

/// Planner's opening greeting — the mirror of [`PEER_ORCH_GREETING`] for the
/// subordinate side, rendered verbatim as the Planner pane's first assistant
/// message so it never boots into silence. Identity framing only; the
/// subordination prose is reused from the shared charter/deference constants.
pub fn peer_planner_greeting(orchestrator_label: &str, _orchestrator_pane_id: &str) -> String {
    format!(
        "Planner specialist paired with Orchestrator {orchestrator_label} — I draft the GOAL, spec, and plan structure and cut the tasks when a plan exists. Ready; I will wait for the goal before doing anything."
    )
}

/// The Planner's detailed onboarding, delivered as its own real launch input by
/// CodeTerm (never relayed by the Orchestrator). Second-person; carries the
/// role, toolset, docs pointer, persona-field mandate, and the wait-if-empty rule.
pub fn planner_onboarding_task(
    orchestrator_label: &str,
    orchestrator_tab_id: &str,
    plan_id: &str,
) -> String {
    format!(
        "You are the Planner specialist for plan {plan_id}, working under Orchestrator {orchestrator_label} (tab {orchestrator_tab_id}). {PEER_PLANNER_CHARTER_LINE} {PEER_ORCH_DEFERENCE_LINE}\n\
First read `codeterm docs PLANNER` for your role. Planner can EXECUTE: `codeterm plan goal set`, `codeterm plan section add`, `codeterm plan task add`, `codeterm plan section show`, `codeterm plan get --sections`, and `codeterm plan doctor --kind quality`. YOU cut the tasks — each carries a brief, acceptance criteria, an executable test plan, a correctness vector, and the persona/role fields (`--personality`, `--executor-role`, `--role-guidance`); the Orchestrator dispatches and accepts them, it does not create them. The full CLI reference is the `codeterm` skill (`codeterm mem search \"codeterm skill\"` if it is not loaded). A fresh pairing starts empty: if no goal or planning request has been delegated yet, reply that you are ready and WAIT — do not explore the project or read other plans."
    )
}

/// Pair-spawn Planner onboarding before any plan exists. The first
/// Orchestrator goal/plan create binds this pane to the resulting plan.
pub fn planner_onboarding_task_no_plan(
    orchestrator_label: &str,
    orchestrator_tab_id: &str,
) -> String {
    format!(
        "You are the Planner specialist paired with Orchestrator {orchestrator_label} (tab {orchestrator_tab_id}). No plan exists yet; the Orchestrator's first goal/plan create will bind you to that plan. {PEER_PLANNER_CHARTER_LINE} {PEER_ORCH_DEFERENCE_LINE}\n\
First read `codeterm docs PLANNER` for your role. Planner can EXECUTE after a plan exists: `codeterm plan goal set`, `codeterm plan section add`, `codeterm plan task add`, `codeterm plan section show`, `codeterm plan get --sections`, and `codeterm plan doctor --kind quality`. YOU cut the tasks after the Orchestrator delegates the goal — each carries a brief, acceptance criteria, an executable test plan, a correctness vector, and the persona/role fields (`--personality`, `--executor-role`, `--role-guidance`); the Orchestrator dispatches and accepts them, it does not create them. A fresh pairing starts empty: if no goal or planning request has been delegated yet, reply that you are ready and WAIT — do not explore the project or read other plans."
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
    ORCHESTRATOR_SUFFIX
        .replace("{bounce_unwinding_contract}", BOUNCE_UNWINDING_CONTRACT)
        .replace("{plan_skills_principles}", PLAN_SKILLS_PRINCIPLES)
}

/// Planner suffix with the testing methodology and plan-skills principles substituted in.
pub fn planner_suffix() -> String {
    PLANNER_SUFFIX
        .replace("{bounce_unwinding_contract}", BOUNCE_UNWINDING_CONTRACT)
        .replace(
            "{planner_testing_methodology}",
            &planner_testing_methodology(),
        )
        .replace("{test_author_guidance}", TEST_AUTHOR_GUIDANCE)
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
    fn planner_suffix_uses_canonical_test_author_guidance() {
        let suffix = planner_suffix();

        assert!(suffix.contains(
            "A task whose primary deliverable is tests (specs, drills, eval harnesses, baselines)"
        ));
        assert!(suffix.contains("`executor_role=test-author`"));
        assert!(suffix.contains(TEST_AUTHOR_GUIDANCE));
        assert!(!suffix.contains("{test_author_guidance}"));
    }

    #[test]
    fn operational_prompts_keep_clarity_fixes() {
        assert!(WORKER_SUFFIX.contains("STATUS: done | blocked | partial"));
        assert!(WORKER_SUFFIX.contains("TESTS RUN:"));
        assert!(WORKER_SUFFIX.contains("GATES REQUESTED:"));
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
    fn general_agent_requires_mem_skill_search_before_declaring_missing() {
        assert!(GENERAL_AGENT_TEMPLATE.contains("Skill lookup"));
        assert!(GENERAL_AGENT_TEMPLATE.contains("pre-injected provider skill list"));
        assert!(GENERAL_AGENT_TEMPLATE.contains("codeterm mem search \"<capability> skill\""));
        assert!(GENERAL_AGENT_TEMPLATE.contains("before declaring the skill missing"));
        assert!(BRIDGE_MEM.contains("Skill lookup before \"missing\""));
        assert!(BRIDGE_MEM.contains("codeterm mem search \"<capability> skill\""));
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
    fn peer_planner_greeting_is_a_short_user_hint() {
        let g = peer_planner_greeting("Opus Orchestrator", "pane_orch_9");
        assert!(g.contains("Opus Orchestrator"), "names the orchestrator");
        assert!(g.contains("Planner"), "identifies the role");
        assert!(g.contains("wait for the goal"), "conveys the hold state");
        // Display hint only — no CLI commands, no docs/toolset detail.
        assert!(!g.contains("codeterm "), "greeting carries no commands");
        assert!(!g.contains("--personality"), "greeting carries no flags");
    }

    #[test]
    fn planner_onboarding_task_carries_role_and_toolset_from_codeterm() {
        let t = planner_onboarding_task("Opus Orchestrator", "tab_orch_9", "pl_abc");
        assert!(t.contains("pl_abc"), "names the plan");
        assert!(t.contains("Opus Orchestrator"), "names the orchestrator");
        assert!(
            t.contains("You are the Planner"),
            "second-person onboarding"
        );
        assert!(t.contains("codeterm docs PLANNER"), "points at the doc");
        assert!(
            t.contains("Planner can EXECUTE:"),
            "lists concrete executable actions"
        );
        assert!(
            t.contains("codeterm plan task add"),
            "names concrete planner action"
        );
        assert!(t.contains("--personality"), "mandates persona fields");
        assert!(t.contains(PEER_PLANNER_CHARTER_LINE), "reuses the charter");
        assert!(t.contains(PEER_ORCH_DEFERENCE_LINE), "reuses the deference");
    }

    #[test]
    fn planner_onboarding_task_no_plan_carries_pair_hold_state_without_plan_id() {
        let t = planner_onboarding_task_no_plan("Opus Orchestrator", "tab_orch_9");
        assert!(t.contains("No plan exists yet"));
        assert!(t.contains("first goal/plan create"));
        assert!(t.contains("Opus Orchestrator"));
        assert!(!t.contains("pl_"));
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
    fn milestone_gate_constants_wire_into_orchestration_ladder() {
        const ORCHESTRATION_DOC: &str = include_str!("../../docs/ORCHESTRATION.md");

        // Suggestion fills both placeholders and names the record command.
        let msg = milestone_gate_suggestion("phase-1", "Phase 1");
        assert!(msg.contains("group Phase 1 complete"));
        assert!(msg.contains("codeterm plan qa next --group phase-1"));
        assert!(!msg.contains("{label}") && !msg.contains("{key}"));

        // Three-tier ladder wording lives in a shared constant AND the doc verbatim.
        assert!(MILESTONE_GATE_LADDER.contains("per-task = focused filters only"));
        assert!(MILESTONE_GATE_LADDER.contains("per-group (milestone) = one full regression gate"));
        assert!(MILESTONE_GATE_LADDER.contains("per-release = the farm matrix"));
        assert!(ORCHESTRATION_DOC.contains(MILESTONE_GATE_LADDER));
        assert_eq!(REGRESSION_MILESTONE_KIND, "regression");
        assert!(!MILESTONE_GATE_HEARTBEAT_TITLE.trim().is_empty());
    }

    #[test]
    fn reflection_block_fills_counts_and_routes_to_existing_primitives() {
        use crate::service::plan::reflection::ReflectionWindow;
        let filled = reflection_block(
            "group Wave 1 closed",
            &ReflectionWindow {
                tasks_closed: 3,
                bounces: 1,
                gate_leaks: 2,
            },
        );
        assert!(filled.contains("group Wave 1 closed"));
        assert!(filled.contains("tasks closed: 3"));
        assert!(filled.contains("bounces: 1"));
        assert!(filled.contains("gate leaks: 2"));
        assert!(!filled.contains("{trigger}") && !filled.contains("{closed}"));
        // Routes to existing homes; invents no new store.
        assert!(filled.contains("codeterm orchestrator learn"));
        assert!(filled.contains("codeterm plan skill new"));
        assert!(filled.contains("Planner"));
        assert!(!REFLECTION_HEARTBEAT_TITLE.trim().is_empty());
    }

    #[test]
    fn orchestrator_suffix_pins_scratch_and_open_risk_lines_and_tripwire() {
        assert!(ORCHESTRATOR_SUFFIX.contains(
            "Inspecting or prototyping? Scratch worktree — the integration checkout stays clean; a dirty checkout is a gate hazard."
        ));
        assert!(ORCHESTRATOR_SUFFIX.contains(
            "An accepted lane with deferred/skipped evidence is OPEN RISK: track it as such until the evidence lands or the user explicitly accepts the substitute."
        ));
        assert!(ORCHESTRATOR_SUFFIX
            .contains("Editing a non-scratch integration file while orchestrating = STOP"));
        assert!(orchestrator_suffix().contains("codeterm orchestrator learn"));
    }

    #[test]
    fn planner_suffix_has_reflection_symmetry_line() {
        assert!(PLANNER_SUFFIX.contains("At reflection points"));
        assert!(PLANNER_SUFFIX.contains("task or test-plan quality"));
    }

    #[test]
    fn role_suffixes_render_one_shared_bounce_contract() {
        assert_eq!(
            PLANNER_SUFFIX
                .matches("{bounce_unwinding_contract}")
                .count(),
            1
        );
        assert_eq!(
            ORCHESTRATOR_SUFFIX
                .matches("{bounce_unwinding_contract}")
                .count(),
            1
        );
        assert!(!PLANNER_SUFFIX.contains("Baseline target:"));
        assert!(!ORCHESTRATOR_SUFFIX.contains("Baseline target:"));

        for rendered in [planner_suffix(), orchestrator_suffix()] {
            assert!(!rendered.contains("{bounce_unwinding_contract}"));
            assert_eq!(rendered.matches("Baseline target:").count(), 1);
            assert!(rendered.contains(BOUNCE_UNWINDING_CONTRACT.trim()));
        }
    }

    #[test]
    fn general_agent_routes_project_work_to_workspaces() {
        assert!(GENERAL_AGENT_TEMPLATE.contains("Machine scope, not project scope"));
        assert!(GENERAL_AGENT_TEMPLATE.contains("spawn or reuse an orchestrator there"));
    }

    #[test]
    fn orchestrator_suffix_has_cross_machine_etiquette() {
        assert!(ORCHESTRATOR_SUFFIX.contains("Cross-machine work:"));
        assert!(ORCHESTRATOR_SUFFIX.contains("redirected into a workspace, not executed"));
    }

    #[test]
    fn orchestrator_suffix_has_blocked_triage_rule() {
        assert!(ORCHESTRATOR_SUFFIX.contains("a claim, not a parking spot"));
        assert!(ORCHESTRATOR_SUFFIX.contains("record the unblock trigger"));
    }

    #[test]
    fn orchestrator_suffix_routes_uncertainty_to_planner() {
        assert!(ORCHESTRATOR_SUFFIX.contains("ask your Planner before guessing"));
    }

    #[test]
    fn planner_suffix_answers_with_decisions() {
        assert!(PLANNER_SUFFIX.contains("decisions and rationale, not options"));
    }

    #[test]
    fn reflection_block_runs_velocity_and_cost_sweep() {
        assert!(REFLECTION_BLOCK.contains("VELOCITY AND COST sweep"));
        assert!(REFLECTION_BLOCK.contains("no longer earn their priority"));
        assert!(REFLECTION_BLOCK.contains("cheaper model"));
    }

    #[test]
    fn retrospective_nudge_graduates_skills_and_scripts_via_reviewed_lane() {
        assert!(PLAN_RETROSPECTIVE_NUDGE.contains("codeterm mem skill"));
        assert!(PLAN_RETROSPECTIVE_NUDGE.contains(".agents/skills/"));
        assert!(PLAN_RETROSPECTIVE_NUDGE.contains("scripts/"));
        assert!(PLAN_RETROSPECTIVE_NUDGE.contains("reviewed lane"));
    }

    #[test]
    fn plan_skills_worker_and_registry_lines_are_pointers() {
        assert!(PLAN_SKILLS_WORKER_LINE.contains("codeterm plan skill list"));
        assert!(PLAN_SKILLS_WORKER_LINE.contains("codeterm plan skill new"));
        assert!(PLAN_SKILLS_REGISTRY_LINE.contains("{n}"));
        assert!(PLAN_SKILLS_REGISTRY_LINE.contains("codeterm plan skill show <slug> | new | edit."));
    }

    #[test]
    fn every_prompt_md_has_a_loader() {
        // All top-level .md files in prompts/ that have an include_str!() loader.
        // Any file in the directory that is NOT in this set is an orphan — it is
        // compiled into the binary by nobody and misleads future editors.
        let loaded: std::collections::HashSet<&str> = [
            "worker_suffix.md",
            "testing_discipline.md",
            "qa_pipeline_discipline.md",
            "planner_suffix.md",
            "bounce_unwinding_contract.md",
            "orchestrator_suffix.md",
            "cron_report_hint.md",
            "cron_complete_suffix.md",
            "codeterm_rule.md",
            "bridge_mem.md",
            "general_agent.md",
            "codeterm_cli.md",
            "bootstrap_instructions.md",
            "worker_task_preamble.md",
            "plugin_agent_task_preamble.md",
            "mem_extraction.md",
            "mem_rerank.md",
            "dreaming_rem.md",
        ]
        .into_iter()
        .collect();

        let prompts_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prompts");
        let orphans: Vec<String> = std::fs::read_dir(&prompts_dir)
            .expect("prompts/ must be readable")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().is_file() && e.path().extension().and_then(|x| x.to_str()) == Some("md")
            })
            .filter(|e| !loaded.contains(e.file_name().to_str().unwrap_or("")))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();

        assert!(
            orphans.is_empty(),
            "orphaned prompt files with no include_str!() loader: {:?}",
            orphans
        );
    }
}
