use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanLifecycleEvent, PlanStatus};

pub(in crate::service_plan) struct DeletedPlanRows {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    pub root: std::path::PathBuf,
    pub specs: Vec<(String, std::path::PathBuf)>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn import_plan_rows(
        &self,
        plan: &mut Plan,
        owned_spec: Option<&crate::plan::Spec>,
    ) -> Result<Option<CommittedRows>, PlanError> {
        let id = plan.id.clone();
        let now = plan.updated_at;
        let result=self.commit_entity_rows(&id,"persist_plan",now,|tx,operation_id| {
            let previous:Option<(u64,String,String)>=tx.query_row("SELECT updated_at,status,json_extract(header_json,'$.owner') FROM plans WHERE id=?1",[&id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(db_error)?;
            if previous.as_ref().is_some_and(|previous|previous.0>now) { return Ok(None); }
            if let Some((_,status,owner))=previous {
                let status:PlanStatus=serde_json::from_value(serde_json::Value::String(status)).map_err(json_error)?;
                if status!=plan.status {
                    let author=if plan.status==PlanStatus::Archived {serde_json::from_str::<OrchestratorRef>(&owner).map_err(json_error)?}else{plan.owner.clone()};
                    let kind=if plan.status==PlanStatus::Archived {"archived"}else{"activated"};
                    plan.lifecycle_events.push(PlanLifecycleEvent {id:format!("lifecycle-{kind}-{}",plan.lifecycle_events.len()),kind:kind.into(),at:now,author});
                }
            } else {crate::service_plan::store_index::record_lifecycle_events(plan,None,now);}
            let rows=store_rows::split_plan(serde_json::to_value(&*plan).map_err(json_error)?).map_err(PlanError::Io)?;
            store_rows::validate_row_sizes(&rows).map_err(PlanError::InvalidPlanText)?;
            let mut receipt=row_receipt(tx,&id,now,operation_id)?;
            receipt.serialized_bytes=rows.header.len()+rows.tasks.iter().map(|task|task.document.len()+task.milestones.iter().map(String::len).sum::<usize>()+task.attempts.iter().map(|attempt|attempt.document.len()).sum::<usize>()).sum::<usize>()+rows.sections.iter().chain(rows.elements.iter()).map(|row|row.document.len()).sum::<usize>()+rows.strides.iter().map(String::len).sum::<usize>()+rows.heartbeat.as_ref().map_or(0,String::len);
            let writes=super::super::upsert_plan(tx,plan,&rows,Some(receipt.revision))?;
            if let Some(spec)=owned_spec {spec_rows::write(tx,&mut spec.clone(),&mut receipt)?;}
            receipt.header_rows=1;receipt.task_rows=writes.tasks;receipt.attempts=writes.attempts;receipt.milestones=writes.milestones;receipt.strides=writes.strides;receipt.section_rows=writes.sections;receipt.element_rows=writes.elements;receipt.heartbeat_rows=writes.heartbeat;
            receipt.plan_metadata=Some(super::super::PlanIndexRow {id:id.clone(),title:plan.title.clone(),status:plan.status.clone(),team_id:plan.team_id.clone(),project_path:plan.project_path.clone(),updated_at:now});
            receipt.index_root=Some(plan.canonical_project_path.clone().into());
            Ok(Some(receipt))
        })?;
        if let Some(receipt) = &result {
            self.record_row_change(receipt);
        }
        Ok(result)
    }

    pub(in crate::service_plan) fn relocate_plan_rows(
        &self,
        tabs: &[String],
        teams: &[String],
        rebase: impl Fn(&str) -> Option<String>,
        now: u64,
    ) -> Result<(Vec<(CommittedRows, std::path::PathBuf)>, Vec<String>), PlanError> {
        let tabs = serde_json::to_string(tabs).map_err(json_error)?;
        let teams = serde_json::to_string(teams).map_err(json_error)?;
        let output=self.commit_entity_rows("workspace","relocate_plans",now,|tx,operation_id| {
            let candidates={let mut query=tx.prepare("SELECT id,project_path,coalesce(json_extract(header_json,'$.canonical_project_path'),'') FROM plans WHERE json_extract(header_json,'$.owner.tab_id') IN (SELECT value FROM json_each(?1)) OR team_id IN (SELECT value FROM json_each(?2)) ORDER BY id").map_err(db_error)?;
                let rows=query.query_map(params![tabs,teams],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?))).map_err(db_error)?;rows.collect::<Result<Vec<_>,_>>().map_err(db_error)?};
            let mut output=vec![];let mut warnings=vec![];
            for (id,mut project,mut canonical) in candidates {
                let old=if canonical.trim().is_empty(){project.clone()}else{canonical.clone()};
                let mut changed=false;
                for (label,path) in [("project",&mut project),("canonical project",&mut canonical)] {
                    if path.is_empty(){continue;}
                    if let Some(next)=rebase(path) {changed|=*path!=next;*path=next;}else{warnings.push(format!("Plan {id} {label}: {path} is outside the old workspace folder; unchanged"));}
                }
                if !changed {continue;}
                if canonical.trim().is_empty(){canonical=project.clone();}
                let mut receipt=row_receipt(tx,&id,now,operation_id)?;
                receipt.projected_text_bytes+=id.len()+project.len()+canonical.len()+old.len();
                receipt.header_rows+=tx.execute("UPDATE plans SET project_path=?2,header_json=json_set(header_json,'$.project_path',?2,'$.canonical_project_path',?3) WHERE id=?1",params![id,project,canonical]).map_err(db_error)?;
                touch_header(tx,&mut receipt)?;
                let (title,status,team):(String,String,Option<String>)=tx.query_row("SELECT title,status,team_id FROM plans WHERE id=?1",[&id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(db_error)?;
                receipt.projected_text_bytes+=title.len()+status.len()+team.as_ref().map_or(0,String::len);
                receipt.plan_metadata=Some(super::super::PlanIndexRow {id,title,status:serde_json::from_value(serde_json::Value::String(status)).map_err(json_error)?,team_id:team,project_path:project,updated_at:now});
                output.push((receipt,old.into()));
            }
            Ok((output,warnings))
        })?;
        for (receipt, _) in &output.0 {
            self.record_row_change(receipt);
        }
        Ok(output)
    }

    pub(in crate::service_plan) fn set_project_path_rows(
        &self,
        id: &str,
        project: &str,
        canonical: &str,
        now: u64,
    ) -> Result<
        (
            crate::service_plan::project_paths::PlanProjectUpdate,
            CommittedRows,
            std::path::PathBuf,
        ),
        PlanError,
    > {
        let mut update = None;
        let mut old_root = std::path::PathBuf::new();
        let receipt=self.hot_transaction(id,now,|tx,receipt| {
            let (old_project,old_canonical,previous_time):(String,String,u64)=tx.query_row("SELECT project_path,coalesce(json_extract(header_json,'$.canonical_project_path'),''),updated_at FROM plans WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(db_error)?;
            receipt.projected_text_bytes+=old_project.len()+old_canonical.len();
            old_root=if old_canonical.trim().is_empty(){old_project.clone()}else{old_canonical.clone()}.into();
            let change=crate::service_plan::project_paths::project_path_update(id,&old_project,&old_canonical,project,canonical);
            if change.changed {
                receipt.updated_at=now.max(previous_time);
                receipt.header_rows+=tx.execute("UPDATE plans SET project_path=?2,header_json=json_set(header_json,'$.project_path',?2,'$.canonical_project_path',?3) WHERE id=?1",params![id,project,canonical]).map_err(db_error)?;
                let (title,status,team):(String,String,Option<String>)=tx.query_row("SELECT title,status,team_id FROM plans WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(db_error)?;
                receipt.projected_text_bytes+=title.len()+status.len()+team.as_ref().map_or(0,String::len);
                receipt.plan_metadata=Some(super::super::PlanIndexRow {id:id.into(),title,status:serde_json::from_value(serde_json::Value::String(status)).map_err(json_error)?,team_id:team,project_path:project.into(),updated_at:receipt.updated_at});
            }
            update=Some(change);Ok(())
        })?;
        Ok((update.ok_or(PlanError::NotFound)?, receipt, old_root))
    }

    pub(in crate::service_plan) fn delete_plan_rows(
        &self,
        id: &str,
        cutoff: Option<u64>,
        now: u64,
    ) -> Result<Option<DeletedPlanRows>, PlanError> {
        let output=self.commit_entity_rows(id,"delete_plan",now,|tx,operation_id| {
            let metadata:Option<(String,u64,String)>=tx.query_row("SELECT title,updated_at,CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END FROM plans WHERE id=?1 AND (?2 IS NULL OR updated_at<?2)",params![id,cutoff],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(db_error)?;
            let Some((title,updated_at,root))=metadata else{return Ok(None);};
            let specs={let mut query=tx.prepare("SELECT id,CASE WHEN trim(coalesce(json_extract(row_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(row_json,'$.canonical_project_path') END FROM specs WHERE plan_id=?1 ORDER BY id").map_err(db_error)?;
                let rows=query.query_map([id],|row|Ok((row.get::<_,String>(0)?,std::path::PathBuf::from(row.get::<_,String>(1)?)))).map_err(db_error)?;rows.collect::<Result<Vec<_>,_>>().map_err(db_error)?};
            let previous=super::super::change_feed::revision(tx,id)?;let revision=super::super::change_feed::next_revision(previous,operation_id)?;
            tx.execute("UPDATE plan_operations SET state='committed',finished_at=?2 WHERE id=?1",params![operation_id,now]).map_err(db_error)?;
            super::super::operation_retention::prune_deleted_plan(tx,id)?;
            tx.execute("DELETE FROM plans WHERE id=?1",[id]).map_err(db_error)?;
            Ok(Some((DeletedPlanRows {id:id.into(),title,updated_at,root:root.into(),specs},previous,revision)))
        })?;
        if let Some((metadata, previous, revision)) = output {
            self.pending_changes
                .lock()
                .insert(super::super::change_feed::change(
                    id,
                    revision,
                    previous,
                    vec![],
                    true,
                    true,
                ));
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
