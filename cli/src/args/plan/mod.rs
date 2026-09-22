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
    /// List active orchestration tasks; without PLAN_ID, scope to the current directory's project
    InProgress {
        /// Plan to list regardless of the caller's working directory
        plan_id: Option<String>,
        /// List active tasks across every project
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
    /// Remove one lane worktree and its branch, refusing while a live tab's shell sits inside it
    LaneRemove {
        /// Lane worktree path to remove
        #[arg(value_name = "WORKTREE")]
        worktree: String,
        /// Lane branch to delete after worktree removal, if merged into project HEAD
        #[arg(long, value_name = "BRANCH")]
        branch: Option<String>,
        /// Canonical checkout (needed for a missing worktree unless its task identifies the project)
        #[arg(long, value_name = "PATH")]
        project: Option<String>,
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
        /// Restrict to plan, plan_section, spec, spec_section, task, or stride
        #[arg(
            long,
            value_parser = ["plan", "plan_section", "spec", "spec_section", "task", "stride"]
        )]
        kind: Option<String>,
        /// Restrict to an indexed scope such as global or workspace:<id>
        #[arg(long)]
        scope: Option<String>,
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
    /// Re-sync a planning binding nonce that went stale after an agent relaunch,
    /// recovering plan mutation/approve/dispatch without a full `agent restart`.
    Rebind {
        /// Optional plan id for context; the rebind re-syncs this tab's binding.
        plan_id: Option<String>,
    },
    /// Return a draft to drafting; approval uses `plan approve`
    PlanningState {
        plan_id: String,
        #[arg(long)]
        set: String,
        /// Deprecated author label retained for draft reset records.
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        note: Option<String>,
    },
    /// Set explicit effort mode: planning | approved | execution | complete
    Mode {
        plan_id: String,
        #[arg(
            value_name = "MODE",
            conflicts_with = "set",
            help = "Mode value as a trailing positional (alternative to --set): planning | approved | execution | complete"
        )]
        mode: Option<String>,
        #[arg(long)]
        set: Option<String>,
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
    /// Update the plan's goal; fields not passed keep their current value
    Set {
        plan_id: String,
        #[arg(
            value_name = "GOAL_TEXT",
            help = "Optional trailing goal statement; stored as vision unless --vision/--vision-file is given"
        )]
        goal_text: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Goal vector; required the first time a goal is set. Every field you omit is kept"
        )]
        vector: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        vision: Option<String>,
        #[arg(long, value_name = "PATH")]
        vision_file: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Replaces the constraint list; repeatable. Omit to keep, pass an empty value to clear"
        )]
        constraint: Vec<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Replaces the postulate list; repeatable. Omit to keep, pass an empty value to clear"
        )]
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
    /// Record a group's retrospective stride: closes its reflection window and clears its retrospective dispatch gate
    Close {
        plan_id: String,
        key: String,
        #[arg(long)]
        note: String,
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

    fn parse_goal_set(argv: &[&str]) -> (Option<String>, Option<String>) {
        use crate::args::cli::Commands;
        use crate::args::plan::{PlanAction, PlanGoalAction};
        use clap::Parser;
        let cli = Cli::try_parse_from(argv).expect("goal set should parse");
        match cli.command {
            Commands::Plan {
                action:
                    PlanAction::Goal {
                        action:
                            PlanGoalAction::Set {
                                goal_text, vision, ..
                            },
                    },
            } => (goal_text, vision),
            _ => panic!("expected plan goal set"),
        }
    }

    #[test]
    fn goal_set_accepts_trailing_positional_goal_text() {
        let (goal_text, vision) = parse_goal_set(&[
            "codeterm",
            "plan",
            "goal",
            "set",
            "pl_1",
            "--vector",
            "success proof",
            "the north-star goal",
        ]);
        assert_eq!(goal_text.as_deref(), Some("the north-star goal"));
        assert_eq!(vision, None);
    }

    fn parse_mode(argv: &[&str]) -> (Option<String>, Option<String>) {
        use crate::args::cli::Commands;
        use crate::args::plan::PlanAction;
        use clap::Parser;
        let cli = Cli::try_parse_from(argv).expect("plan mode should parse");
        match cli.command {
            Commands::Plan {
                action: PlanAction::Mode { mode, set, .. },
            } => (mode, set),
            _ => panic!("expected plan mode"),
        }
    }

    #[test]
    fn plan_mode_positional_and_set_route_to_same_mode_value() {
        let (pos_mode, pos_set) = parse_mode(&["codeterm", "plan", "mode", "pl_1", "complete"]);
        assert_eq!(pos_mode.as_deref(), Some("complete"));
        assert_eq!(pos_set, None);

        let (flag_mode, flag_set) =
            parse_mode(&["codeterm", "plan", "mode", "pl_1", "--set", "complete"]);
        assert_eq!(flag_set.as_deref(), Some("complete"));
        assert_eq!(flag_mode, None);

        // The handler resolves via set-or-positional; both routes yield the same mode.
        let positional_resolved = pos_set.or(pos_mode);
        let flag_resolved = flag_set.or(flag_mode);
        assert_eq!(positional_resolved, flag_resolved);
        assert_eq!(flag_resolved.as_deref(), Some("complete"));
    }

    #[test]
    fn plan_mode_rejects_positional_and_set_together() {
        use clap::Parser;
        let parsed = Cli::try_parse_from([
            "codeterm", "plan", "mode", "pl_1", "complete", "--set", "planning",
        ]);
        assert!(parsed.is_err(), "positional MODE and --set must conflict");
    }

    #[test]
    fn goal_set_keeps_explicit_vision_alongside_positional() {
        let (goal_text, vision) = parse_goal_set(&[
            "codeterm",
            "plan",
            "goal",
            "set",
            "pl_1",
            "--vector",
            "proof",
            "--vision",
            "explicit vision",
        ]);
        assert_eq!(goal_text, None);
        assert_eq!(vision.as_deref(), Some("explicit vision"));
    }
}
