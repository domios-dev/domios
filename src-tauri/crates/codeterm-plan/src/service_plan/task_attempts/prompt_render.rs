use super::*;

fn lane_lines(lane: &TaskLaneMetadata) -> Vec<String> {
    [
        ("worktree", lane.worktree_path.as_deref()),
        ("branch", lane.branch.as_deref()),
        ("base", lane.base.as_deref()),
        ("destination", lane.destination.as_deref()),
        ("workspace", lane.workspace_id.as_deref()),
        ("workspace_name", lane.workspace_name.as_deref()),
    ]
    .into_iter()
    .filter_map(|(name, value)| value.map(|v| format!("- {name}: {v}")))
    .collect()
}

#[cfg(test)]
pub(in crate::service_plan) fn build_prompt(
    plan_id: &str,
    task: &Task,
    source: &TaskExecutionAttempt,
    provider: &str,
    model: Option<&str>,
    report_command: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    build_prompt_read(
        plan_id,
        &TaskDraftPromptRead::from(task),
        source,
        provider,
        model,
        report_command,
        manifest,
        authorized_gates,
    )
}

pub(in crate::service_plan) fn build_prompt_read<'a>(
    plan_id: &str,
    task: impl Into<super::prompt_read::WorkerPrompt<'a>>,
    source: &TaskExecutionAttempt,
    provider: &str,
    model: Option<&str>,
    report_command: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
    authorized_gates: &[String],
) -> String {
    let task = task.into();
    let identity = super::super::worker_identity::compose_worker_identity_contract(
        task.personality,
        task.role_guidance,
        task.executor_role,
        task.name,
    );
    let (last_report, last_status, _) = last_report_for_milestones(task.milestones, source);
    let mut out = String::new();
    out.push_str("You are a CodeTerm Worker respawned from a prior task attempt.\n");
    out.push_str(
        "Work only on this one task. Do not reactivate or reuse the old historical attempt.\n\n",
    );
    out.push_str(&format!("Plan: {plan_id}\nTask: {}\n", task.id));
    out.push_str(&super::super::task_brief::render_plan_skill_manifest(
        plan_id, manifest,
    ));
    out.push_str(&super::super::task_brief::render_authorized_gates(
        authorized_gates,
    ));
    out.push_str(&format!("Task name: {}\n", task.name));
    out.push_str(&format!("Provider/model preference: {provider}"));
    if let Some(model) = model {
        out.push_str(&format!("/{model}"));
    }
    out.push('\n');
    if let Some(strides) = super::super::stride_context::render_worker_strides(task.milestones) {
        out.push('\n');
        out.push_str(&strides);
    }
    out.push_str("\nOriginal task body:\n");
    if task.body.trim().is_empty() {
        out.push_str("(no body recorded)\n");
    } else {
        out.push_str(task.body.trim());
        out.push('\n');
    }
    if let Some(intent) = task.intent.filter(|s| !s.trim().is_empty()) {
        out.push_str("\nIntent:\n");
        out.push_str(intent.trim());
        out.push('\n');
    }
    if !task.acceptance_criteria.is_empty() {
        out.push_str("\nAcceptance criteria:\n");
        for criterion in task.acceptance_criteria {
            out.push_str("- ");
            out.push_str(criterion);
            out.push('\n');
        }
    }
    if let Some(correctness) = task
        .correctness
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nCorrectness:\n");
        out.push_str(correctness);
        out.push('\n');
    }
    if let Some(test_plan) = task
        .test_plan
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nTest plan:\n");
        out.push_str(test_plan);
        out.push('\n');
    }
    let lane = lane_lines(task.lane);
    if !lane.is_empty() {
        out.push_str("\nBranch/worktree hints:\n");
        out.push_str(&lane.join("\n"));
        out.push('\n');
    }
    out.push_str("\nPrior attempt:\n");
    out.push_str(&format!(
        "- attempt_id: {}\n- executor_tab_id: {}\n- status: {}\n",
        source.id,
        source.executor_tab_id,
        attempt_status(source)
    ));
    if let Some(status) = last_status {
        out.push_str(&format!("- last_report_status: {status}\n"));
    }
    if let Some(report) = last_report {
        out.push_str("\nLast report:\n");
        out.push_str(report.trim());
        out.push('\n');
    }
    out.push_str("\nWhen done, run exactly:\n");
    out.push_str(report_command);
    out.push('\n');
    super::super::worker_identity::attach_worker_identity(&identity, &out)
}

pub(in crate::service_plan) fn build_compete_prompt(
    plan_id: &str,
    task: &TaskDraftPromptRead,
    group_id: &str,
    lane: u32,
    model_label: &str,
    report_command: &str,
    manifest: &[crate::plan::PlanSkillManifestEntry],
) -> String {
    let identity = super::super::worker_identity::compose_worker_identity_contract(
        task.personality.as_deref(),
        task.role_guidance.as_deref(),
        task.executor_role.as_deref(),
        &task.name,
    );
    let mut out = String::new();
    out.push_str(crate::service_plan::competition::COMPETITION_WORKER_ISOLATION_PROMPT);
    out.push_str("\n\n");
    out.push_str(&format!("Plan: {plan_id}\nTask: {}\n", task.id));
    out.push_str(&super::super::task_brief::render_plan_skill_manifest(
        plan_id, manifest,
    ));
    out.push_str(&format!(
        "Competition group: {group_id}\nAttempt lane: {lane}\n"
    ));
    out.push_str(&format!("Model: {model_label}\n"));
    out.push_str(&format!("Task name: {}\n", task.name));
    if let Some(hint) = task
        .competition
        .as_ref()
        .and_then(|competition| competition.judge_hint.as_deref())
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("\nJudge hint:\n");
        out.push_str(hint.trim());
        out.push('\n');
    }
    if let Some(strides) = super::super::stride_context::render_worker_strides(&task.milestones) {
        out.push('\n');
        out.push_str(&strides);
    }
    out.push_str("\nTask body:\n");
    if task.body.trim().is_empty() {
        out.push_str("(no body recorded)\n");
    } else {
        out.push_str(task.body.trim());
        out.push('\n');
    }
    if let Some(intent) = task.intent.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("\nIntent:\n");
        out.push_str(intent.trim());
        out.push('\n');
    }
    if !task.acceptance_criteria.is_empty() {
        out.push_str("\nAcceptance criteria:\n");
        for criterion in &task.acceptance_criteria {
            out.push_str("- ");
            out.push_str(criterion);
            out.push('\n');
        }
    }
    if let Some(correctness) = task
        .correctness
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nCorrectness:\n");
        out.push_str(correctness);
        out.push('\n');
    }
    if let Some(test_plan) = task
        .test_plan
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("\nTest plan:\n");
        out.push_str(test_plan);
        out.push('\n');
    }
    let lane_lines = lane_lines(&task.lane);
    if !lane_lines.is_empty() {
        out.push_str("\nBase lane hints:\n");
        out.push_str(&lane_lines.join("\n"));
        out.push('\n');
    }
    out.push_str("\nWhen done, run exactly:\n");
    out.push_str(report_command);
    out.push('\n');
    super::super::worker_identity::attach_worker_identity(&identity, &out)
}
