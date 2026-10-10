pub const WORKER_SUFFIX: &str = include_str!("../../../../prompts/worker_suffix.md");
pub const TESTING_DISCIPLINE: &str = include_str!("../../../../prompts/testing_discipline.md");
pub const QA_PIPELINE_DISCIPLINE: &str =
    include_str!("../../../../prompts/qa_pipeline_discipline.md");

pub const WORKER_LOOP_CONTEXT_TITLE: &str = "Loop context:";
pub const WORKER_LOOP_EVIDENCE_RULE: &str =
    "When you report, include this iteration's evidence, whether it made progress, and whether the loop should continue or stop.";
pub const WORKER_COMPETITIVE_CONTEXT_TITLE: &str = "Competitive context:";
pub const WORKER_COMPETITIVE_ISOLATION_RULES: [&str; 3] = [
    "Work only in your own assigned lane, branch, and worktree.",
    "Do not inspect, tail, or reuse another live attempt's tab, worktree, or branch.",
    "The Orchestrator compares the attempts and decides the winner or next follow-up.",
];

pub const HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE: &str = "Competitive attempts:";
pub const REGRESSION_MILESTONE_KIND: &str = "regression";

pub const REFLECTION_HEARTBEAT_TITLE: &str = "Reflection:";
pub const REFLECTION_BLOCK: &str = "{trigger} — optional self-review at plan wrap-up (tasks closed: {closed}, bounces: {bounces}, gate leaks: {leaks}). Route useful lessons with `codeterm orchestrator learn`; reusable procedures belong in plan skills, and prompt defects go to the Planner.";

pub fn reflection_block(trigger: &str, window: &super::reflection::ReflectionWindow) -> String {
    REFLECTION_BLOCK
        .replace("{trigger}", trigger)
        .replace("{closed}", &window.tasks_closed.to_string())
        .replace("{bounces}", &window.bounces.to_string())
        .replace("{leaks}", &window.gate_leaks.to_string())
}

pub const PLAN_ARCHIVE_SUGGESTION: &str = "This plan is complete. Before archiving, CLEAN UP AFTER THE EFFORT — this is a mandatory closeout step, not optional tidiness:\n\
- Close every finished worker tab you spawned (`codeterm tab close <id>`), and the attached Planner tab once its work is done.\n\
- Remove each lane worktree and its merged branch with `codeterm plan lane-remove <worktree> --branch <branch>`, which refuses while a live tab still holds that directory, and stop any lane-specific dev daemons.\n\
- Prune stale or superseded milestones so the completed plan reads clean.\n\
Orphaned tabs, worktrees, branches, and dev daemons you spawned are THIS effort's mess to clear — never the next orchestrator's. Then archive with `codeterm plan archive <plan>`.";

pub const PLAN_RETROSPECTIVE_NUDGE: &str = "At plan wrap-up, optionally record one useful note with `codeterm plan retrospective <plan> --body \"...\"`, `codeterm plan groups close <plan> <group> --note \"<note>\"`, or `codeterm orchestrator learn \"<lesson>\"`. A plan can archive without any of these records.\n\
Graduate proven procedures to machine mem (`codeterm mem skill`) and, when repo-shaped, to `.agents/skills/` or `scripts/` through a reviewed lane.";

pub const PEER_PLANNER_CHARTER_LINE: &str = "Planner is the Team planning specialist and \
     delegation is the default: read the Team's active Plan without a separate binding, refine the \
     GOAL the Orchestrator states into a correctness vector, constraints, and postulates, draft the \
     Spec, the Plan sections, and the task briefs including worker routing, and surface decision \
     options. The Orchestrator owns the Team Plan, dispatches, and accepts.";

pub const PEER_WAIVER_HINT: &str = "No Planner specialist is bound. \
     Bind one with `codeterm plan update --planner <TAB_ID>` or waive with \
     `codeterm plan update --planner none --note <reason>`.";

/// The plan HOLDS a Planner ref whose tab is not a live, nonce-verified agent
/// session. Distinct from `PEER_WAIVER_HINT` because the refusal reads the same
/// while the remedy does not: re-binding the tab that is already bound changes
/// nothing, so naming the real state is what saves the dead-end retry.
pub const PEER_PLANNER_DOWN_HINT: &str = "A Planner specialist is bound, but its \
     tab has no live nonce-verified agent session, so planning cannot be \
     delegated to it. Respawn the Planner and rebind it with `codeterm plan \
     update --planner <TAB_ID>`, or waive with `codeterm plan update --planner \
     none --note <reason>`. Re-binding the same dead tab does not revive it.";

pub const PEER_ORCH_DEFER_STRUCTURE_LINE: &str =
    "Planning is not approved yet — state the GOAL and hand the Team Plan to {planner} to draft, \
     then review and approve it. Draft it yourself only on a formal Planner waiver or a narrow \
     edit per `codeterm docs PLAN-EDITING`.";

pub fn orch_defer_structure_line(planner: &str) -> String {
    PEER_ORCH_DEFER_STRUCTURE_LINE.replace("{planner}", planner)
}

pub const PLAN_SKILLS_PRINCIPLES: &str = "\
Plan skills — the plan's own know-how:
- Learned something reusable in this effort (a test recipe, a seeding script, a gotcha)? Record it: `codeterm plan skill new <slug>`. Improve an existing skill before writing a new one.
- A skill names what is FIXED (the oracle: the command or observation that proves success) and what is MUTABLE. Never edit the oracle to make results look better.
- Prefer skills with a --check command; cite the check when keeping or discarding an approach.
- Simplicity wins: a marginal gain that adds hacky steps is a loss; a deletion that preserves results is a win.
- Every skill edit logs keep/discard/superseded + why. Discarded approaches are data, not embarrassment.";

pub const PLAN_SKILLS_WORKER_LINE: &str = "This plan has skills — procedures earlier agents proved out. Check before inventing: `codeterm plan skill list`. Learned something reusable yourself? `codeterm plan skill new <slug>` — future workers inherit it.";

pub const PLAN_SKILLS_REGISTRY_LINE: &str =
    "Plan skills ({n}): {slug1}, {slug2}, … — codeterm plan skill show <slug> | new | edit.";

pub fn worker_suffix() -> String {
    format!("{TESTING_DISCIPLINE}\n{QA_PIPELINE_DISCIPLINE}\n{WORKER_SUFFIX}")
}
