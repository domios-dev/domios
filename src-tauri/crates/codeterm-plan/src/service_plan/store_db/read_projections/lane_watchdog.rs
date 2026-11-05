use super::{column, decode, record_query};
use crate::plan::{Plan, TaskStatus};
use crate::service_plan::store_db::{db_error, PlanDatabase};
use crate::service_plan::{PlanError, PlanStore};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LaneWatchdogRead {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_tab_id: Option<String>,
    pub tasks: Vec<LaneWatchdogTaskRead>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LaneWatchdogTaskRead {
    pub id: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    pub last_activity_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_session: Option<String>,
}

impl From<&Plan> for LaneWatchdogRead {
    fn from(plan: &Plan) -> Self {
        Self {
            id: plan.id.clone(),
            owner_tab_id: plan.owner.tab_id.clone(),
            tasks: plan
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::InProgress)
                .map(|task| {
                    let tab = task
                        .assigned_tab_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|tab| !tab.is_empty());
                    let mut matching = task.execution_attempts.iter().filter(|a| {
                        a.closed_at.is_none() && Some(a.executor_tab_id.as_str()) == tab
                    });
                    let first = matching.next();
                    let expected_session = if matching.next().is_none() {
                        first
                            .and_then(|a| a.session_id.as_deref())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    } else {
                        None
                    };
                    LaneWatchdogTaskRead {
                        id: task.id.clone(),
                        status: task.status.clone(),
                        assigned_tab_id: task.assigned_tab_id.clone(),
                        peer: task.peer.clone(),
                        last_activity_ms: crate::plan::reconcile::lane_last_activity_ms(task),
                        expected_session,
                    }
                })
                .collect(),
        }
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn lane_watchdog_projection(
        &self,
    ) -> Result<Vec<LaneWatchdogRead>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached(
            "WITH watchdog_tasks AS MATERIALIZED (
                SELECT plan_id,task_id,position,status,assigned_tab_id,json_extract(row_json,'$.peer') AS peer,
                    coalesce(json_extract(row_json,'$.status_changed_at'),0) AS changed_at
                FROM plan_tasks WHERE status='in_progress' AND plan_id IN (SELECT id FROM plans WHERE status='active')
             ) SELECT p.id,json_object('id',p.id,'owner_tab_id',json_extract(p.header_json,'$.owner.tab_id'),
                'tasks',json((SELECT json_group_array(json(task)) FROM (SELECT json_object(
                    'id',t.task_id,'status',t.status,'assigned_tab_id',t.assigned_tab_id,'peer',t.peer,
                    'last_activity_ms',max(t.changed_at,
                        coalesce((SELECT max(json_extract(m.row_json,'$.ts')) FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id),0),
                        coalesce((SELECT max(json_extract(a.row_json,'$.reported_at')) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id),0)),
                    'expected_session',CASE WHEN length(trim(t.assigned_tab_id,?1))>0 AND (SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id
                        AND json_extract(a.row_json,'$.closed_at') IS NULL AND a.executor_tab_id=trim(t.assigned_tab_id,?1))=1
                        THEN (SELECT nullif(trim(json_extract(a.row_json,'$.session_id'),?1),'') FROM plan_task_attempts a
                            WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.closed_at') IS NULL
                                AND a.executor_tab_id=trim(t.assigned_tab_id,?1)) END)
                    AS task FROM watchdog_tasks t WHERE t.plan_id=p.id ORDER BY t.position))))
             FROM plans p WHERE p.status='active' AND EXISTS(SELECT 1 FROM watchdog_tasks t WHERE t.plan_id=p.id)
             ORDER BY p.updated_at DESC,p.id"
        ).map_err(db_error)?;
        let rows = statement
            .query_map([super::super::status_queries::WHITESPACE], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (id, raw) = row.map_err(db_error)?;
            decode(&id, &raw)
        })
        .collect()
    }
}

impl PlanStore {
    pub fn lane_watchdog_projection(&self) -> Result<Vec<LaneWatchdogRead>, PlanError> {
        self.database.lane_watchdog_projection()
    }
}
