use clap::Subcommand;

mod canvas;
mod task;

pub use canvas::{
    PlanDecisionAction, PlanElementAction, PlanHandoffAction, PlanLayoutAction,
    PlanMilestoneAction, PlanNoteAction, PlanQaAction, PlanReviewAction,
};
pub use task::PlanTaskAction;

#[derive(Subcommand)]
#[allow(
    clippy::large_enum_variant,
    reason = "Clap command enums keep parsed options inline."
)]
pub enum PlanAction {
    /// Create a new plan
    Create {
        #[arg(long, allow_hyphen_values = true)]
        title: String,
        #[arg(long)]
        from_spec: Option<String>,
        #[arg(long, allow_hyphen_values = true, hide = true)]
        spec: Option<String>,
        #[arg(long, value_name = "PATH", hide = true)]
        spec_file: Option<String>,
        #[arg(long)]
        kinds: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// List plans
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        workspace: bool,
        #[arg(long)]
        active: bool,
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        unowned: bool,
        #[arg(long)]
        orchestrator: Option<String>,
    },
    /// List active orchestration tasks
    InProgress {
        plan_id: Option<String>,
        #[arg(long, conflicts_with = "plan_id")]
        all: bool,
    },
    /// Render the whole plan as markdown (goal + sections + tasks); --sections for the tree only
    Get {
        plan_id: Option<String>,
        #[arg(long)]
        sections: bool,
    },
    /// Print only the plan's hydrated section prose (no goal, tasks, or metadata)
    Spec { plan_id: String },
    /// File + line range of a body block, for in-place edits with file tools
    Locate {
        plan_id: String,
        /// Task id or handle
        #[arg(long, conflicts_with_all = ["section", "spec_section"])]
        task: Option<String>,
        /// Plan section id or handle
        #[arg(long, conflicts_with_all = ["task", "spec_section"])]
        section: Option<String>,
        /// Spec section id or handle (of the plan's spec)
        #[arg(long, conflicts_with_all = ["task", "section"])]
        spec_section: Option<String>,
    },
    /// List generated section/task body files
    Files { plan_id: String },
    /// Show tag facets, counts, topics, and duplicate warnings
    Tags { plan_id: String },
    /// One read-only state snapshot (counts, coverage, drift, loops, lanes, strides)
    State { plan_id: Option<String> },
    /// Lane hygiene table: one row per open lane (task/status/tab/branch/worktree/cleanup)
    Lanes {
        plan_id: Option<String>,
        /// Only show stale lanes (dead tab / report-overdue / uncleaned)
        #[arg(long)]
        stale: bool,
    },
    /// Prioritized, advisory next-action guidance computed from plan state
    Next { plan_id: String },
    /// Show section coverage (% + gaps)
    Coverage { plan_id: String },
    /// Show sections with zero tasks
    Gaps { plan_id: String },
    /// Detect plan/task drift and optionally apply conservative fixes
    Reconcile {
        plan_id: String,
        #[arg(long)]
        fix: bool,
    },
    /// Inspect project-local plan files for storage and structure errors
    Doctor {
        plan_id: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "all")]
        kind: String,
    },
    /// Preview or delete smoke/empty project-local plan files (dry-run by default)
    Prune {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Adopt an existing plan sidecar directory into this project
    Adopt {
        #[arg(value_name = "SOURCE_PLAN_DIR")]
        source: String,
        #[arg(long, alias = "to", value_name = "PATH")]
        project: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        overwrite: bool,
    },
    /// Move a plan sidecar from another project or data dir into this project
    Move {
        plan_id: String,
        #[arg(long, value_name = "PROJECT_OR_DATA_DIR")]
        from: String,
        #[arg(long, value_name = "PATH")]
        to: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        overwrite: bool,
    },
    /// Plan north-star goal
    Goal {
        #[command(subcommand)]
        action: PlanGoalAction,
    },
    /// Show structural neighbors for a plan/spec/task/section unit
    Related {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "1")]
        depth: usize,
    },
    /// Search synced plan/spec/task/stride history for this project
    Search {
        query: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value = "10")]
        limit: usize,
        #[arg(long, conflicts_with_all = ["archived", "all"])]
        active: bool,
        #[arg(long, conflicts_with_all = ["active", "all"])]
        archived: bool,
        #[arg(long, conflicts_with_all = ["active", "archived"])]
        all: bool,
    },
    /// Rebuild the project-local plan recall index
    Reindex {
        #[arg(long)]
        project: Option<String>,
    },
    /// Set task groups (phases)
    Groups {
        #[command(subcommand)]
        action: PlanGroupsAction,
    },
    /// Set declared tag facets
    Facets {
        #[command(subcommand)]
        action: PlanFacetsAction,
    },
    /// Link this plan to a related plan
    Link {
        plan_id: String,
        #[arg(long)]
        to: String,
    },
    /// Remove a plan link
    Unlink {
        plan_id: String,
        #[arg(long)]
        to: String,
    },
    /// Update plan metadata
    Update {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        spec: Option<String>,
        #[arg(long, value_name = "PATH")]
        spec_file: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        mode: Option<String>,
        /// Set attached Planner: <tab_id> to bind, or "none" to waive (requires --note).
        #[arg(long)]
        planner: Option<String>,
        /// Waiver note (required when --planner none).
        #[arg(long, allow_hyphen_values = true)]
        note: Option<String>,
    },
    /// Mark the plan approved for task generation
    Approve { plan_id: String },
    /// Return an approved/execution plan to planning mode
    Unapprove { plan_id: String },
    /// Advance planner-owned planning state: drafting | structure_ready | approved
    PlanningState {
        plan_id: String,
        #[arg(long)]
        set: String,
        /// Planner tab id advancing the state (records author as planner; omit = user).
        #[arg(long = "tab", value_name = "TAB")]
        pane: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        note: Option<String>,
    },
    /// Set explicit effort mode: planning | approved | execution | complete
    Mode {
        plan_id: String,
        #[arg(long)]
        set: String,
    },
    /// Park the whole effort without changing task state or executor bindings
    #[command(visible_alias = "park")]
    Pause {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        reason: String,
    },
    /// Resume a paused effort, restoring its previous mode
    Resume { plan_id: String },
    /// Archive a plan
    Archive { plan_id: String },
    /// Restore an archived plan to active
    Unarchive { plan_id: String },
    /// Delete a plan. Cascades: the plan's owned spec (1:1, co-located
    /// spec.yaml) is deleted with it; specs merely linked/referenced from
    /// other plans are untouched and only lose their reference to this plan.
    Delete {
        plan_id: String,
        /// Confirm permanent deletion.
        #[arg(long, alias = "confirm")]
        yes: bool,
    },
    /// Transfer plan ownership
    Transfer {
        plan_id: String,
        #[arg(long)]
        to: String,
        #[arg(long, allow_hyphen_values = true)]
        reason: Option<String>,
    },
    /// Release plan ownership
    Disown { plan_id: String },
    /// Claim an unowned plan
    Claim { plan_id: String },
    /// Record task progress with one atomic plan write
    Record {
        /// Plan id (if available from tab binding or env)
        #[arg(env = "CODETERM_PLAN_ID")]
        plan_id: Option<String>,
        #[arg(long, env = "CODETERM_TASK_ID")]
        task: Option<String>,
        #[arg(long)]
        upsert_task: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        name: Option<String>,
        #[arg(long)]
        sections: Option<String>,
        #[arg(
            long,
            help = "Task status: pending | in_progress | in_review | blocked | paused | done | cancelled"
        )]
        status: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long = "step", allow_hyphen_values = true)]
        steps: Vec<String>,
        #[arg(long = "link", allow_hyphen_values = true)]
        links: Vec<String>,
        #[arg(long)]
        key: Option<String>,
    },
    /// Write the plan completion retrospective (plan-level stride, kind retrospective)
    Retrospective {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
    },
    /// Ingest markdown headings as plan sections
    Ingest {
        plan_id: String,
        #[arg(value_name = "PATH_OR_DASH")]
        markdown: String,
    },
    /// Orchestrator handoffs attached to the plan
    Handoff {
        #[command(subcommand)]
        action: PlanHandoffAction,
    },
    /// Plan sections
    Section {
        #[command(subcommand)]
        action: PlanSectionAction,
    },
    /// Set milestone kind columns
    Kinds {
        #[command(subcommand)]
        action: PlanKindsAction,
    },
    /// Self-improving loops
    Loop {
        #[command(subcommand)]
        action: PlanLoopAction,
    },
    /// Plan tasks
    Task {
        #[command(subcommand)]
        action: PlanTaskAction,
    },
    /// Task milestones
    Milestone {
        #[command(subcommand)]
        action: PlanMilestoneAction,
    },
    /// Serialized QA queue for in_review tasks
    Qa {
        #[command(subcommand)]
        action: PlanQaAction,
    },
    /// Review-backpressure: capacity, credit reconciliation, and verdicts
    Review {
        #[command(subcommand)]
        action: PlanReviewAction,
    },
    /// Canvas layout overrides
    Layout {
        #[command(subcommand)]
        action: PlanLayoutAction,
    },
    /// Free-form notes on the canvas
    Note {
        #[command(subcommand)]
        action: PlanNoteAction,
    },
    /// Generic canvas elements
    Element {
        #[command(subcommand)]
        action: PlanElementAction,
    },
    /// Decision gates — structured blocking questions routed to the planner
    Decision {
        #[command(subcommand)]
        action: PlanDecisionAction,
    },
    /// Plan-scoped reusable skills (folder-on-disk, ledgered, mem-indexed)
    Skill {
        #[command(subcommand)]
        action: PlanSkillAction,
    },
}

#[derive(Subcommand)]
pub enum PlanSkillAction {
    /// Create a new plan skill folder (SKILL.md + scripts/)
    New {
        slug: String,
        /// Plan id (omit to use $CODETERM_PLAN_ID or the single active plan)
        #[arg(long)]
        plan: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        #[arg(long)]
        area: Option<String>,
        /// Optional verification command persisted in SKILL.md frontmatter
        #[arg(long, allow_hyphen_values = true)]
        check: Option<String>,
        /// One-line ledger rationale (default: "created")
        #[arg(long, allow_hyphen_values = true)]
        rationale: Option<String>,
    },
    /// List plan skills with their latest ledger verdict
    List {
        #[arg(long)]
        plan: Option<String>,
    },
    /// Print a plan skill's SKILL.md and ledger history
    Show {
        slug: String,
        #[arg(long)]
        plan: Option<String>,
    },
    /// Record a keep|discard|superseded ledger entry; --body-file replaces SKILL.md
    Edit {
        slug: String,
        #[arg(long)]
        plan: Option<String>,
        #[arg(long, default_value = "keep")]
        verdict: String,
        #[arg(long, allow_hyphen_values = true)]
        rationale: String,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PlanGoalAction {
    /// Show the plan's north-star goal (alias of show)
    Get { plan_id: String },
    /// Set the plan's goal vector, vision, constraints, and postulates
    Set {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        vector: String,
        #[arg(long, allow_hyphen_values = true)]
        vision: Option<String>,
        #[arg(long, value_name = "PATH")]
        vision_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        constraint: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        postulate: Vec<String>,
    },
    /// Show the plan's north-star goal (vector, vision, constraints, postulates)
    Show { plan_id: String },
}

#[derive(Subcommand)]
pub enum PlanSectionAction {
    /// Add a plan section (execution phase), optionally nested under --parent
    Add {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        title: String,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        primary: Option<String>,
        #[arg(long)]
        handle: Option<String>,
    },
    /// Update a plan section's title, body, tags, or handle
    Update {
        plan_id: String,
        section_id: String,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        primary: Option<String>,
        #[arg(long)]
        handle: Option<String>,
    },
    /// Show one plan section's title, body, tags, and subsections
    Show { plan_id: String, section_id: String },
    /// Attach a file to a plan section
    Attach {
        plan_id: String,
        section_id: String,
        #[arg(value_name = "PATH")]
        file: String,
    },
    /// Remove a plan section and its subsections
    Rm { plan_id: String, section_id: String },
}

#[derive(Subcommand)]
pub enum PlanKindsAction {
    /// Set the declared milestone kind columns (CSV)
    Set {
        plan_id: String,
        #[arg(long)]
        kinds: String,
    },
}

#[derive(Subcommand)]
pub enum PlanGroupsAction {
    /// Set the task groups / phases (CSV)
    Set {
        plan_id: String,
        #[arg(long)]
        groups: String,
    },
}

#[derive(Subcommand)]
pub enum PlanFacetsAction {
    /// Set the declared tag facets (CSV)
    Set {
        plan_id: String,
        #[arg(long)]
        facets: String,
    },
}

#[derive(Subcommand)]
pub enum PlanLoopAction {
    /// Declare a self-improving loop with a target and iteration budget
    Create {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        target: String,
        #[arg(long)]
        budget: u32,
        #[arg(long, allow_hyphen_values = true)]
        stop_when: Option<String>,
    },
    /// Update a loop's budget or stop condition
    Set {
        loop_id: String,
        #[arg(long)]
        budget: Option<u32>,
        #[arg(long, allow_hyphen_values = true)]
        stop_when: Option<String>,
    },
    /// Show a loop's iteration count, budget, and status
    Status { loop_id: String },
    /// Stop a loop (marks it complete)
    Stop { loop_id: String },
    /// Record one loop iteration and leave a stride
    Record {
        loop_id: String,
        #[arg(long, allow_hyphen_values = true)]
        note: String,
        #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
        stop: bool,
        #[arg(long, hide = true, env = "CODETERM_TASK_ID")]
        task: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::CommandFactory;

    #[test]
    fn plan_delete_help_documents_owned_spec_cascade() {
        let help = Cli::command()
            .find_subcommand_mut("plan")
            .unwrap()
            .find_subcommand_mut("delete")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("Cascades"), "{help}");
        assert!(help.contains("owned spec"), "{help}");
    }
}
