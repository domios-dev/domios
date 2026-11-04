use super::header_edits::{append_field, read_field};
use super::*;
use crate::plan::{OrchestratorRef, PlanLifecycleEvent, PlanMode, PlanStatus};

#[derive(Default)]
pub(in crate::service_plan) struct PlanEdit {
    pub title: Option<String>,
    pub status: Option<PlanStatus>,
    pub mode: Option<PlanMode>,
    pub unarchive: bool,
    pub incident: Option<Milestone>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn edit_plan_state(
        &self,
        plan_id: &str,
        edit: PlanEdit,
        now_ms: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let (title, status, team_id, project_path): (String, String, Option<String>, String) = tx.query_row("SELECT title,status,team_id,project_path FROM plans WHERE id=?1", [plan_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).map_err(db_error)?;
            receipt.projected_text_bytes += title.len() + status.len() + team_id.as_ref().map_or(0, String::len) + project_path.len();
            let previous: PlanStatus = serde_json::from_value(serde_json::Value::String(status)).map_err(json_error)?;
            let status = edit.status.unwrap_or_else(|| previous.clone());
            if status == PlanStatus::Active && previous == PlanStatus::Archived && !edit.unarchive {
                return Err(PlanError::PlanActivationRequiresUnarchive { plan_id: plan_id.into() });
            }
            if edit.unarchive {
                let team = team_id.as_deref().filter(|team| !team.trim().is_empty());
                if !cfg!(test) && team.is_none() { return Err(PlanError::ActivePlanRequiresTeam { plan_id: plan_id.into() }); }
                if let Some(team) = team {
                    let existing: Option<(String, String)> = tx.query_row("SELECT id,title FROM plans WHERE team_id=?1 AND status='active' AND id<>?2 LIMIT 1", params![team, plan_id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(db_error)?;
                    if let Some((existing_plan_id, existing_title)) = existing {
                        return Err(PlanError::TeamAlreadyHasActivePlan { team_id: team.into(), existing_plan_id, existing_title });
                    }
                }
            }
            if previous != status {
                let owner: OrchestratorRef = read_field(tx, plan_id, "$.owner", receipt)?;
                let count: u64 = tx.query_row("SELECT coalesce(json_array_length(header_json,'$.lifecycle_events'),0) FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
                let kind = if status == PlanStatus::Active { "activated" } else { "archived" };
                append_field(tx, plan_id, "$.lifecycle_events", &PlanLifecycleEvent { id: format!("lifecycle-{kind}-{count}"), kind: kind.into(), at: now_ms, author: owner }, receipt)?;
            }
            if let Some(incident) = edit.incident { append_stride(tx, plan_id, &incident, receipt)?; }
            let title = edit.title.unwrap_or(title);
            let mut patch = serde_json::json!({"title": title, "status": status});
            if let Some(mode) = edit.mode { patch["mode"] = serde_json::to_value(mode).map_err(json_error)?; }
            let document = encode(&patch, receipt)?;
            receipt.header_rows += tx.execute("UPDATE plans SET title=json_extract(?2,'$.title'),status=json_extract(?2,'$.status'),header_json=json_patch(header_json,json(?2)) WHERE id=?1", params![plan_id, document]).map_err(db_error)?;
            receipt.plan_metadata = Some(super::super::PlanIndexRow { id: plan_id.into(), title, status, team_id, project_path, updated_at: now_ms });
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
