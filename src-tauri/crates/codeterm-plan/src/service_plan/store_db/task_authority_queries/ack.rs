use super::super::document_queries::regression::unsigned_timestamp;
use super::*;
use crate::service_plan::task_authority_projection::TaskAckAuthorityRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn task_ack_authority_projection(
        &self,
        plan: &str,
        task: &str,
        tab: &str,
        attempt: &str,
        runtime_binding: Option<(&str, &str)>,
        enforce: bool,
    ) -> Result<TaskAckAuthorityRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let acknowledged = unsigned_timestamp("a.row_json", "acknowledged_at", false);
        let (exists,raw):(bool,Option<String>)=connection.query_row(
            &format!("{SELECTED_TASK_IN_PLAN}, first_matching AS (
                SELECT a.position FROM plan_task_attempts a JOIN selected_task t ON t.plan_id=a.plan_id AND t.task_id=a.task_id
                WHERE json_extract(a.row_json,'$.id')=?4 AND json_extract(a.row_json,'$.executor_tab_id')=?3
                    AND json_extract(a.row_json,'$.closed_at') IS NULL AND json_extract(a.row_json,'$.retired_at') IS NULL
                ORDER BY a.position LIMIT 1), facts AS MATERIALIZED (
                SELECT t.plan_id,t.task_id,m.position,
                    (SELECT json_extract(latest.row_json,'$.id') FROM plan_task_attempts latest
                        WHERE latest.plan_id=t.plan_id AND latest.task_id=t.task_id AND json_extract(latest.row_json,'$.closed_at') IS NULL ORDER BY latest.position DESC LIMIT 1) AS latest_unclosed,
                    (SELECT json_extract(latest.row_json,'$.competition_group_id') FROM plan_task_attempts latest
                        WHERE latest.plan_id=t.plan_id AND latest.task_id=t.task_id AND json_extract(latest.row_json,'$.closed_at') IS NULL
                            AND json_extract(latest.row_json,'$.competition_group_id') IS NOT NULL ORDER BY latest.position DESC LIMIT 1) AS latest_group
                FROM selected_task t LEFT JOIN first_matching m)
             SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('task_id',t.task_id,'status',json_extract(t.row_json,'$.status'),'assigned_tab',t.assigned_tab_id,
                    'attempt',json(CASE WHEN a.position IS NOT NULL THEN json_object('id',json_extract(a.row_json,'$.id'),
                        'executor',json_extract(a.row_json,'$.executor_tab_id'),'session',json_extract(a.row_json,'$.session_id'),
                        'nonce',json_extract(a.row_json,'$.session_binding_nonce'),'group',json_extract(a.row_json,'$.competition_group_id'),
                        'acknowledged',json(CASE WHEN json_extract(a.row_json,'$.acknowledged_at') IS NULL THEN 'false'
                            WHEN length({acknowledged})>0 THEN 'true' END)) END),
                    'latest_unclosed',f.latest_unclosed,'latest_group',f.latest_group,
                    'response_attempt',json(CASE WHEN json_extract(a.row_json,'$.acknowledged_at') IS NOT NULL THEN
                        CASE WHEN domios_ack_response_authorized(?3,t.assigned_tab_id,json_extract(a.row_json,'$.id'),json_extract(a.row_json,'$.executor_tab_id'),
                            json_extract(a.row_json,'$.session_id'),json_extract(a.row_json,'$.session_binding_nonce'),json_extract(a.row_json,'$.competition_group_id'),f.latest_unclosed,f.latest_group,?5,?6,?7)
                        THEN a.row_json END END))
                 FROM facts f JOIN selected_task t ON t.plan_id=f.plan_id AND t.task_id=f.task_id
                    LEFT JOIN plan_task_attempts a ON a.plan_id=f.plan_id AND a.task_id=f.task_id AND a.position=f.position)"),
            params![plan,task,tab,attempt,runtime_binding.map(|(session,_)|session),runtime_binding.map(|(_,nonce)|nonce),enforce],|row|Ok((column(row,0)?,column(row,1)?)),
        ).map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        decode(plan, &raw.ok_or(PlanError::TaskNotFound)?)
    }
}

pub(super) fn register(connection: &rusqlite::Connection) -> Result<(), PlanError> {
    use crate::service_plan::task_authority_projection::startup::{
        startup_subject_current, StartupSubject,
    };
    connection.create_scalar_function("domios_ack_response_authorized",12,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let tab:String=context.get(0)?;let assigned:Option<String>=context.get(1)?;
            let id:String=context.get(2)?;let executor:String=context.get(3)?;
            let session:Option<String>=context.get(4)?;let nonce:Option<String>=context.get(5)?;let group:Option<String>=context.get(6)?;
            let latest:Option<String>=context.get(7)?;let latest_group:Option<String>=context.get(8)?;
            let current_session:Option<String>=context.get(9)?;let current_nonce:Option<String>=context.get(10)?;let enforce:bool=context.get(11)?;
            Ok(startup_subject_current(&tab,assigned.as_deref(),StartupSubject {id:&id,executor:&executor,session:session.as_deref(),group:group.as_deref()},latest.as_deref(),latest_group.as_deref())
                && crate::service_plan::task_authority_projection::acknowledging_binding_matches(session.as_deref(),nonce.as_deref(),current_session.as_deref().zip(current_nonce.as_deref()),enforce))
        }).map_err(db_error)
}
