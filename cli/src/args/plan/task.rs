use clap::Subcommand;

#[derive(Subcommand)]
pub enum PlanTaskAction {
    /// List a plan's tasks, optionally filtered by status
    List {
        plan_id: Option<String>,
        #[arg(
            long,
            help = "Task status: pending | in_progress | in_review | blocked | paused | done | cancelled"
        )]
        status: Option<String>,
        #[arg(long, help = "Include paused, done, and cancelled tasks")]
        all: bool,
    },
    /// Show one task in the plan with full detail (body, criteria, test_plan, correctness, executor_role, sections)
    Show {
        plan_id: String,
        task_id: String,
        #[arg(long, help = "List sibling tasks (open-only unless --all)")]
        siblings: bool,
        #[arg(long, help = "With --siblings, include done/cancelled tasks")]
        all: bool,
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
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Routing rationale; stored as model_rationale in task JSON."
        )]
        why: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Routing rationale; stored as model_rationale in task JSON."
        )]
        why_file: Option<String>,
        #[arg(long = "criterion", allow_hyphen_values = true)]
        criterion: Vec<String>,
        #[arg(
            long = "criterion-file",
            value_name = "PATH",
            help = "Read acceptance criteria from a file: a JSON string array, or one criterion per line. Appends to --criterion"
        )]
        criterion_file: Option<String>,
        #[arg(
            long = "external-ref",
            value_name = "URL_OR_REPO#NUMBER",
            help = "GitHub issue reference; repeatable. Accepts a full issue URL or owner/repository#number"
        )]
        external_ref: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        test_plan: Option<String>,
        #[arg(long, value_name = "PATH")]
        test_plan_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        qa_plan: Option<String>,
        #[arg(long, value_name = "PATH")]
        qa_plan_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        correctness: Option<String>,
        #[arg(long, value_name = "PATH")]
        correctness_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Second-person brief: who the worker IS, what EXACTLY they do here, their RED LINE. e.g. \"You are Alex; property tests come before implementation; red line: a lock claim without a test is not proof.\""
        )]
        role_guidance: Option<String>,
        #[arg(long, value_name = "PATH")]
        role_guidance_file: Option<String>,
        #[arg(
            long = "executor-role",
            allow_hyphen_values = true,
            help = "REQUIRED: who executes this task. Specialization scoping what surface they own, e.g. rust-backend-dev, frontend-dev, docs-sync. Presence is the contract; no vocabulary is enforced"
        )]
        executor_role: String,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Executor persona name, e.g. Alex, Emma, Leo"
        )]
        personality: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(
            long,
            value_parser = parse_task_add_execution_intent,
            help = "Classifies whether this task changes the repository; dispatch uses read_only | mutating instead"
        )]
        execution_intent: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(
            long = "depends-on",
            value_name = "TASK_ID_OR_HANDLE",
            help = "Task this one waits on; repeatable. Accepts a full task id (task_xxxxxx), a unique id prefix, or a task handle (see --handle)"
        )]
        depends_on: Vec<String>,
        #[arg(long = "assign-tab", value_name = "TAB")]
        assign_tab: Option<String>,
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
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Routing rationale; stored as model_rationale in task JSON."
        )]
        why: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Routing rationale; stored as model_rationale in task JSON."
        )]
        why_file: Option<String>,
        #[arg(
            long,
            help = "Task status: pending | in_progress | in_review | blocked | paused | done | cancelled"
        )]
        status: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        status_label: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Second-person brief: who the worker IS, what EXACTLY they do here, their RED LINE. e.g. \"You are Alex; property tests come before implementation; red line: a lock claim without a test is not proof.\""
        )]
        role_guidance: Option<String>,
        #[arg(long, value_name = "PATH")]
        role_guidance_file: Option<String>,
        #[arg(
            long = "executor-role",
            allow_hyphen_values = true,
            help = "Replace the executor role. Cannot be emptied: an update carrying a blank value is refused"
        )]
        executor_role: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Executor persona name, e.g. Alex, Emma, Leo"
        )]
        personality: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long)]
        sections: Option<String>,
        #[arg(long = "criterion", allow_hyphen_values = true)]
        criterion: Vec<String>,
        #[arg(
            long = "criterion-file",
            value_name = "PATH",
            help = "Read acceptance criteria from a file: a JSON string array, or one criterion per line. Appends to --criterion"
        )]
        criterion_file: Option<String>,
        #[arg(
            long = "external-ref",
            value_name = "URL_OR_REPO#NUMBER",
            help = "GitHub issue reference to add; repeatable. Accepts a full issue URL or owner/repository#number"
        )]
        external_ref: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        test_plan: Option<String>,
        #[arg(long, value_name = "PATH")]
        test_plan_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        qa_plan: Option<String>,
        #[arg(long, value_name = "PATH")]
        qa_plan_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        correctness: Option<String>,
        #[arg(long, value_name = "PATH")]
        correctness_file: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(
            long = "depends-on",
            value_name = "TASK_ID_OR_HANDLE",
            conflicts_with = "clear_dependencies",
            help = "Add a dependency; repeatable. Appends to existing dependencies (dedup). Accepts a full task id (task_xxxxxx), a unique id prefix, or a task handle. Use --replace-dependencies for wholesale replacement"
        )]
        depends_on: Vec<String>,
        #[arg(
            long = "remove-dependency",
            value_name = "TASK_ID_OR_HANDLE",
            conflicts_with = "clear_dependencies",
            help = "Remove a specific dependency edge; repeatable. Applied after --depends-on additions"
        )]
        remove_dependency: Vec<String>,
        #[arg(
            long = "replace-dependencies",
            conflicts_with = "clear_dependencies",
            help = "Replace the whole dependency set with --depends-on instead of appending"
        )]
        replace_dependencies: bool,
        #[arg(long = "clear-dependencies")]
        clear_dependencies: bool,
        #[arg(long = "tab", value_name = "TAB")]
        tab: Option<String>,
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
    /// Close a task from any lifecycle state. The authenticated Plan Orchestrator overrides worker owner-epoch drift; open subtasks still block
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
    /// Pause a task (explicit operator hold outside the heartbeat open-work set; requires --reason)
    Pause {
        plan_id: String,
        task_id: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        reason: String,
    },
    /// Resume a paused task (returns it to dependency-waiting pending)
    Resume {
        plan_id: String,
        task_id: Option<String>,
    },
    /// Assign a task to a tab (exports CODETERM_TASK_ID to that tab)
    Assign {
        plan_id: String,
        task_id: Option<String>,
        #[arg(long = "tab", value_name = "TAB")]
        tab: String,
        #[arg(long, alias = "peer")]
        mesh: Option<String>,
    },
    /// Spawn a worker, assign the task, and deliver its brief in one atomic call
    Dispatch {
        plan_id: String,
        task_id: Option<String>,
        #[arg(long, help = "Agent provider to spawn (e.g. claude, codex, opencode)")]
        agent: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        model: Option<String>,
        #[arg(
            long = "reasoning-effort",
            help = "Provider-supported reasoning effort (rejected if unsupported)"
        )]
        reasoning_effort: Option<String>,
        #[arg(
            long = "parent-tab",
            help = "Parent tab override (defaults to the caller tab)"
        )]
        parent: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(
            long = "execution-intent",
            value_parser = ["read_only", "mutating"],
            help = "Explicit task intent used by provider reserve policy"
        )]
        execution_intent: String,
        #[arg(
            long = "provider-reserve-pct",
            default_value_t = 5.0,
            help = "Minimum remaining provider allowance for mutating work"
        )]
        provider_reserve_pct: f32,
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
    /// Adopt a sealed handoff checkpoint into this authenticated Worker tab
    AdoptCheckpoint { plan_id: String, task_id: String },
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

fn parse_task_add_execution_intent(value: &str) -> Result<String, String> {
    match value {
        "repository_mutation" | "non_repository" => Ok(value.to_string()),
        _ => Err(format!(
            "invalid execution intent for `plan task add`; use repository_mutation or non_repository. `{value}` belongs to `plan task dispatch` (read_only | mutating)."
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::CommandFactory;

    fn depends_on_help(subcommand: &str) -> String {
        Cli::command()
            .find_subcommand_mut("plan")
            .unwrap()
            .find_subcommand_mut("task")
            .unwrap()
            .find_subcommand_mut(subcommand)
            .unwrap()
            .render_long_help()
            .to_string()
    }

    #[test]
    fn task_add_help_documents_depends_on_value_types() {
        let help = depends_on_help("add");
        assert!(help.contains("--depends-on"), "{help}");
        assert!(help.contains("full task id"), "{help}");
        assert!(help.contains("handle"), "{help}");
    }

    #[test]
    fn task_update_help_documents_depends_on_value_types() {
        let help = depends_on_help("update");
        assert!(help.contains("--depends-on"), "{help}");
        assert!(help.contains("full task id"), "{help}");
        assert!(help.contains("handle"), "{help}");
    }

    #[test]
    fn task_add_and_update_help_name_model_rationale_storage() {
        for subcommand in ["add", "update"] {
            let help = depends_on_help(subcommand);
            assert!(help.contains("model_rationale"), "{subcommand}: {help}");
        }
    }

    #[test]
    fn task_add_and_update_help_document_external_ref_forms() {
        for subcommand in ["add", "update"] {
            let help = depends_on_help(subcommand);
            assert!(help.contains("--external-ref"), "{subcommand}: {help}");
            assert!(help.contains("full issue URL"), "{subcommand}: {help}");
            assert!(
                help.contains("owner/repository#number"),
                "{subcommand}: {help}"
            );
        }
    }
}
