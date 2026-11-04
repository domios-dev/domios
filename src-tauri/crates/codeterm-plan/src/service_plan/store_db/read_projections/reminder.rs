use super::{column, decode, record_query};
use crate::plan::{ControlLoopMode, PlanMode, PlanStatus, PlanningState, TaskStatus};
use crate::service_plan::state::TaskProjection;
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

mod functions;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderPlanRead {
    pub id: String,
    pub status: PlanStatus,
    pub mode: PlanMode,
    pub planning_state: PlanningState,
    pub project_path: String,
    pub team_id: Option<String>,
    pub owner_tab_id: Option<String>,
    pub control_loop: ReminderControlRead,
    pub tasks: Vec<ReminderTaskRead>,
    pub state_key_digest: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderControlRead {
    pub mode: ControlLoopMode,
    pub heartbeat_interval_secs: Option<u64>,
    pub has_fallback: bool,
}

impl ReminderControlRead {
    pub fn owner_heartbeat(&self) -> bool {
        self.mode != ControlLoopMode::Off
            && (self.mode == ControlLoopMode::Heartbeat || self.has_fallback)
    }

    pub fn interval(&self, default: u64) -> u64 {
        if self.mode == ControlLoopMode::Heartbeat {
            self.heartbeat_interval_secs.unwrap_or(default)
        } else {
            default
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderTaskRead {
    pub id: String,
    pub status: TaskStatus,
    pub assigned_tab_id: Option<String>,
    pub label: String,
    pub projection: TaskProjection,
    pub held: bool,
}

impl ReminderPlanRead {
    pub fn complete(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(|task| task.status.is_terminal())
    }

    pub fn actionable_task_ids(&self) -> Vec<String> {
        self.tasks
            .iter()
            .filter(|task| !task.held && task.projection.needs_orchestrator_action())
            .map(|task| task.id.clone())
            .collect()
    }
}

pub struct ReminderReadSnapshot {
    connection: Connection,
}

impl PlanStore {
    pub fn reminder_read_snapshot(&self) -> Result<ReminderReadSnapshot, PlanError> {
        self.database.reminder_read_snapshot()
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn reminder_read_snapshot(
        &self,
    ) -> Result<ReminderReadSnapshot, PlanError> {
        let connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(super::super::db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(super::super::db_error)?;
        super::super::status_queries::register_functions(&connection)?;
        super::super::curator_notice_queries::register_functions(&connection)?;
        functions::register(&connection)?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION")
            .map_err(super::super::db_error)?;
        Ok(ReminderReadSnapshot { connection })
    }
}

impl ReminderReadSnapshot {
    pub fn plan(&self, id: &str) -> Result<Option<ReminderPlanRead>, PlanError> {
        plan(&self.connection, id)
    }

    pub fn candidates(&self, tab: Option<&str>) -> Result<Vec<ReminderPlanRead>, PlanError> {
        if tab.is_some_and(|tab| tab.trim().is_empty()) {
            return Ok(Vec::new());
        }
        record_query();
        let mut statement = self.connection.prepare_cached(
            "SELECT p.id FROM plans p WHERE p.status='active' AND json_extract(p.header_json,'$.mode')='execution'
             AND EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id AND t.status IN ('pending','in_progress','in_review','blocked'))
             AND (?1 IS NULL OR p.owner_tab_id=?1 OR EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id AND t.assigned_tab_id=?1 AND t.status NOT IN ('done','cancelled')))
             ORDER BY p.updated_at DESC,p.id"
        ).map_err(super::super::db_error)?;
        let ids = statement
            .query_map([tab], |row| column::<String>(row, 0))
            .map_err(super::super::db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(super::super::db_error)?;
        ids.iter()
            .map(|id| self.plan(id)?.ok_or(PlanError::NotFound))
            .collect()
    }
}

fn plan(connection: &Connection, id: &str) -> Result<Option<ReminderPlanRead>, PlanError> {
    record_query();
    let header: Option<String> = connection.query_row(
        "SELECT json_object('id',id,'status',status,'mode',coalesce(json_extract(header_json,'$.mode'),'planning'),
            'planning_state',coalesce(json_extract(header_json,'$.planning_state'),'drafting'),
            'project_path',project_path,'team_id',json_extract(header_json,'$.team_id'),'owner_tab_id',owner_tab_id,
            'control_loop',json_object('mode',coalesce(json_extract(header_json,'$.control_loop.mode'),'curator'),
                'heartbeat_interval_secs',json_extract(header_json,'$.control_loop.heartbeat_interval_secs'),
                'has_fallback',json(CASE WHEN json_type(header_json,'$.control_loop.fallback_reason') IS NULL OR json_type(header_json,'$.control_loop.fallback_reason')='null' THEN 'false' ELSE 'true' END)),
            'tasks',json('[]'),'state_key_digest','') FROM plans WHERE id=?1",
        [id], |row| column(row,0)).optional().map_err(super::super::db_error)?;
    let Some(header) = header else {
        return Ok(None);
    };
    let mut read: ReminderPlanRead = decode(id, &header)?;
    let facts = super::super::curator_notice_queries::reminder_task_facts();
    record_query();
    let sql = format!("{facts} SELECT json_object('id',t.task_id,'status',t.status,
        'assigned_tab_id',t.assigned_tab_id,'label',domios_notice_label(json_extract(t.row_json,'$.name'),json_extract(t.row_json,'$.key'),json_extract(t.row_json,'$.handle')),
        'projection',json(domios_reminder_projection(t.status,coalesce(json_extract(t.row_json,'$.review_stage'),'review'),json_extract(t.row_json,'$.qa_state'),
            (SELECT json_group_array(json_object('task_id',dep.value,'name','', 'status',target.status,'missing',json(CASE WHEN target.task_id IS NULL THEN 'true' ELSE 'false' END)))
             FROM json_each(coalesce(t.row_json->'$.depends_on','[]')) dep LEFT JOIN plan_tasks target ON target.plan_id=t.plan_id AND target.task_id=dep.value
             WHERE target.task_id IS NULL OR target.status NOT IN ('done','cancelled')),
            coalesce(json_extract(t.row_json,'$.interruption.handoff_required'),0),
            EXISTS(SELECT 1 FROM plan_tasks child WHERE child.plan_id=t.plan_id AND json_extract(child.row_json,'$.parent_task_id')=t.task_id AND child.status NOT IN ('done','cancelled')),
            json_extract(f.decision,'$.parked'))),
        'held',json(CASE WHEN json_extract(f.decision,'$.held') THEN 'true' ELSE 'false' END))
        FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 ORDER BY t.position");
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(super::super::db_error)?;
    read.tasks = statement
        .query_map(
            params![id, super::super::status_queries::WHITESPACE],
            |row| column::<String>(row, 0),
        )
        .map_err(super::super::db_error)?
        .map(|raw| decode(id, &raw.map_err(super::super::db_error)?))
        .collect::<Result<_, _>>()?;
    read.state_key_digest = functions::state_key(connection, &read)?;
    Ok(Some(read))
}
