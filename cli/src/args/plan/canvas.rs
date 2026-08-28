use clap::Subcommand;

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
pub enum PlanQaAction {
    /// Show the plan QA queue, or enqueue one in_review task with --task
    Queue {
        plan_id: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        note: Option<String>,
    },
    /// Show the next queued QA task, claim it, or record a group's full-gate regression run
    Next {
        plan_id: Option<String>,
        #[arg(long)]
        claim: bool,
        /// Record ONE group-scoped regression working point instead of claiming a task
        #[arg(long)]
        group: Option<String>,
        /// Claim note. Required with --claim.
        #[arg(long, allow_hyphen_values = true, required_if_eq("claim", "true"))]
        note: Option<String>,
    },
    /// Mark QA passed and close the task
    Pass {
        plan_id: Option<String>,
        task_id: String,
        #[arg(long, allow_hyphen_values = true)]
        note: String,
    },
    /// Mark QA failed, bounce to in_progress, and nudge the implementer
    Fail {
        plan_id: Option<String>,
        task_id: String,
        #[arg(long, allow_hyphen_values = true)]
        note: String,
        /// This attempt changed executable implementation surfaces
        #[arg(long)]
        changed_executable_surfaces: bool,
        /// This attempt used a materially distinct verification oracle
        #[arg(long)]
        distinct_oracle: bool,
    },
    /// Authorized Orchestrator waiver of pending regression gates (owner-only, audited)
    Override {
        plan_id: Option<String>,
        /// Gate keys to waive; defaults to every currently blocking gate
        #[arg(long, value_delimiter = ',')]
        gate: Vec<String>,
        /// Why the frontier may advance past the gate — required
        #[arg(long, allow_hyphen_values = true)]
        reason: String,
    },
}

#[derive(Subcommand)]
pub enum PlanReviewAction {
    /// Show review-backpressure status: WIP count/limit, credit, blocked reason
    Status { plan_id: Option<String> },
    /// Show batch-compatible QA waves (grouped by task group)
    Waves { plan_id: Option<String> },
    /// Record a P0 exception authorizing the next over-round-limit review entry
    Override {
        plan_id: Option<String>,
        task_id: String,
        #[arg(long)]
        owner: String,
        #[arg(long, allow_hyphen_values = true)]
        reason: String,
        #[arg(long, allow_hyphen_values = true)]
        reproduction: String,
    },
    /// Repair historical review-credit debt from leaked review exits and audit it
    ReconcileCredit {
        plan_id: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        reason: String,
    },
    /// Grant daemon-owned review-capacity provenance to one task
    AuthorizeCapacity {
        plan_id: Option<String>,
        task_id: String,
        #[arg(long, value_parser = ["review", "canonical_gate"])]
        class: String,
    },
    /// ACCEPT: attaches the verdict and closes the task (in_review -> done)
    Accept {
        plan_id: Option<String>,
        task_id: String,
        #[arg(long, allow_hyphen_values = true)]
        note: String,
        #[arg(long)]
        changed_executable_surfaces: bool,
        #[arg(long)]
        distinct_oracle: bool,
        /// Past the default round bound, routes residual risk to a follow-up task
        #[arg(long, allow_hyphen_values = true)]
        residual_risk: Option<String>,
    },
    /// BOUNCE: atomically exits in_review back to in_progress
    Bounce {
        plan_id: Option<String>,
        task_id: String,
        #[arg(long, allow_hyphen_values = true)]
        note: String,
        #[arg(long)]
        changed_executable_surfaces: bool,
        #[arg(long)]
        distinct_oracle: bool,
    },
    /// HOLD: records a review hold without releasing the task assignment or lane
    Hold {
        plan_id: Option<String>,
        task_id: String,
        #[arg(long, allow_hyphen_values = true)]
        note: String,
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

#[derive(Subcommand)]
pub enum PlanDecisionAction {
    /// Create a decision gate, pause blocking tasks, and route the question to the planner
    Ask {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        question: String,
        #[arg(long, allow_hyphen_values = true, default_value = "")]
        context: String,
        /// Task ids (or handles) to pause until this gate is answered
        #[arg(long = "block")]
        blocking_task_ids: Vec<String>,
        #[arg(long = "asked-by-tab")]
        asked_by_tab: Option<String>,
    },
    /// Answer an open gate (immutable audit record, resumes paused tasks)
    Answer {
        plan_id: String,
        gate_id: String,
        #[arg(long, allow_hyphen_values = true)]
        answer: String,
        #[arg(long = "answered-by-tab")]
        answered_by_tab: Option<String>,
    },
    /// Acknowledge one durably delivered Planner request
    #[command(hide = true)]
    AckRequest { plan_id: String, request_id: String },
    /// List decision gates for a plan
    List {
        plan_id: String,
        /// Show only open gates
        #[arg(long)]
        open: bool,
    },
    /// Escalate a stale open gate
    Escalate {
        plan_id: String,
        gate_id: String,
        #[arg(long, allow_hyphen_values = true)]
        note: Option<String>,
    },
}
