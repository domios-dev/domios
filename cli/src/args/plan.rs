use clap::Subcommand;

#[derive(Subcommand)]
pub enum PlanAction {
    /// Create a new plan
    Create {
        #[arg(long, allow_hyphen_values = true)]
        title: String,
        #[arg(long, allow_hyphen_values = true)]
        spec: Option<String>,
        #[arg(long, value_name = "PATH")]
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
    /// Get a plan (markdown by default)
    Get { plan_id: String },
    /// Print only the hydrated section prose
    Spec { plan_id: String },
    /// List generated section/task body files
    Files { plan_id: String },
    /// Show tag facets, counts, topics, and duplicate warnings
    Tags { plan_id: String },
    /// Show section coverage (% + gaps)
    Coverage { plan_id: String },
    /// Show sections with zero tasks
    Gaps { plan_id: String },
    /// Set task groups (phases)
    Groups {
        #[command(subcommand)]
        action: PlanGroupsAction,
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
    },
    /// Archive a plan
    Archive { plan_id: String },
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
pub enum PlanSectionAction {
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
    Rm {
        plan_id: String,
        section_id: String,
    },
}

#[derive(Subcommand)]
pub enum PlanKindsAction {
    Set {
        plan_id: String,
        #[arg(long)]
        kinds: String,
    },
}

#[derive(Subcommand)]
pub enum PlanGroupsAction {
    Set {
        plan_id: String,
        #[arg(long)]
        groups: String,
    },
}

#[derive(Subcommand)]
pub enum PlanLoopAction {
    Create {
        plan_id: String,
        #[arg(long, allow_hyphen_values = true)]
        target: String,
        #[arg(long)]
        budget: u32,
        #[arg(long, allow_hyphen_values = true)]
        stop_when: Option<String>,
    },
    Set {
        loop_id: String,
        #[arg(long)]
        budget: Option<u32>,
        #[arg(long, allow_hyphen_values = true)]
        stop_when: Option<String>,
    },
    Status {
        loop_id: String,
    },
    Stop {
        loop_id: String,
    },
}

#[derive(Subcommand)]
pub enum PlanTaskAction {
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
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        assign_pane: Option<String>,
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
        loop_from: Option<String>,
    },
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
        #[arg(long)]
        status: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        status_label: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long)]
        parent: Option<String>,
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
    Assign {
        plan_id: String,
        task_id: String,
        #[arg(long)]
        pane: String,
        #[arg(long, alias = "peer")]
        mesh: Option<String>,
    },
    Rm {
        plan_id: String,
        task_id: String,
    },
}

#[derive(Subcommand)]
pub enum PlanMilestoneAction {
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
    Rm {
        plan_id: String,
        task_id: String,
        milestone_id: String,
    },
}

#[derive(Subcommand)]
pub enum PlanLayoutAction {
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
}

#[derive(Subcommand)]
pub enum PlanElementAction {
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
}
