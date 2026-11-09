use super::*;
use crate::plan::Goal;
use crate::service_plan::mutations::goal_ops::GoalPatch;

impl PlanDatabase {
    pub(in crate::service_plan) fn change_goal(
        &self,
        plan_id: &str,
        replacement: Option<Goal>,
        patch: GoalPatch,
        now_ms: u64,
    ) -> Result<(Goal, CommittedRows), PlanError> {
        let mut updated = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let goal = if let Some(goal) = replacement {
                goal
            } else {
                let json: String = tx.query_row(
                    "SELECT coalesce(json_extract(header_json, '$.goal'), 'null') FROM plans WHERE id=?1",
                    [plan_id],
                    |row| row.get(0),
                ).map_err(db_error)?;
                receipt.decoded_bytes += json.len();
                let mut goal = serde_json::from_str::<Option<Goal>>(&json)
                    .map_err(json_error)?.unwrap_or_default();
                if let Some(vector) = patch.vector { goal.vector = vector; }
                if let Some(vision) = patch.vision { goal.vision = vision; }
                if let Some(constraints) = patch.constraints { goal.constraints = constraints; }
                if let Some(postulates) = patch.postulates { goal.postulates = postulates; }
                if goal.vector.trim().is_empty() {
                    return Err(PlanError::GoalVectorRequired);
                }
                goal
            };
            let document = encode(&goal, receipt)?;
            receipt.header_rows += tx.execute(
                "UPDATE plans SET header_json=json_set(header_json, '$.goal', json(?2)) WHERE id=?1",
                params![plan_id, document],
            ).map_err(db_error)?;
            updated = Some(goal);
            Ok(())
        })?;
        Ok((updated.ok_or(PlanError::NotFound)?, receipt))
    }
}

#[cfg(test)]
mod tests;
