use clap::Subcommand;

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
        plan_id: String,
        #[arg(long)]
        sections: bool,
    },
    /// Print only the plan's hydrated section prose (no goal, tasks, or metadata)
    Spec { plan_id: String },
    /// List generated section/task body files
    Files { plan_id: String },
    /// Show tag facets, counts, topics, and duplicate warnings
    Tags { plan_id: String },
    /// One read-only state snapshot (counts, coverage, drift, loops, lanes, strides)
    State { plan_id: String },
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
    },
    /// Mark the plan approved for task generation
    Approve { plan_id: String },
    /// Return an approved/execution plan to planning mode
    Unapprove { plan_id: String },
    /// Set explicit effort mode: planning | approved | execution | complete
    Mode {
        plan_id: String,
        #[arg(long)]
        set: String,
    },
    /// Archive a plan
    Archive { plan_id: String },
    /// Restore an archived plan to active
    Unarchive { plan_id: String },
    /// Delete a plan
    Delete { plan_id: String },
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
        /// Plan id (default: $CODETERM_PLAN_ID)
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
            help = "Task status: pending | in_progress | in_review | blocked | done | cancelled"
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
    /// Advance a loop one iteration, recording whether it made progress
    Advance {
        loop_id: String,
        #[arg(long, action = clap::ArgAction::Set, default_value_t = false)]
        progress: bool,
    },
}

#[derive(Subcommand)]
pub enum PlanTaskAction {
    /// List a plan's tasks, optionally filtered by status
    List {
        plan_id: String,
        #[arg(
            long,
            help = "Task status: pending | in_progress | in_review | blocked | done | cancelled"
        )]
        status: Option<String>,
    },
    /// Add a task to a plan (carries model/why routing + quality fields)
    Add {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        name: String,
        #[arg(long = "type", allow_hyphen_values = true)]
        kind: Option<String>,
        #[arg(long)]
        sections: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        model: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        why: Option<String>,
        #[arg(long, value_name = "PATH")]
        why_file: Option<String>,
        #[arg(long = "criterion", allow_hyphen_values = true)]
        criterion: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        test_plan: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        correctness: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        role_guidance: Option<String>,
        #[arg(long, value_name = "PATH")]
        role_guidance_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        executor_role: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        assign_pane: Option<String>,
        #[arg(long)]
        workspace_id: Option<String>,
        #[arg(long)]
        workspace_name: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        destination: Option<String>,
        #[arg(long)]
        cleanup_status: Option<String>,
        #[arg(long = "cleanup-hint", allow_hyphen_values = true)]
        cleanup_hints: Vec<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        primary: Option<String>,
        #[arg(long)]
        handle: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        loop_from: Option<String>,
    },
    /// Update task metadata, status, quality fields, or lane assignment
    Update {
        plan_id: String,
        task_id: String,
        #[arg(long, allow_hyphen_values = true)]
        name: Option<String>,
        #[arg(long = "type", allow_hyphen_values = true)]
        kind: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        model: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        why: Option<String>,
        #[arg(long, value_name = "PATH")]
        why_file: Option<String>,
        #[arg(
            long,
            help = "Task status: pending | in_progress | in_review | blocked | done | cancelled"
        )]
        status: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        status_label: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        role_guidance: Option<String>,
        #[arg(long, value_name = "PATH")]
        role_guidance_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        executor_role: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long)]
        sections: Option<String>,
        #[arg(long = "criterion", allow_hyphen_values = true)]
        criterion: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        test_plan: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        correctness: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        workspace_id: Option<String>,
        #[arg(long)]
        workspace_name: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        destination: Option<String>,
        #[arg(long)]
        cleanup_status: Option<String>,
        #[arg(long = "cleanup-hint", allow_hyphen_values = true)]
        cleanup_hints: Vec<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        primary: Option<String>,
        #[arg(long)]
        handle: Option<String>,
    },
    /// Replace facet/topic tags and regenerate the task handle
    Retag {
        plan_id: String,
        task_id: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        primary: Option<String>,
    },
    /// Close a task (records a result; rejects if subtasks are open)
    Close {
        plan_id: String,
        task_id: String,
        #[arg(long, allow_hyphen_values = true)]
        result: Option<String>,
        #[arg(long, value_name = "PATH")]
        result_file: Option<String>,
        #[arg(long)]
        mesh: Option<String>,
    },
    /// Assign a task to a pane (exports CODETERM_TASK_ID to that pane)
    Assign {
        plan_id: String,
        task_id: String,
        #[arg(long)]
        pane: String,
        #[arg(long, alias = "peer")]
        mesh: Option<String>,
    },
    /// List execution attempts for a plan task
    Attempts { plan_id: String, task_id: String },
    /// Show one execution attempt for a plan task
    Attempt {
        plan_id: String,
        task_id: String,
        attempt_id: String,
    },
    /// Respawn a worker from one prior execution attempt
    Respawn {
        plan_id: String,
        task_id: String,
        #[arg(long)]
        attempt: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        title: Option<String>,
    },
    /// Link a task to one or more plan sections (CSV)
    Link {
        plan_id: String,
        task_id: String,
        #[arg(long)]
        sections: String,
    },
    /// Preview or apply automatic section links for unlinked tasks (--apply)
    Relink {
        plan_id: String,
        #[arg(long)]
        apply: bool,
    },
    /// Remove a task from the plan, or --all to reset generated tasks
    Rm {
        plan_id: String,
        task_id: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum PlanMilestoneAction {
    /// Add a milestone (stride) to a task; --add-kind declares a new kind inline
    Add {
        plan_id: String,
        task_id: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        add_kind: bool,
        #[arg(long, allow_hyphen_values = true)]
        text: Option<String>,
        #[arg(long, value_name = "PATH")]
        text_file: Option<String>,
        #[arg(long)]
        from: Option<String>,
    },
    /// Update a milestone's kind or text
    Update {
        plan_id: String,
        task_id: String,
        milestone_id: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        text: Option<String>,
        #[arg(long, value_name = "PATH")]
        text_file: Option<String>,
    },
    /// Remove a milestone from a task
    Rm {
        plan_id: String,
        task_id: String,
        milestone_id: String,
    },
}

#[derive(Subcommand)]
pub enum PlanLayoutAction {
    /// Set a canvas element's x/y layout override
    Set {
        plan_id: String,
        #[arg(long)]
        element: String,
        #[arg(long)]
        x: Option<f64>,
        #[arg(long)]
        y: Option<f64>,
    },
}

#[derive(Subcommand)]
pub enum PlanNoteAction {
    /// Add a free-form canvas note, optionally linked to a task or section
    Add {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        text: Option<String>,
        #[arg(long, value_name = "PATH")]
        text_file: Option<String>,
        #[arg(long)]
        link_task: Option<String>,
        #[arg(long)]
        link_section: Option<String>,
    },
    /// Update a free-form canvas note
    Update {
        plan_id: String,
        note_id: String,
        #[arg(long, allow_hyphen_values = true)]
        text: Option<String>,
        #[arg(long, value_name = "PATH")]
        text_file: Option<String>,
    },
    /// Remove a free-form canvas note
    Rm { plan_id: String, note_id: String },
}

#[derive(Subcommand)]
pub enum PlanElementAction {
    /// List generic canvas elements
    List {
        plan_id: String,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Show one generic canvas element
    Show { plan_id: String, element_id: String },
    /// Add a generic canvas element of an arbitrary kind, optionally linked
    Add {
        plan_id: String,
        #[arg(long)]
        kind: String,
        #[arg(long, allow_hyphen_values = true)]
        content: Option<String>,
        #[arg(long, value_name = "PATH")]
        content_file: Option<String>,
        #[arg(long)]
        link: Option<String>,
    },
    /// Update a generic canvas element's content, kind, or links
    Update {
        plan_id: String,
        element_id: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        content: Option<String>,
        #[arg(long, value_name = "PATH")]
        content_file: Option<String>,
        #[arg(long)]
        link: Option<String>,
    },
    /// Remove a generic canvas element
    Rm { plan_id: String, element_id: String },
}

#[derive(Subcommand)]
pub enum PlanHandoffAction {
    /// Create a durable, goal-anchored handoff attached to the plan
    Create {
        plan_id: String,
        #[arg(long, default_value = "record")]
        mode: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        goal: Option<String>,
        #[arg(long = "next", allow_hyphen_values = true)]
        next_actions: Vec<String>,
        #[arg(long = "rule", allow_hyphen_values = true)]
        foundational_rules: Vec<String>,
        #[arg(long = "decision", allow_hyphen_values = true)]
        open_decisions: Vec<String>,
        #[arg(long, value_name = "PATH")]
        from_file: Option<String>,
        #[arg(long)]
        stdin: bool,
    },
    /// Show a handoff (--latest or --seq N), optionally as markdown
    Show {
        plan_id: String,
        selector: Option<String>,
        #[arg(long)]
        seq: Option<String>,
        #[arg(long)]
        latest: bool,
        #[arg(long)]
        markdown: bool,
    },
    /// List the handoffs attached to a plan
    List { plan_id: String },
}
