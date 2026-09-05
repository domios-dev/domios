//! The plan's north-star goal: the whole-value write and the field-wise
//! update that keeps what it was not passed.

use super::*;

/// A goal edit as the caller expressed it: `None` means "not mentioned", which
/// is kept, and `Some` means "set to this", including to an empty value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GoalPatch {
    pub vector: Option<String>,
    pub vision: Option<String>,
    pub constraints: Option<Vec<String>>,
    pub postulates: Option<Vec<String>>,
}

impl PlanStore {
    pub fn set_goal(&self, plan_id: &str, goal: Goal, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.goal = Some(goal);
            Ok(())
        })
    }

    /// Field-wise goal update: a `None` field keeps what the plan already holds.
    /// Wholesale replacement is reachable by naming every field; it is not what
    /// omitting one means, because a caller setting the vector alone was silently
    /// destroying the constraints, postulates and vision it never mentioned.
    pub fn patch_goal(
        &self,
        plan_id: &str,
        patch: GoalPatch,
        now_ms: u64,
    ) -> Result<Goal, PlanError> {
        let mut updated = None;
        self.with_plan(plan_id, now_ms, |p| {
            let mut goal = p.goal.clone().unwrap_or_default();
            if let Some(vector) = patch.vector.clone() {
                goal.vector = vector;
            }
            if let Some(vision) = patch.vision.clone() {
                goal.vision = vision;
            }
            if let Some(constraints) = patch.constraints.clone() {
                goal.constraints = constraints;
            }
            if let Some(postulates) = patch.postulates.clone() {
                goal.postulates = postulates;
            }
            if goal.vector.trim().is_empty() {
                return Err(PlanError::GoalVectorRequired);
            }
            updated = Some(goal.clone());
            p.goal = Some(goal);
            Ok(())
        })?;
        updated.ok_or(PlanError::NotFound)
    }
}
