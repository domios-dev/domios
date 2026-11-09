use super::{Task, TaskDraftPromptRead};

impl From<&Task> for TaskDraftPromptRead {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id.clone(),
            name: task.name.clone(),
            status: task.status.clone(),
            body: task.body.clone(),
            execution_intent: task.interruption.execution_intent.clone(),
            model: task.model.clone(),
            speed: task.speed.clone(),
            personality: task.personality.clone(),
            role_guidance: task.role_guidance.clone(),
            executor_role: task.executor_role.clone(),
            intent: task.intent.clone(),
            correctness: task.correctness.clone(),
            test_plan: task.test_plan.clone(),
            acceptance_criteria: task.acceptance_criteria.clone(),
            lane: task.lane.clone(),
            competition: task.competition.clone(),
            milestones: task.milestones.clone(),
        }
    }
}

pub(in crate::service_plan) struct WorkerPrompt<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub body: &'a str,
    pub personality: Option<&'a str>,
    pub role_guidance: Option<&'a str>,
    pub executor_role: Option<&'a str>,
    pub intent: Option<&'a str>,
    pub correctness: Option<&'a str>,
    pub test_plan: Option<&'a str>,
    pub acceptance_criteria: &'a [String],
    pub lane: &'a crate::plan::TaskLaneMetadata,
    pub milestones: &'a [crate::plan::Milestone],
}

impl<'a> From<&'a TaskDraftPromptRead> for WorkerPrompt<'a> {
    fn from(task: &'a TaskDraftPromptRead) -> Self {
        Self {
            id: &task.id,
            name: &task.name,
            body: &task.body,
            personality: task.personality.as_deref(),
            role_guidance: task.role_guidance.as_deref(),
            executor_role: task.executor_role.as_deref(),
            intent: task.intent.as_deref(),
            correctness: task.correctness.as_deref(),
            test_plan: task.test_plan.as_deref(),
            acceptance_criteria: &task.acceptance_criteria,
            lane: &task.lane,
            milestones: &task.milestones,
        }
    }
}

impl<'a>
    From<(
        &'a super::read::TaskAttemptPromptRead,
        &'a [crate::plan::Milestone],
    )> for WorkerPrompt<'a>
{
    fn from(
        (task, milestones): (
            &'a super::read::TaskAttemptPromptRead,
            &'a [crate::plan::Milestone],
        ),
    ) -> Self {
        Self {
            id: &task.id,
            name: &task.name,
            body: &task.body,
            personality: task.personality.as_deref(),
            role_guidance: task.role_guidance.as_deref(),
            executor_role: task.executor_role.as_deref(),
            intent: task.intent.as_deref(),
            correctness: task.correctness.as_deref(),
            test_plan: task.test_plan.as_deref(),
            acceptance_criteria: &task.acceptance_criteria,
            lane: &task.lane,
            milestones,
        }
    }
}
