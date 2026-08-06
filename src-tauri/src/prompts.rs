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

pub const PLANNER_DOC: &str = include_str!("../../docs/PLANNER.md");

/// Shared bounce-unwinding contract exposed through embedded docs and fixtures.
pub const BOUNCE_UNWINDING_CONTRACT: &str = include_str!("../prompts/bounce_unwinding_contract.md");

/// Canonical role_guidance for tasks whose primary deliverable is tests.
pub const TEST_AUTHOR_GUIDANCE: &str = "You are a test-author. docs/contributing/test-authoring.md is your operating charter — read it fully before writing the first test. Non-negotiables: oracle-first, red-green demo in your report, condition-waits never sleeps, agent-readiness gates, bounded everything, smoke-run every harness script against the real CLI.";

/// Appended to Orchestrator tasks — no-sleep rule.
pub const ORCHESTRATOR_SUFFIX: &str = include_str!("../prompts/orchestrator_suffix.md");

pub const ORCHESTRATION_DOC: &str = include_str!("../../docs/ORCHESTRATION.md");

/// Task-decomposition contract shared by both planning surfaces. One source so
/// the Planner that cuts the graph and the Orchestrator that judges it agree.
pub const PLANNING_COMPACTION_CONTRACT: &str = include_str!("../prompts/planning_compaction.md");

/// Team-bound creation contract shared by the two roles that can ask for a new
/// agent, shell, or schedule. One source so neither can drift into a loose one.
pub const TEAM_CREATION_CONTRACT: &str = include_str!("../prompts/team_creation_contract.md");

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
    let role_prompt = ORCHESTRATOR_SUFFIX
        .replace(
            "{planning_compaction_contract}",
            PLANNING_COMPACTION_CONTRACT.trim(),
        )
        .replace("{team_creation_contract}", TEAM_CREATION_CONTRACT.trim());
    format!(
        "{}\n\n## Canonical Orchestrator manual\n\n{}",
        role_prompt.trim(),
        ORCHESTRATION_DOC.trim()
    )
}

/// Stable marker the plan-absent first-message emits so tests assert the
/// create-first path by constant, never by prose.
pub const ORCH_ONBOARD_NO_PLAN_MARKER: &str =
    "No Team Plan exists yet — create and bind one before spawning a Planner.";
/// Stable token for the plan-absent create+bind command.
pub const ORCH_ONBOARD_CREATE_STEP: &str = "codeterm plan create --title";

fn orchestrator_infra_brief_with_plan(tab_id: &str, team_id: &str, plan_id: &str) -> String {
    let spawn_cmd =
        crate::service::plan::peer_mandate::planner_spawn_cmd_hint(Some(tab_id), plan_id);
    format!(
        "Your tab is {tab_id}. Your team is {team_id}. Your active Team Plan is {plan_id}.\n\n\
         Begin with narrow state: `codeterm orchestrator status --plan {plan_id}`.\n\n\
         Spawn your Planner directly (never through the General Agent) with exactly:\n{spawn_cmd}"
    )
}

fn orchestrator_infra_brief_no_plan(tab_id: &str, team_id: &str) -> String {
    format!(
        "Your tab is {tab_id}. Your team is {team_id}. {ORCH_ONBOARD_NO_PLAN_MARKER}\n\n\
         Step 1 — create and bind the Team Plan to this team:\n{ORCH_ONBOARD_CREATE_STEP} \"<your goal>\" --team {team_id}\n\n\
         Step 2 — once step 1 prints the new plan id, spawn your Planner bound to it and to this tab (never through the General Agent):\ncodeterm agent spawn <provider> [--model <model>] --parent {tab_id} --role Planner --plan <the plan id printed in step 1>"
    )
}

/// Concrete, real-valued first message delivered right after an Orchestrator
/// launches. Methodology lives in the system prompt (`orchestrator_suffix`) and
/// is intentionally NOT duplicated here — this is only infrastructure with real
/// tab/team/plan ids, or an explicit plan-not-yet-created create+bind path.
pub fn orchestrator_first_message(tab_id: &str, team_id: &str, plan_id: Option<&str>) -> String {
    let brief = match plan_id {
        Some(plan_id) => orchestrator_infra_brief_with_plan(tab_id, team_id, plan_id),
        None => orchestrator_infra_brief_no_plan(tab_id, team_id),
    };
    ORCHESTRATOR_ONBOARDING.replace("{infra_brief}", &brief)
}

pub fn planner_suffix() -> String {
    let role_prompt = PLANNER_SUFFIX
        .replace("{test_author_guidance}", TEST_AUTHOR_GUIDANCE)
        .replace(
            "{planning_compaction_contract}",
            PLANNING_COMPACTION_CONTRACT.trim(),
        )
        .replace("{team_creation_contract}", TEAM_CREATION_CONTRACT.trim());
    format!(
        "{}\n\n## Canonical Planner manual\n\n{}",
        role_prompt.trim(),
        PLANNER_DOC.trim()
    )
}

/// Upper bound for a composed role prompt. Not a style preference: every byte
/// here is spent on every launch of that role, so growth needs a decision.
pub const COMPOSED_ROLE_PROMPT_MAX_BYTES: usize = 32 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed clap manifest is the authority on which commands exist.
    /// Generated artifact — read here, never written.
    const CLI_MANIFEST: &str = include_str!("../../docs/cli-manifest.json");

    /// Every prompt fragment that teaches CLI grammar, with the file it is
    /// loaded from so a failure names something a human can open.
    fn cli_teaching_fragments() -> [(&'static str, &'static str); 7] {
        [
            ("general_agent.md", GENERAL_AGENT_TEMPLATE),
            ("codeterm_rule.md", CODETERM_RULE),
            ("codeterm_cli.md", CODETERM_CLI),
            ("planner_suffix.md", PLANNER_SUFFIX),
            ("orchestrator_suffix.md", ORCHESTRATOR_SUFFIX),
            ("worker_suffix.md", WORKER_SUFFIX),
            ("team_creation_contract.md", TEAM_CREATION_CONTRACT),
        ]
    }

    fn subcommand<'a>(node: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
        node["subcommands"].as_array()?.iter().find(|child| {
            child["name"] == *name
                || child["aliases"]
                    .as_array()
                    .is_some_and(|aliases| aliases.iter().any(|alias| alias == name))
        })
    }

    fn is_leaf(node: &serde_json::Value) -> bool {
        node["subcommands"]
            .as_array()
            .is_none_or(|children| children.is_empty())
    }

    /// A bare command word, or a `list|show` / `get/set` alternation of them.
    fn command_words(token: &str) -> Option<Vec<&str>> {
        let words: Vec<&str> = token.split(['|', '/']).filter(|w| !w.is_empty()).collect();
        let bare = |w: &str| {
            w.starts_with(|c: char| c.is_ascii_lowercase())
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        };
        (!words.is_empty() && words.iter().copied().all(bare)).then_some(words)
    }

    /// Lines and backtick spans are where a fragment prescribes a runnable
    /// command. Prose that merely says "codeterm" is not a reference.
    fn command_spans(text: &str) -> Vec<&str> {
        let mut spans = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("codeterm ") {
                spans.push(trimmed);
            }
            for (index, part) in trimmed.split('`').enumerate() {
                if index % 2 == 1 {
                    spans.push(part.trim());
                }
            }
        }
        spans
    }

    /// Resolve every command reference in one span, appending a human-readable
    /// complaint for each reference the manifest cannot account for.
    fn check_span(root: &serde_json::Value, span: &str, problems: &mut Vec<String>) {
        let tokens: Vec<&str> = span.split_whitespace().collect();
        for (index, token) in tokens.iter().enumerate() {
            if *token != "codeterm" || index + 1 >= tokens.len() {
                continue;
            }
            if command_words(tokens[index + 1]).is_none() {
                continue; // `codeterm <cmd> --help`, a bare binary mention, prose
            }
            let mut node = root;
            let mut path: Vec<&str> = Vec::new();
            let mut cursor = index + 1;
            let mut alternation = false;
            let mut unknown = false;
            while cursor < tokens.len() && !alternation && tokens[cursor] != "codeterm" {
                let Some(words) = command_words(tokens[cursor]) else {
                    break;
                };
                let resolved: Vec<Option<&serde_json::Value>> =
                    words.iter().map(|word| subcommand(node, word)).collect();
                if resolved.iter().any(Option::is_none) {
                    if is_leaf(node) {
                        break; // a positional argument of a leaf command
                    }
                    path.push(tokens[cursor]);
                    problems.push(format!("unknown command `codeterm {}`", path.join(" ")));
                    unknown = true;
                    break;
                }
                node = resolved[0].expect("resolved above");
                path.push(tokens[cursor]);
                cursor += 1;
                alternation = words.len() > 1;
            }
            if unknown {
                continue;
            }
            if !alternation && !is_leaf(node) {
                if let Some(next) = tokens.get(cursor) {
                    if !next.starts_with('-')
                        && !next.starts_with('<')
                        && *next != "..."
                        && *next != "codeterm"
                    {
                        problems.push(format!(
                            "`codeterm {}` is a command group but is used as if it took arguments",
                            path.join(" ")
                        ));
                    }
                }
            }
        }
    }

    /// A rename that strands a prompt fails here instead of teaching agents a
    /// dead command. Resolution against the manifest, never prompt wording.
    #[test]
    fn prompt_cli_references_exist_in_manifest() {
        let root: serde_json::Value =
            serde_json::from_str(CLI_MANIFEST).expect("cli-manifest.json must parse");

        let mut problems: Vec<String> = Vec::new();
        for (name, fragment) in cli_teaching_fragments() {
            for span in command_spans(fragment) {
                let before = problems.len();
                check_span(&root, span, &mut problems);
                for problem in problems.iter_mut().skip(before) {
                    *problem = format!("{name}: {problem}");
                }
            }
        }

        assert!(
            problems.is_empty(),
            "prompts teach commands the CLI manifest does not have. If the command \
             is new, docs/cli-manifest.json needs regeneration:\n{}",
            problems.join("\n")
        );
    }

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

    /// The docs-discovery rule has exactly one home, asserted by command form
    /// rather than by the sentence that introduces it.
    #[test]
    fn codeterm_docs_rule_has_exactly_one_home() {
        const DISCOVERY_FORM: &str = "codeterm docs <NAME>";

        assert_eq!(CODETERM_CLI.matches(DISCOVERY_FORM).count(), 1);
        let general = GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI);
        assert_eq!(general.matches(DISCOVERY_FORM).count(), 1);
        for role_surface in [
            ORCHESTRATOR_ONBOARDING,
            ORCHESTRATOR_SUFFIX,
            PLANNER_SUFFIX,
            WORKER_SUFFIX,
            AGENT_CODETERM_PRIMER,
            WORKER_CODETERM_CONTEXT,
        ] {
            assert_eq!(
                role_surface.matches(DISCOVERY_FORM).count(),
                0,
                "role instruction restates the docs rule instead of pointing at its one home"
            );
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
                        "API-CLI",
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
    fn orchestrator_suffix_embeds_the_canonical_manual() {
        let rendered = orchestrator_suffix();
        assert!(rendered.ends_with(ORCHESTRATION_DOC.trim()));
        assert!(!rendered.contains("{planning_compaction_contract}"));
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

        // Both planning surfaces are system prompts; the onboarding first-message
        // no longer carries methodology, so it is intentionally absent here.
        for rendered in [orchestrator_suffix(), planner_suffix()] {
            assert!(
                rendered.contains(contract),
                "planning contract not delivered"
            );
            assert!(!rendered.contains("{planning_compaction_contract}"));
        }
    }

    /// Round-trips values this test supplies, so it proves substitution rather
    /// than a phrase the prompt happens to contain.
    #[test]
    fn prompt_placeholders_are_fully_substituted() {
        let spawn_cmd = "codeterm agent spawn claude --role Planner";
        let composed = [
            ("orchestrator_suffix", orchestrator_suffix()),
            ("planner_suffix", planner_suffix()),
            (
                "orchestrator_first_message",
                orchestrator_first_message("tab-x", "team-y", Some("pl_z")),
            ),
            (
                "orchestrator_first_message_no_plan",
                orchestrator_first_message("tab-x", "team-y", None),
            ),
            ("peer_pairing_mandate", peer_pairing_mandate_full(spawn_cmd)),
            (
                "general_agent",
                GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI),
            ),
        ];

        for (name, rendered) in composed {
            for slot in [
                "{planning_compaction_contract}",
                "{team_creation_contract}",
                "{test_author_guidance}",
                "{orchestrator_suffix}",
                "{planner_spawn_cmd}",
                "{spawn_cmd}",
                "{codeterm_cli}",
            ] {
                assert!(
                    !rendered.contains(slot),
                    "{name} still carries the unsubstituted slot {slot}"
                );
            }
        }

        // Round-trip: the ids this test supplied must survive substitution.
        let first_message = orchestrator_first_message("tab-x", "team-y", Some("pl_z"));
        assert!(first_message.contains("tab-x"));
        assert!(first_message.contains("team-y"));
        assert!(first_message.contains("pl_z"));
        assert!(peer_pairing_mandate_full(spawn_cmd).contains(spawn_cmd));
    }

    /// Delivery, asserted by constant identity. Fragments are opaque tokens
    /// here: which recipient receives which, never their wording.
    #[test]
    fn role_fragments_reach_their_recipients() {
        let orchestrator = orchestrator_suffix();
        let planner = planner_suffix();
        let worker = worker_suffix();

        for (recipient, rendered, fragments) in [
            (
                "orchestrator",
                &orchestrator,
                vec![PLANNING_COMPACTION_CONTRACT, TEAM_CREATION_CONTRACT],
            ),
            (
                "planner",
                &planner,
                vec![PLANNING_COMPACTION_CONTRACT, TEAM_CREATION_CONTRACT],
            ),
            (
                "worker",
                &worker,
                vec![TESTING_DISCIPLINE, QA_PIPELINE_DISCIPLINE, WORKER_SUFFIX],
            ),
        ] {
            for fragment in fragments {
                let token = fragment.trim();
                assert!(!token.is_empty(), "an empty fragment proves nothing");
                assert_eq!(
                    rendered.matches(token).count(),
                    1,
                    "{recipient} must receive each of its fragments exactly once"
                );
            }
        }

        // The Team-bound creation contract has one source, not a copy per role.
        let contract = TEAM_CREATION_CONTRACT.trim();
        for surface in [
            WORKER_SUFFIX,
            AGENT_CODETERM_PRIMER,
            WORKER_CODETERM_CONTEXT,
        ] {
            assert!(!surface.contains(contract));
        }
    }

    #[test]
    fn composed_prompt_stays_within_size_bound() {
        for (name, rendered) in [
            ("orchestrator_suffix", orchestrator_suffix()),
            ("planner_suffix", planner_suffix()),
            ("worker_suffix", worker_suffix()),
            (
                "general_agent",
                GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI),
            ),
        ] {
            assert!(
                rendered.len() <= COMPOSED_ROLE_PROMPT_MAX_BYTES,
                "{name} is {} bytes, over COMPOSED_ROLE_PROMPT_MAX_BYTES ({})",
                rendered.len(),
                COMPOSED_ROLE_PROMPT_MAX_BYTES
            );
        }
    }

    #[test]
    fn planner_suffix_embeds_the_canonical_manual() {
        let rendered = planner_suffix();
        assert!(rendered.ends_with(PLANNER_DOC.trim()));
        assert!(!rendered.contains("{planning_compaction_contract}"));
        assert!(!rendered.contains("{test_author_guidance}"));
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

    /// Was named for GA routing while asserting template phrases; the mechanism
    /// it can actually own is the CLI-reference injection seam.
    #[test]
    fn general_agent_template_receives_the_cli_reference() {
        assert!(GENERAL_AGENT_TEMPLATE.contains("{codeterm_cli}"));

        let general = GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI);
        assert!(general.contains(CODETERM_CLI));
        assert!(!general.contains("{codeterm_cli}"));
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
            "team_creation_contract.md",
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
