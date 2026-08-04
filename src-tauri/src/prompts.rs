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
pub const MILESTONE_GATE_LADDER: &str = "Testing ladder — per-task = focused filters only; per-group (milestone) = one full regression gate (cargo suite + ts-unit, serialized in the warm checkout); per-release = the farm matrix across every platform.";

/// Appended to Planner-profile Worker tasks.
pub const PLANNER_SUFFIX: &str = include_str!("../prompts/planner_suffix.md");

/// Shared bounce-unwinding contract exposed through embedded docs and fixtures.
pub const BOUNCE_UNWINDING_CONTRACT: &str = include_str!("../prompts/bounce_unwinding_contract.md");

/// Canonical role_guidance for tasks whose primary deliverable is tests.
pub const TEST_AUTHOR_GUIDANCE: &str = "You are a test-author. docs/contributing/test-authoring.md is your operating charter — read it fully before writing the first test. Non-negotiables: oracle-first, red-green demo in your report, condition-waits never sleeps, agent-readiness gates, bounded everything, smoke-run every harness script against the real CLI.";

/// Appended to Orchestrator tasks — no-sleep rule.
pub const ORCHESTRATOR_SUFFIX: &str = include_str!("../prompts/orchestrator_suffix.md");

/// Task-decomposition contract shared by both planning surfaces. One source so
/// the Planner that cuts the graph and the Orchestrator that judges it agree.
pub const PLANNING_COMPACTION_CONTRACT: &str = include_str!("../prompts/planning_compaction.md");

/// Delivered when an existing agent tab becomes an Orchestrator.
pub const ORCHESTRATOR_ONBOARDING: &str = include_str!("../prompts/orchestrator_onboarding.md");

/// Contextual bootstrap for Team-staffed managers. Rendered by the shared
/// role-identity builder before launch delivery chooses a provider channel.
pub const MANAGER_BOOTSTRAP_TEMPLATE: &str = include_str!("../prompts/manager_bootstrap.md");

/// Appended to cron tasks when creator_tab_id is set.
pub const CRON_REPORT_HINT: &str = include_str!("../prompts/cron_report_hint.md");

/// Appended to every cron task — completion instruction. `{}` is replaced with run_id.
pub const CRON_COMPLETE_SUFFIX: &str = include_str!("../prompts/cron_complete_suffix.md");

/// Appended to a cron task when the job declares `--skill <slug>` and the
/// skill folder was found. Placeholders: `{skill_slug}`, `{skill_path}`.
pub const CRON_SKILL_HINT: &str = include_str!("../prompts/cron_skill_hint.md");

/// Appended to a cron task when the job declares `--skill <slug>` but the
/// skill folder is missing. Placeholders: `{skill_slug}`, `{skill_path}`.
pub const CRON_SKILL_MISSING: &str = include_str!("../prompts/cron_skill_missing.md");

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

/// Compact CodeTerm control contract prepended to non-General agent launches.
pub const AGENT_CODETERM_PRIMER: &str = include_str!("../prompts/agent_codeterm_primer.md");

/// Compact control context placed after a Worker's task-owned identity.
pub const WORKER_CODETERM_CONTEXT: &str = include_str!("../prompts/worker_codeterm_context.md");

/// Shared rich-rendering capability included in every CodeTerm agent contract.
pub const RICH_RENDERING_GUIDANCE: &str = include_str!("../prompts/rich_rendering_guidance.md");

/// AgentPad contract — single source. Every surface that provisions a pad must
/// deliver this; the pad only exists for CodeTerm-spawned agent tabs.
pub const AGENT_PAD_RULE: &str = include_str!("../prompts/agent_pad_rule.md");

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

/// Planner-pairing mandate block. `{spawn_cmd}` is replaced at runtime with the tab-spawn command.
pub const PEER_PAIRING_MANDATE_BLOCK: &str = "\
## Planner attachment

This plan requires an attached Planner specialist. Spawn it with `{spawn_cmd}` if it is not live. \
The Orchestrator remains the single authority for plan structure, execution, and acceptance.";

/// Short user-facing hint shown in the tab for a freshly-promoted Orchestrator
/// (display only — the detailed onboarding is delivered separately as real input).
pub fn orchestrator_display_hint() -> String {
    "Orchestrator for this tab — I own the plan's execution: I dispatch tasks to workers and accept or bounce their results. Tell me the goal; until then, we hold.".to_string()
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
        "You are the Planner specialist for the active Team Plan {plan_id}, working with Orchestrator {orchestrator_label} (tab {orchestrator_tab_id}). {PEER_PLANNER_CHARTER_LINE} {PEER_ORCH_DEFERENCE_LINE}\n\
First read `codeterm docs PLANNER` for your role. The Orchestrator owns and creates the Team Plan and may create its complete structure directly. When planning is delegated, refine that same Team Plan with `codeterm plan goal set`, `codeterm plan section add`, `codeterm plan task add`, `codeterm plan section show`, `codeterm plan get --sections`, and `codeterm plan doctor --kind quality`. No separate Planner-to-Plan binding or General Agent bootstrap is required. The full CLI reference is the `codeterm` skill (`codeterm mem search \"codeterm skill\"` if it is not loaded). If no planning request has been delegated yet, reply that you are ready and WAIT."
    )
}

/// Pair-spawn Planner onboarding before any plan exists. The first
/// Orchestrator goal/plan create binds this tab to the resulting plan.
pub fn planner_onboarding_task_no_plan(
    orchestrator_label: &str,
    orchestrator_tab_id: &str,
) -> String {
    format!(
        "You are the Planner specialist paired with Orchestrator {orchestrator_label} (tab {orchestrator_tab_id}). The Team may not have a Plan yet; the Orchestrator creates it and Team membership makes it visible to you automatically. {PEER_PLANNER_CHARTER_LINE} {PEER_ORCH_DEFERENCE_LINE}\n\
First read `codeterm docs PLANNER` for your role. Do not create a bootstrap Plan, bind yourself to a Plan, or contact the General Agent. When the Orchestrator delegates planning, refine the active Team Plan with the normal plan editing commands. If no planning request has been delegated yet, reply that you are ready and WAIT."
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

pub fn orchestrator_suffix() -> String {
    ORCHESTRATOR_SUFFIX.replace(
        "{planning_compaction_contract}",
        PLANNING_COMPACTION_CONTRACT.trim(),
    )
}

pub fn orchestrator_onboarding(planner_spawn_cmd: &str) -> String {
    ORCHESTRATOR_ONBOARDING
        .replace("{planner_spawn_cmd}", planner_spawn_cmd)
        .replace("{orchestrator_suffix}", &orchestrator_suffix())
}

pub fn planner_suffix() -> String {
    PLANNER_SUFFIX
        .replace("{test_author_guidance}", TEST_AUTHOR_GUIDANCE)
        .replace(
            "{planning_compaction_contract}",
            PLANNING_COMPACTION_CONTRACT.trim(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_suffix_substitutes_the_test_author_guidance_constant() {
        let suffix = planner_suffix();

        assert!(suffix.contains(TEST_AUTHOR_GUIDANCE));
        assert!(!suffix.contains("{test_author_guidance}"));
    }

    /// Worker briefs compose three fragments. Their wording belongs to the .md
    /// files; only the composition is a mechanism worth asserting here.
    #[test]
    fn worker_suffix_composes_its_discipline_fragments() {
        let rendered = worker_suffix();
        assert!(!TESTING_DISCIPLINE.trim().is_empty());
        assert!(!QA_PIPELINE_DISCIPLINE.trim().is_empty());
        assert!(rendered.contains(TESTING_DISCIPLINE.trim()));
        assert!(rendered.contains(QA_PIPELINE_DISCIPLINE.trim()));
        assert!(!rendered.contains('{'), "unsubstituted placeholder remains");
    }

    #[test]
    fn cron_skill_prompts_keep_their_substitution_slots() {
        assert!(CRON_SKILL_HINT.contains("{skill_path}"));
        assert!(CRON_SKILL_HINT.contains("{skill_slug}"));
        assert!(CRON_SKILL_MISSING.contains("{skill_slug}"));
    }

    #[test]
    fn orchestration_guidance_uses_only_complete_document_reads() {
        const ORCHESTRATION_DOC: &str = include_str!("../../docs/ORCHESTRATION.md");
        const PLANNER_DOC: &str = include_str!("../../docs/PLANNER.md");
        const PLAN_EDITING_DOC: &str = include_str!("../../docs/PLAN-EDITING.md");
        let surfaces = [
            ORCHESTRATOR_ONBOARDING,
            PLANNER_SUFFIX,
            CODETERM_CLI,
            ORCHESTRATION_DOC,
            PLANNER_DOC,
            PLAN_EDITING_DOC,
        ];

        for surface in surfaces {
            for fragment in surface.split('`') {
                let command = fragment.trim();
                if command.starts_with("codeterm docs") {
                    for removed in ["--head", "--section", "--full"] {
                        assert!(
                            !command.contains(removed),
                            "removed docs mode remains: {command}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn codeterm_docs_rule_is_single_and_unformatted() {
        const RULE: &str = "Use codeterm docs <NAME>; it returns the complete document. Use codeterm docs or codeterm docs --help to discover available names.";

        assert_eq!(CODETERM_CLI.matches(RULE).count(), 1);
        assert!(!RULE.contains('*'));
        let general = GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI);
        assert_eq!(general.matches(RULE).count(), 1);
        for role_surface in [
            ORCHESTRATOR_ONBOARDING,
            ORCHESTRATOR_SUFFIX,
            PLANNER_SUFFIX,
            WORKER_SUFFIX,
            AGENT_CODETERM_PRIMER,
            WORKER_CODETERM_CONTEXT,
        ] {
            assert!(
                role_surface.matches(RULE).count() <= 1,
                "role instruction duplicates the docs rule"
            );
        }
    }

    #[test]
    fn system_events_are_non_replyable_on_every_agent_surface() {
        for surface in [
            CODETERM_RULE,
            CODETERM_CLI,
            AGENT_CODETERM_PRIMER,
            GENERAL_AGENT_TEMPLATE,
        ] {
            assert!(surface.contains("codeterm:system"));
            assert!(!surface.contains("tab=system"));
        }
    }

    #[test]
    fn agent_pad_rule_has_exactly_one_source() {
        // The fragment is an opaque token here. Asserting its wording would make
        // rewriting the rule fail a test that has nothing to say about wording.
        let rule = AGENT_PAD_RULE.trim();
        assert!(!rule.is_empty());

        for surface in [
            CODETERM_CLI,
            ORCHESTRATOR_ONBOARDING,
            ORCHESTRATOR_SUFFIX,
            PLANNER_SUFFIX,
            WORKER_SUFFIX,
            AGENT_CODETERM_PRIMER,
            WORKER_CODETERM_CONTEXT,
        ] {
            assert!(
                !surface.contains(rule),
                "the AgentPad rule must live only in AGENT_PAD_RULE"
            );
        }
    }

    #[test]
    fn selected_instruction_references_name_whole_documents() {
        const ORCHESTRATION_DOC: &str = include_str!("../../docs/ORCHESTRATION.md");
        const PLANNER_DOC: &str = include_str!("../../docs/PLANNER.md");
        const PLAN_EDITING_DOC: &str = include_str!("../../docs/PLAN-EDITING.md");
        let surfaces = [
            ORCHESTRATOR_ONBOARDING,
            PLANNER_SUFFIX,
            CODETERM_CLI,
            ORCHESTRATION_DOC,
            PLANNER_DOC,
            PLAN_EDITING_DOC,
        ];
        for surface in surfaces {
            for fragment in surface.split('`') {
                let command = fragment.trim();
                if !command.starts_with("codeterm docs ")
                    || command.contains("<TOPIC>")
                    || command.contains("<NAME>")
                    || command == "codeterm docs --help"
                {
                    continue;
                }
                let document = command
                    .strip_prefix("codeterm docs ")
                    .and_then(|rest| rest.split_whitespace().next())
                    .unwrap_or_default();
                assert!(
                    [
                        "ORCHESTRATION",
                        "ORCHESTRATION-OPERATIONS",
                        "PLANNER",
                        "PLAN-EDITING",
                        "LANES",
                        "BOUNCE",
                        "TEST-AUTHORING",
                        "ROUTING",
                        "MESH",
                        "MEMORY",
                        "SECRETS",
                    ]
                    .contains(&document),
                    "instruction reference names an unknown document: {command}"
                );
                assert_eq!(command, format!("codeterm docs {document}"));
            }
        }
    }

    #[test]
    fn peer_pairing_mandate_block_fills_spawn_cmd() {
        let filled = peer_pairing_mandate_block("codeterm agent spawn claude --role Planner");
        assert!(filled.contains("codeterm agent spawn claude --role Planner"));
        assert!(!filled.contains("{spawn_cmd}"));
        assert!(!PEER_WAIVER_HINT.trim().is_empty());
    }

    #[test]
    fn peer_pairing_mandate_full_fills_spawn_cmd_and_carries_charter() {
        let full = peer_pairing_mandate_full("codeterm agent spawn claude --role Planner");
        assert!(full.contains("codeterm agent spawn claude --role Planner"));
        assert!(!full.contains("{spawn_cmd}"));
        assert!(full.contains(PEER_PLANNER_CHARTER_LINE));
        assert!(full.contains(PEER_ORCH_DEFERENCE_LINE));
    }

    #[test]
    fn planner_onboarding_task_carries_role_and_toolset_from_codeterm() {
        let t = planner_onboarding_task("Opus Orchestrator", "tab_orch_9", "pl_abc");
        assert!(t.contains("pl_abc"), "names the plan");
        assert!(t.contains("Opus Orchestrator"), "names the orchestrator");
        assert!(t.contains(PEER_PLANNER_CHARTER_LINE), "reuses the charter");
        assert!(t.contains(PEER_ORCH_DEFERENCE_LINE), "reuses the deference");
    }

    #[test]
    fn planner_onboarding_task_no_plan_carries_pair_hold_state_without_plan_id() {
        let t = planner_onboarding_task_no_plan("Opus Orchestrator", "tab_orch_9");
        assert!(t.contains("Opus Orchestrator"));
        // The real invariant: the no-plan variant must never leak a plan id.
        assert!(!t.contains("pl_"));
    }

    #[test]
    fn orchestrator_suffix_is_bounded_and_fully_rendered() {
        let rendered = orchestrator_suffix();
        assert!(rendered.split_whitespace().count() < 650);
        assert!(rendered.contains('\n'), "suffix rendered to a single line");
        assert!(!rendered.contains('{'), "unsubstituted placeholder remains");
    }

    // Rich-rendering delivery is a composition fact, so it is asserted where the
    // composition lives — service::orchestrator::launch::tests. The two tests that
    // used to sit here were named after the primer and the worker context but only
    // asserted phrases inside RICH_RENDERING_GUIDANCE, so they passed regardless of
    // whether either recipient received it.

    /// One source, every surface that cuts or judges a task graph. The fragment
    /// is an opaque token here; its wording belongs to the .md file.
    #[test]
    fn planning_compaction_contract_reaches_both_planning_surfaces() {
        let contract = PLANNING_COMPACTION_CONTRACT.trim();
        assert!(!contract.is_empty());

        for rendered in [
            orchestrator_suffix(),
            planner_suffix(),
            orchestrator_onboarding("codeterm agent spawn claude --role Planner"),
        ] {
            assert!(
                rendered.contains(contract),
                "planning contract not delivered"
            );
            assert!(!rendered.contains("{planning_compaction_contract}"));
        }
    }

    #[test]
    fn planner_suffix_is_bounded_and_fully_rendered() {
        let rendered = planner_suffix();
        assert!(rendered.split_whitespace().count() < 800);
        assert!(!rendered.contains('{'), "unsubstituted placeholder remains");
    }

    #[test]
    fn milestone_gate_constants_wire_into_orchestration_ladder() {
        const ORCHESTRATION_DOC: &str = include_str!("../../docs/ORCHESTRATION-OPERATIONS.md");

        // Suggestion fills both placeholders and names the record command.
        let msg = milestone_gate_suggestion("phase-1", "Phase 1");
        assert!(msg.contains("group Phase 1 complete"));
        assert!(msg.contains("codeterm plan qa next --group phase-1"));
        assert!(!msg.contains("{label}") && !msg.contains("{key}"));

        // The doc must quote the shared constant verbatim — asserted by constant,
        // not by quoted prose, so rewording the ladder updates one place.
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
        assert!(!REFLECTION_HEARTBEAT_TITLE.trim().is_empty());
    }

    #[test]
    fn general_agent_routes_project_work_to_workspaces() {
        assert!(GENERAL_AGENT_TEMPLATE.contains("TEAM COORDINATION IS MANDATORY"));
        assert!(GENERAL_AGENT_TEMPLATE.contains("codeterm team list"));
        assert!(GENERAL_AGENT_TEMPLATE.contains("--role Orchestrator --workspace"));
        assert!(GENERAL_AGENT_TEMPLATE.contains("MACHINE CONTROL REMAINS YOUR AUTHORITY"));
        assert!(
            GENERAL_AGENT_TEMPLATE.contains("REMEMBER REQUEST PIPELINE IS ORDERED AND IMMEDIATE")
        );
        assert!(!GENERAL_AGENT_TEMPLATE.contains("**"));
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
            "planning_compaction.md",
            "bounce_unwinding_contract.md",
            "orchestrator_suffix.md",
            "orchestrator_onboarding.md",
            "manager_bootstrap.md",
            "team_bootstrap_starter.md",
            "cron_report_hint.md",
            "cron_complete_suffix.md",
            "cron_skill_hint.md",
            "cron_skill_missing.md",
            "codeterm_rule.md",
            "bridge_mem.md",
            "general_agent.md",
            "agent_codeterm_primer.md",
            "agent_pad_rule.md",
            "worker_codeterm_context.md",
            "rich_rendering_guidance.md",
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
