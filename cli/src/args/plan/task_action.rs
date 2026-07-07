use clap::Subcommand;

#[derive(Subcommand)]
pub enum PlanTaskAction {
    /// List a plan's tasks, optionally filtered by status
    List {
        plan_id: Option<String>,
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
        #[arg(long = "executor-role", allow_hyphen_values = true)]
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
        plan_id: Option<String>,
        task_id: Option<String>,
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
        #[arg(long = "executor-role", allow_hyphen_values = true)]
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
        task_id: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        result: Option<String>,
        #[arg(long, value_name = "PATH")]
        result_file: Option<String>,
        #[arg(long)]
        mesh: Option<String>,
    },
    /// Park a task (defers it out of the heartbeat open-work set; requires --reason)
    Park {
        plan_id: String,
        task_id: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        reason: String,
    },
    /// Unpark a task (returns it to pending)
    Unpark {
        plan_id: String,
        task_id: Option<String>,
    },
    /// Assign a task to a pane (exports CODETERM_TASK_ID to that pane)
    Assign {
        plan_id: String,
        task_id: Option<String>,
        #[arg(long)]
        pane: String,
        #[arg(long, alias = "peer")]
        mesh: Option<String>,
    },
    /// Spawn a worker, assign the task, and deliver its brief in one atomic call
    Dispatch {
        plan_id: String,
        task_id: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        model: Option<String>,
        #[arg(long, help = "Parent pane override (defaults to the caller pane)")]
        parent: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
    },
    /// List execution attempts for a plan task
    Attempts { plan_id: String, task_id: String },
    /// Show one execution attempt for a plan task
    Attempt {
        plan_id: String,
        task_id: String,
        attempt_id: String,
    },
    /// Attach a file to a plan task
    Attach {
        plan_id: String,
        task_id: String,
        #[arg(value_name = "PATH")]
        file: String,
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
