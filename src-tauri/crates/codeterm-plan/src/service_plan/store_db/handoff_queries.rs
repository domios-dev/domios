use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;
use rusqlite::{params, OptionalExtension};

pub(in crate::service_plan) struct HandoffResumeCandidateRead {
    pub plan_id: String,
    pub handoff_id: String,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct HandoffCreateRead {
    pub id: String,
    pub title: String,
    pub project_path: String,
    pub updated_at: u64,
    pub done: usize,
    pub total: usize,
    #[serde(skip)]
    pub seq: u64,
    #[serde(skip)]
    pub in_flight: Vec<HandoffTaskRead>,
    #[serde(skip)]
    pub decisions: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(in crate::service_plan) struct HandoffTaskRead {
    pub task_id: String,
    pub task_handle: String,
    pub status: crate::plan::TaskStatus,
    pub assigned_tab_id: Option<String>,
    pub peer: Option<String>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn handoff_create_input(
        &self,
        id: &str,
        include_decisions: bool,
    ) -> Result<HandoffCreateRead, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        record_query();
        let (header,sequence):(String,String)=connection.query_row("SELECT json_object('id',id,'title',title,'project_path',project_path,'updated_at',json(header_json->'$.updated_at'),
            'total',(SELECT count(*) FROM plan_tasks WHERE plan_id=?1),'done',(SELECT count(*) FROM plan_tasks WHERE plan_id=?1 AND status IN ('done','cancelled'))),
            coalesce((SELECT max(domios_handoff_seq(row_json->'$.content.seq')) FROM plan_elements WHERE plan_id=?1 AND json_extract(row_json,'$.kind')='handoff'),'0') FROM plans WHERE id=?1",
            [id],|row|Ok((column(row,0)?,column(row,1)?))).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let mut input: HandoffCreateRead = decode(id, &header)?;
        input.seq = sequence
            .parse::<u64>()
            .map_err(|error| PlanError::Io(error.to_string()))?
            .checked_add(1)
            .ok_or_else(|| PlanError::Io("handoff sequence exhausted".into()))?;
        record_query();
        let mut statement=connection.prepare_cached("SELECT json_object('task_id',task_id,'task_handle',CASE WHEN json_type(row_json,'$.handle') IS NULL OR json_type(row_json,'$.handle')='null' THEN task_id ELSE json_extract(row_json,'$.handle') END,
            'status',status,'assigned_tab_id',json_extract(row_json,'$.assigned_tab_id'),'peer',json_extract(row_json,'$.peer')) FROM plan_tasks WHERE plan_id=?1 AND status NOT IN ('done','cancelled') ORDER BY position").map_err(db_error)?;
        input.in_flight = statement
            .query_map([id], |row| column::<String>(row, 0))
            .map_err(db_error)?
            .map(|row| decode(id, &row.map_err(db_error)?))
            .collect::<Result<_, _>>()?;
        drop(statement);
        if include_decisions {
            record_query();
            let mut statement=connection.prepare_cached("SELECT CASE WHEN json_type(m.row_json,'$.text') IS NULL THEN '' ELSE json_extract(m.row_json,'$.text') END FROM plan_task_milestones m JOIN plan_tasks t USING(plan_id,task_id)
                WHERE m.plan_id=?1 AND json_extract(m.row_json,'$.kind')='decision' ORDER BY t.position,m.seq").map_err(db_error)?;
            input.decisions = statement
                .query_map([id], |row| column::<String>(row, 0))
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?;
        }
        connection.commit().map_err(db_error)?;
        Ok(input)
    }

    pub(in crate::service_plan) fn handoff_create_receipt(
        &self,
        id: &str,
        handoff: &str,
    ) -> Result<(String, String), PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row("SELECT CASE WHEN json_type(row_json,'$.content.body_path')='text' THEN json_extract(row_json,'$.content.body_path') ELSE '' END,
            CASE WHEN json_type(row_json,'$.content.resume_prompt')='text' THEN json_extract(row_json,'$.content.resume_prompt') ELSE '' END FROM plan_elements WHERE plan_id=?1 AND element_id=?2",
            params![id,handoff],|row|Ok((column(row,0)?,column(row,1)?))).optional().map_err(db_error)?.ok_or(PlanError::NotFound)
    }

    pub(in crate::service_plan) fn active_reseed_for_tab(
        &self,
        tab: &str,
        except: Option<&str>,
    ) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection.query_row("SELECT EXISTS(SELECT 1 FROM plan_elements WHERE (?2 IS NULL OR plan_id<>?2)
            AND json_extract(row_json,'$.kind')='handoff'
            AND json_type(row_json,'$.content.reseed_pending')='object'
            AND json_type(row_json,'$.content.reseed_pending.tab_id')='text'
            AND json_extract(row_json,'$.content.reseed_pending.tab_id')=?1
            AND json_extract(row_json,'$.content.reseed_state') IN ('new_sent','session_seen','resume_sent'))",
            params![tab,except],|row|row.get(0)).map_err(db_error)
    }

    pub(in crate::service_plan) fn handoff_resume_candidates(
        &self,
        tab: &str,
        new_session: &str,
    ) -> Result<Vec<HandoffResumeCandidateRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached("WITH candidates AS (
            SELECT e.plan_id,json_extract(e.row_json,'$.id') AS handoff_id,p.updated_at,
                row_number() OVER(PARTITION BY e.plan_id ORDER BY e.position) AS rank
            FROM plan_elements e JOIN plans p ON p.id=e.plan_id
            WHERE json_extract(e.row_json,'$.kind')='handoff'
                AND json_type(e.row_json,'$.content.reseed_pending')='object'
                AND json_type(e.row_json,'$.content.reseed_pending.tab_id')='text'
                AND json_extract(e.row_json,'$.content.reseed_pending.tab_id')=?1
                AND json_extract(e.row_json,'$.content.reseed_state')='new_sent'
                AND json_type(e.row_json,'$.content.reseed_pending.source_session_id')='text'
                AND json_extract(e.row_json,'$.content.reseed_pending.source_session_id')<>?2)
            SELECT plan_id,handoff_id FROM candidates WHERE rank=1 ORDER BY updated_at DESC,plan_id").map_err(db_error)?;
        let rows = statement
            .query_map(params![tab, new_session], |row| {
                Ok(HandoffResumeCandidateRead {
                    plan_id: column(row, 0)?,
                    handoff_id: column(row, 1)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}
