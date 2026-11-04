use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GoalPatch {
    pub vector: Option<String>,
    pub vision: Option<String>,
    pub constraints: Option<Vec<String>>,
    pub postulates: Option<Vec<String>>,
}

impl PlanStore {
    pub fn set_goal(&self, plan_id: &str, goal: Goal, now_ms: u64) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let (_, receipt) =
            self.database
                .change_goal(plan_id, Some(goal), GoalPatch::default(), now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(())
    }

    pub fn patch_goal(
        &self,
        plan_id: &str,
        patch: GoalPatch,
        now_ms: u64,
    ) -> Result<Goal, PlanError> {
        let turn = self.mutation_writer.enter();
        let (goal, receipt) = self.database.change_goal(plan_id, None, patch, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(goal)
    }
}
