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
