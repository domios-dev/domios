use super::{column, decode, record_query};
use crate::plan::{ControlLoopMode, PlanMode, PlanStatus, PlanningState, TaskStatus};
use crate::service_plan::state::TaskPresentation;
use crate::service_plan::{PlanError, PlanStore};
#[cfg(any(test, feature = "test-support"))]
use rusqlite::{params, OptionalExtension};
use rusqlite::{Connection, OpenFlags};
use std::collections::BTreeMap;
use std::rc::Rc;

mod blockers;
mod candidates;
mod composition;
pub use candidates::{
    reminder_targets, ReminderCandidateRead, ReminderTargetRead, ReminderWorkerBindingRead,
};
pub use composition::ReminderCompositionRead;
mod admission;
mod functions;
mod gate;
mod gate_rows;
pub use admission::{ReminderAdmissionRead, ReminderAdmissionTaskRead, ReminderOwnerRowRead};

#[derive(Clone, serde::Deserialize, serde::Serialize)]
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
    #[serde(default)]
    pub projection: BTreeMap<String, String>,
    #[serde(default)]
    pub learning_catalog: BTreeMap<String, String>,
    #[serde(default)]
    pub gates: Vec<ReminderGateRead>,
    #[serde(skip)]
    connection: Option<Rc<Connection>>,
    #[serde(skip)]
    gate_scope: Option<GateScope>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    _fixture: Option<Rc<tempfile::TempDir>>,
}

#[derive(Clone, PartialEq, Eq)]
enum GateScope {
    Owner,
    Worker(String),
}

impl std::fmt::Debug for ReminderPlanRead {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output
            .debug_struct("ReminderPlanRead")
            .field("id", &self.id)
            .field("tasks", &self.tasks)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderControlRead {
    pub mode: ControlLoopMode,
    pub heartbeat_interval_secs: Option<u64>,
    pub has_fallback: bool,
}

impl ReminderControlRead {
    pub fn active_execution(&self, status: &PlanStatus, mode: &PlanMode, complete: bool) -> bool {
        active_execution(status, mode, self.mode, complete)
    }
    pub fn owner_eligible(
        &self,
        status: &PlanStatus,
        mode: &PlanMode,
        complete: bool,
        open_work: bool,
    ) -> bool {
        self.active_execution(status, mode, complete) && self.owner_heartbeat() && open_work
    }

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
    pub projection: ReminderTaskProjectionRead,
    pub held: bool,
    #[serde(default)]
    pub status_changed_at: u64,
    #[serde(default)]
    pub handoff_required: bool,
    #[serde(default)]
    pub blocked_fingerprint: Option<String>,
    #[serde(default)]
    pub last_review: Option<ReminderReviewRead>,
    #[serde(default)]
    pub lane_path: Option<String>,
    #[serde(default)]
    pub lane_branch: Option<String>,
    #[serde(default)]
    pub attempts: Vec<ReminderAttemptRead>,
    #[serde(default)]
    pub projection_hashes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderTaskProjectionRead {
    pub presentation: TaskPresentation,
    pub dependency_waits: Vec<ReminderDependencyRead>,
    pub actionable: bool,
}

impl ReminderTaskProjectionRead {
    pub fn needs_orchestrator_action(&self) -> bool {
        self.actionable
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderDependencyRead {
    pub task_id: String,
    pub status: Option<TaskStatus>,
    pub missing: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderReviewRead {
    pub id: String,
    pub verdict: crate::plan::ReviewVerdict,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderAttemptRead {
    pub id: String,
    pub executor_tab_id: String,
    pub session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub closed: bool,
    pub acknowledged: bool,
    pub report_id: Option<String>,
    pub interruption_fingerprint: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderGateRead {
    pub id: String,
    pub status: crate::plan::GateStatus,
    pub answered: bool,
}

impl ReminderPlanRead {
    pub fn composition(&self) -> Result<ReminderCompositionRead, PlanError> {
        Ok(ReminderCompositionRead {
            id: self.id.clone(),
            mode: self.mode.clone(),
            planning_state: self.planning_state,
            owner_tab_id: self.owner_tab_id.clone(),
            team_id: self.team_id.clone(),
            connection: self
                .connection
                .clone()
                .ok_or_else(|| PlanError::Validation("reminder snapshot is closed".into()))?,
        })
    }
    pub fn digest_input(
        &self,
        now: u64,
        targets: &crate::service_plan::NoticeInventoryTargets,
    ) -> Result<crate::service_plan::NoticeDigestInput, PlanError> {
        self.composition()?.digest_input(now, targets)
    }
    pub fn compose_worker(&self, tab: &str, text: &str) -> Result<String, PlanError> {
        self.composition()?.compose_worker(tab, text)
    }
    pub fn compose_owner(
        &self,
        directive: &str,
        digests: &[String],
        selection: Option<&[String]>,
        plain: bool,
        now: u64,
    ) -> Result<String, PlanError> {
        self.composition()?
            .compose_owner(directive, digests, selection, plain, now)
    }
    pub fn planner_resolution(
        &self,
        probe: impl Fn(&str) -> crate::plan::reconcile::TabProbe,
    ) -> Result<crate::plan::peer::PeerResolution, PlanError> {
        self.composition()?.planner_resolution(probe)
    }
    pub fn release_snapshot(&mut self) {
        self.connection = None;
    }
    pub fn with_worker_gate_facts(mut self, task_id: &str) -> Result<Self, PlanError> {
        let scope = GateScope::Worker(task_id.into());
        if self.gate_scope.as_ref() != Some(&scope) {
            self.clear_gate_facts();
            let connection = self
                .connection
                .clone()
                .ok_or_else(|| PlanError::Validation("reminder snapshot is closed".into()))?;
            gate::load(&connection, &mut self, Some(task_id))?;
            self.gate_scope = Some(scope);
        }
        Ok(self)
    }

    pub fn blocker_facts(
        &self,
        task_id: Option<&str>,
    ) -> Result<Vec<ReminderBlockerRead>, PlanError> {
        blockers::read(self.connection()?, &self.id, task_id)
    }

    pub fn with_gate_facts(mut self) -> Result<Self, PlanError> {
        if self.gate_scope != Some(GateScope::Owner) {
            self.clear_gate_facts();
            let connection = self
                .connection
                .clone()
                .ok_or_else(|| PlanError::Validation("reminder snapshot is closed".into()))?;
            gate::load(&connection, &mut self, None)?;
            self.gate_scope = Some(GateScope::Owner);
        }
        Ok(self)
    }

    fn clear_gate_facts(&mut self) {
        self.projection.clear();
        self.gates.clear();
        self.learning_catalog.clear();
        for task in &mut self.tasks {
            task.attempts.clear();
            task.projection_hashes.clear();
        }
    }
    fn connection(&self) -> Result<&Connection, PlanError> {
        self.connection
            .as_deref()
            .ok_or_else(|| PlanError::Validation("reminder snapshot is closed".into()))
    }

    pub fn delta_context(
        &self,
        delta: &BTreeMap<String, String>,
        limit: usize,
    ) -> Result<(BTreeMap<String, serde_json::Value>, bool), PlanError> {
        gate::context(self.connection()?, self, delta, limit)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(plan: &crate::plan::Plan, scrubber: fn(&str) -> String) -> Self {
        let fixture = Rc::new(tempfile::tempdir().expect("reminder fixture directory"));
        let database = super::PlanDatabase::open(fixture.path().join("plans.db"))
            .expect("reminder fixture database");
        database
            .persist_plan_rows(plan, None)
            .expect("reminder fixture rows");
        let snapshot = database
            .reminder_read_snapshot(scrubber)
            .expect("reminder fixture snapshot");
        let mut read = snapshot
            .plan(&plan.id)
            .expect("reminder fixture projection")
            .expect("reminder fixture plan")
            .with_gate_facts()
            .expect("reminder fixture gate facts");
        read._fixture = Some(fixture);
        read
    }
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
    connection: Rc<Connection>,
}

impl PlanStore {
    pub fn reminder_read_snapshot(
        &self,
        scrubber: fn(&str) -> String,
    ) -> Result<ReminderReadSnapshot, PlanError> {
        self.database.reminder_read_snapshot(scrubber)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn reminder_read_snapshot(
        &self,
        scrubber: fn(&str) -> String,
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
        super::super::worker_read_queries::register_functions(&connection)?;
        functions::register(&connection, scrubber)?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION")
            .map_err(super::super::db_error)?;
        Ok(ReminderReadSnapshot {
            connection: Rc::new(connection),
        })
    }
}

impl ReminderReadSnapshot {
    pub fn composition(&self, id: &str) -> Result<Option<ReminderCompositionRead>, PlanError> {
        composition::read(self.connection.clone(), id)
    }
    pub fn candidate_targets(
        &self,
        tab: Option<&str>,
    ) -> Result<Vec<ReminderCandidateRead>, PlanError> {
        candidates::read(&self.connection, tab)
    }
    pub fn worker_bindings(
        &self,
        tab: &str,
        known_plan: Option<&str>,
    ) -> Result<Vec<ReminderWorkerBindingRead>, PlanError> {
        candidates::worker_bindings(&self.connection, tab, known_plan)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn plan(&self, id: &str) -> Result<Option<ReminderPlanRead>, PlanError> {
        let Some(mut read) = plan(&self.connection, id)? else {
            return Ok(None);
        };
        read.connection = Some(self.connection.clone());
        Ok(Some(read))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn candidate_reference(
        &self,
        tab: Option<&str>,
    ) -> Result<Vec<ReminderPlanRead>, PlanError> {
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
            .map(|id| {
                let mut read = plan(&self.connection, id)?.ok_or(PlanError::NotFound)?;
                read.connection = Some(self.connection.clone());
                Ok(read)
            })
            .collect()
    }
}

#[cfg(any(test, feature = "test-support"))]
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
            EXISTS(SELECT 1 FROM open_parents parent WHERE parent.parent_id=t.task_id),
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

#[derive(Debug)]
pub struct ReminderBlockerRead {
    pub identity: String,
    pub summary: String,
    pub task_ids: std::collections::BTreeSet<String>,
}

fn active_execution(
    status: &PlanStatus,
    mode: &PlanMode,
    control: ControlLoopMode,
    complete: bool,
) -> bool {
    *status == PlanStatus::Active
        && *mode == PlanMode::Execution
        && control != ControlLoopMode::Off
        && !complete
}
