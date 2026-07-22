use clap::Subcommand;

#[derive(Subcommand)]
#[allow(
    clippy::large_enum_variant,
    reason = "Clap command enums keep parsed options inline."
)]
pub enum TaskAction {
    /// List tasks across active plans
    List {
        #[arg(long)]
        active: bool,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        mine: bool,
        #[arg(long)]
        unassigned: bool,
        #[arg(long)]
        workspace: bool,
        #[arg(long)]
        plan: Option<String>,
        #[arg(
            long,
            help = "Task status: pending | in_progress | in_review | blocked | paused | done | cancelled"
        )]
        status: Option<String>,
    },
    /// Show one task with plan and section context
    Show {
        task_id: String,
        #[arg(long, help = "List sibling tasks (open-only unless --all)")]
        siblings: bool,
        #[arg(long, help = "With --siblings, include done/cancelled tasks")]
        all: bool,
    },
    /// Assign the task to the caller's tab
    Take { task_id: String },
    /// Release the task assignment
    Drop { task_id: String },
    /// Add a standalone task to the workspace Inbox
    Add {
        #[arg(long, allow_hyphen_values = true)]
        name: String,
        #[arg(long = "type", allow_hyphen_values = true)]
        kind: Option<String>,
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
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long = "assign-tab", value_name = "TAB")]
        assign_pane: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CanvasAction {
    /// Focus the open canvas on a task, tab, or plan
    Focus {
        #[arg(long, conflicts_with_all = ["tab", "plan"])]
        task: Option<String>,
        #[arg(long = "tab", value_name = "TAB", conflicts_with_all = ["task", "plan"])]
        pane: Option<String>,
        #[arg(long, conflicts_with_all = ["task", "tab"])]
        plan: Option<String>,
    },
    /// Dim the canvas to a facet value such as area:layout
    Filter {
        #[arg(long)]
        facet: String,
    },
}
