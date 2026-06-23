use clap::Subcommand;

#[derive(Subcommand)]
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
    },
    /// Show one task with plan and section context
    Show { task_id: String },
    /// Assign the task to the caller's pane
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
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long)]
        assign_pane: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CanvasAction {
    /// Focus the open canvas on a task, pane, or plan
    Focus {
        #[arg(long, conflicts_with_all = ["pane", "plan"])]
        task: Option<String>,
        #[arg(long, conflicts_with_all = ["task", "plan"])]
        pane: Option<String>,
        #[arg(long, conflicts_with_all = ["task", "pane"])]
        plan: Option<String>,
    },
}
