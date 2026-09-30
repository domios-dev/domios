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

pub const PLANNER_DOC: &str = include_str!("../../prod-docs/PLANNER.md");

/// The executor-role member of [`PLANNER_TASK_MANDATORY_FIELDS`]. It lives in
/// that list, not in a sentence beside it: a list member cannot be delivered
/// with its requirement edited out.
pub const PLANNER_TASK_EXECUTOR_ROLE_FIELD: &str = "an `--executor-role` naming who executes it";

/// The worker-conditioning member of [`PLANNER_TASK_MANDATORY_FIELDS`].
/// Presence is enforceable; prose quality is deliberately not validated.
pub const PLANNER_TASK_ROLE_GUIDANCE_FIELD: &str =
    "`--role-guidance` conditioning who the worker is and what it does";

/// The persona member of [`PLANNER_TASK_MANDATORY_FIELDS`]. A task without one
/// dispatches a worker that falls back to the title 'Worker'.
pub const PLANNER_TASK_PERSONALITY_FIELD: &str = "a one-word `--personality`";

/// Every field a task must carry at creation. The prompt sentence is composed
/// from this list, so a field is obliged by membership rather than by prose.
pub const PLANNER_TASK_MANDATORY_FIELDS: &[&str] = &[
    "a bounded body",
    "section links",
    "acceptance criteria",
    "a correctness vector",
    "an executable test plan",
    "an ownership boundary",
    "report expectations",
    PLANNER_TASK_EXECUTOR_ROLE_FIELD,
    PLANNER_TASK_ROLE_GUIDANCE_FIELD,
    PLANNER_TASK_PERSONALITY_FIELD,
];

/// Routing that stays optional — out of the mandatory scope, not out of the product.
pub const PLANNER_TASK_ROUTING_FLAGS: &[&str] = &["`--model`"];

fn oxford_join(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [only] => (*only).to_string(),
        [first, second] => format!("{first} and {second}"),
        [head @ .., last] => format!("{}, and {last}", head.join(", ")),
    }
}

/// Fills `{task_contract_line}` in [`PLANNER_SUFFIX`].
pub fn planner_task_contract_line() -> String {
    format!(
        "Every task needs {}.",
        oxford_join(PLANNER_TASK_MANDATORY_FIELDS)
    )
}

/// Fills `{task_routing_line}` in [`PLANNER_SUFFIX`].
pub fn planner_task_routing_line() -> String {
    format!(
        "Routing is yours too: set {} at add time with a rationale, so the Orchestrator never has to re-route by hand.",
        oxford_join(PLANNER_TASK_ROUTING_FLAGS)
    )
}

/// Canonical role_guidance for tasks whose primary deliverable is tests.
pub const TEST_AUTHOR_GUIDANCE: &str = "You are a test-author. `codeterm docs TEST-AUTHORING` is your operating charter — read it fully before writing the first test. Non-negotiables: oracle-first, red-green demo in your report, condition-waits never sleeps, agent-readiness gates, bounded everything, smoke-run every harness script against the real CLI.";

/// Appended to Orchestrator tasks — no-sleep rule.
pub const ORCHESTRATOR_SUFFIX: &str = include_str!("../prompts/orchestrator_suffix.md");

pub const ORCHESTRATION_DOC: &str = include_str!("../../prod-docs/ORCHESTRATION.md");

/// Fills `{roleless_draft_rejection}` in [`ORCHESTRATOR_SUFFIX`], inside the
/// instruction where drafts are already accepted, revised, or rejected.
pub const ORCH_ROLELESS_DRAFT_REJECTION: &str = "reject any task that names no executor role";

/// Delivered at the Orchestrator's existing draft-verdict decision point.
pub const ORCH_EMPTY_ROLE_GUIDANCE_REJECTION: &str =
    "reject any draft whose worker guidance says nothing";

/// Connects the missing-ACK event to the shared child PTY recovery procedure.
pub const ORCH_CHILD_PTY_RECOVERY_TRIGGER: &str = "- worker_ack_startup_failed/interactive_child_pty: inspect tab/attempt; hold prompt-bound; declared safe choices only; recheck screen/binding/ACK; action-needed unknowns; no brief/ACK prose, keys.";

/// Stable Orchestrator delivery token for the optional Cartographer Manager.
pub const ORCH_CARTOGRAPHER_CONTRACT_NOTICE: &str =
    "Cartographer: optional Manager for project-specific normative essences, including from zero; no spawn, approval or release authority, no continuous cadence.";

/// Task-decomposition contract shared by both planning surfaces. One source so
/// the Planner that cuts the graph and the Orchestrator that judges it agree.
pub const PLANNING_COMPACTION_CONTRACT: &str = include_str!("../prompts/planning_compaction.md");

/// Team-bound creation contract shared by the two roles that can ask for a new
/// agent, shell, or schedule. One source so neither can drift into a loose one.
pub const TEAM_CREATION_CONTRACT: &str = include_str!("../prompts/team_creation_contract.md");

/// How the Orchestrator sequences work: actionability, group retrospectives, and rare plan pause.
pub const TASK_SEQUENCING_CONTRACT: &str = include_str!("../prompts/task_sequencing_contract.md");

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

/// Read-act-verify procedure injected once into the General Agent prompt.
pub const INTERACTIVE_CHILD_PTY_OPERATION: &str =
    include_str!("../prompts/interactive_child_pty.md");
pub const INTERACTIVE_CHILD_PTY_CAPABILITY_ID: &str = "interactive_child_pty";

/// Compact CodeTerm control contract prepended to non-General agent launches.
pub const AGENT_CODETERM_PRIMER: &str = include_str!("../prompts/agent_codeterm_primer.md");

/// The load-bearing half of the notify restriction: rare, and only for the two
/// cases the primer names. Pinned here so a rewrite that softens the escalation
/// into an invitation fails a test instead of shipping.
pub const NOTIFY_RARE_ESCALATION_RULE: &str =
    "a rare user-attention escalation, valid in exactly two cases";

/// The ask clause carried by every surface that teaches notify. One sentence,
/// one source: an escalation that already earned its rarity may ask instead of
/// announce, and this is the grammar for asking.
pub const NOTIFY_ASK_CLAUSE: &str = "An escalation may ask rather than announce: repeat `--choice \"Label\"` once per option (no cap) and the tapped label returns to the tab that asked; blank and duplicate labels are refused. Asking the user a question goes through this same path and never through a provider-native question or prompt tool — those render only inside this tab, so the bell, Telegram, push, and mesh never carry the question and a user who is away cannot answer it.";

/// The surviving half of the relaxation: a periodic status, never one
/// notification per step. Kept as its own constant, and as a substring of
/// [`NOTIFY_VERBOSE_COMPLETION_CLAUSE`], so the permission cannot be delivered
/// with the restriction edited out of it.
pub const NOTIFY_NO_RUNNING_COMMENTARY_RULE: &str =
    "never a running commentary of one notification per step";

/// The other half a rewrite must not soften: the two roles that report through
/// a channel instead of interrupting the user. Pinned by constant so a fan-out
/// of Workers can never become a fan-out of notifications.
pub const NOTIFY_WORKER_PLANNER_SILENCE_RULE: &str =
    "Worker and Planner tabs never notify the user directly";

/// `verbose` may carry a short completion or progress note. One sentence, one
/// source, carried by every surface that teaches notify.
pub const NOTIFY_VERBOSE_COMPLETION_CLAUSE: &str = "A short completion or progress note may be filed at `verbose`; keep it a periodic status, never a running commentary of one notification per step.";

/// What an `unseen` report on a send result means, so an agent reads it as
/// information rather than an error and does not retry a recorded notification.
pub const NOTIFY_UNSEEN_CLAUSE: &str = "A send that reports `unseen` surfaces under the id is information, not a failure: the record was kept and the exit is still 0, so nothing is waiting to be retried.";

/// Standalone Manager contract for project-specific normative essences.
/// It carries its own bounded authority and canonical verification discipline.
pub const CARTOGRAPHER_SUFFIX: &str = include_str!("../prompts/cartographer_suffix.md");

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
First read `codeterm docs PLANNER` for your role. The Orchestrator owns the Team Plan and states its GOAL, then delegates planning to you by default. Refine that same Team Plan with `codeterm plan goal set`, `codeterm plan section add`, `codeterm plan task add`, `codeterm plan section show`, `codeterm plan get --sections`, and `codeterm plan doctor --kind quality`. No separate Planner-to-Plan binding or General Agent bootstrap is required. If no planning request has been delegated yet, reply that you are ready and wait."
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
First read `codeterm docs PLANNER` for your role. Do not create a bootstrap Plan, bind yourself to a Plan, or contact the General Agent. Planning is delegated to you by default: refine the active Team Plan with the normal plan editing commands once the Orchestrator states the goal. If no planning request has been delegated yet, reply that you are ready and wait."
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
            "{child_pty_recovery_trigger}",
            ORCH_CHILD_PTY_RECOVERY_TRIGGER,
        )
        .replace("{roleless_draft_rejection}", ORCH_ROLELESS_DRAFT_REJECTION)
        .replace(
            "{empty_role_guidance_rejection}",
            ORCH_EMPTY_ROLE_GUIDANCE_REJECTION,
        )
        .replace(
            "{task_sequencing_contract}",
            TASK_SEQUENCING_CONTRACT.trim(),
        )
        .replace(
            "{planning_compaction_contract}",
            PLANNING_COMPACTION_CONTRACT.trim(),
        )
        .replace(
            "{cartographer_contract_notice}",
            ORCH_CARTOGRAPHER_CONTRACT_NOTICE,
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
/// Stable token for the plan-absent create command. `plan create` takes no
/// `--team`; the bind is a separate verb, pinned by [`ORCH_ONBOARD_ADOPT_STEP`].
pub const ORCH_ONBOARD_CREATE_STEP: &str = "codeterm plan create --title";
/// Stable token for the plan-absent bind step that adopts the new plan.
pub const ORCH_ONBOARD_ADOPT_STEP: &str = "codeterm team adopt-plan";

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
         The daemon waits for this tab's session-binding event during plan writes, through the startup deadline. If it expires first, stop on the retryable `session_binding_pending` result instead of polling. Then proceed with these plan commands:\n\
         Step 1 — create the Team Plan:\n{ORCH_ONBOARD_CREATE_STEP} \"<your goal>\"\n\n\
         Step 2 — once step 1 prints the new plan id, bind it to this team:\n{ORCH_ONBOARD_ADOPT_STEP} {team_id} <the plan id printed in step 1>\n\n\
         Step 3 — spawn your Planner bound to that plan and to this tab (never through the General Agent):\ncodeterm agent spawn <provider> [--model <model>] --parent-tab {tab_id} --role Planner --plan <the plan id printed in step 1>\n\n\
         Then state the goal to the Planner and wait for its draft; do not cut tasks yourself."
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
        .replace("{task_contract_line}", &planner_task_contract_line())
        .replace("{task_routing_line}", &planner_task_routing_line())
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

/// The Cartographer Manager has its own contract, without Worker or Planner
/// task lifecycle instructions granting contradictory authority.
pub fn cartographer_suffix() -> String {
    CARTOGRAPHER_SUFFIX.trim().to_string()
}

/// Upper bound for a composed role prompt. Not a style preference: every byte
/// here is spent on every launch of that role, so growth needs a decision.
/// Re-measured at the General Agent's 32_895 LF bytes for v1.11.11.
pub const COMPOSED_ROLE_PROMPT_MAX_BYTES: usize = 32_900;

#[cfg(test)]
mod tests;
