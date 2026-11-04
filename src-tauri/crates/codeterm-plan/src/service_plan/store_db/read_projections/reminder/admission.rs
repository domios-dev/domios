use super::{
    column, decode, record_query, ReminderAttemptRead, ReminderCompositionRead,
    ReminderControlRead, ReminderGateRead, ReminderReadSnapshot,
};
use crate::plan::{PlanMode, PlanStatus, PlanningState, TaskStatus};
use crate::service_plan::PlanError;
use rusqlite::{Connection, OptionalExtension};
use std::{collections::BTreeMap, rc::Rc};
mod queries;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReminderAdmissionTaskRead {
    pub id: String,
    pub status: TaskStatus,
    pub assigned_tab_id: Option<String>,
    pub actionable: bool,
    pub held: bool,
    pub status_changed_at: u64,
    pub lane_path: Option<String>,
    pub lane_branch: Option<String>,
    pub attempts: Vec<ReminderAttemptRead>,
}
#[derive(Clone, serde::Serialize)]
pub struct ReminderAdmissionRead {
    pub id: String,
    pub status: PlanStatus,
    pub mode: PlanMode,
    pub planning_state: PlanningState,
    pub project_path: String,
    pub team_id: Option<String>,
    pub owner_tab_id: Option<String>,
    pub control_loop: ReminderControlRead,
    pub complete: bool,
    pub owner_open_work: bool,
    pub tasks: Vec<ReminderAdmissionTaskRead>,
    pub projection: BTreeMap<String, String>,
    pub learning_catalog: BTreeMap<String, String>,
    pub gates: Vec<ReminderGateRead>,
    #[serde(skip)]
    connection: Rc<Connection>,
    #[serde(skip)]
    loaded: Option<Option<String>>,
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    fixture: Option<Rc<tempfile::TempDir>>,
}
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReminderOwnerRowRead {
    Wake { event: String, id: String },
    Worker { task: ReminderAdmissionTaskRead },
}

impl ReminderReadSnapshot {
    pub fn admission(&self, id: &str) -> Result<Option<ReminderAdmissionRead>, PlanError> {
        record_query();
        let raw: Option<String> = self
            .connection
            .query_row(queries::HEADER, [id], |row| column(row, 0))
            .optional()
            .map_err(super::super::super::db_error)?;
        #[derive(serde::Deserialize)]
        struct Header {
            id: String,
            status: PlanStatus,
            mode: PlanMode,
            planning_state: PlanningState,
            project_path: String,
            team_id: Option<String>,
            owner_tab_id: Option<String>,
            control_loop: ReminderControlRead,
            complete: bool,
            owner_open_work: bool,
        }
        let Some(raw) = raw else { return Ok(None) };
        let h: Header = decode(id, &raw)?;
        Ok(Some(ReminderAdmissionRead {
            id: h.id,
            status: h.status,
            mode: h.mode,
            planning_state: h.planning_state,
            project_path: h.project_path,
            team_id: h.team_id,
            owner_tab_id: h.owner_tab_id,
            control_loop: h.control_loop,
            complete: h.complete,
            owner_open_work: h.owner_open_work,
            tasks: Vec::new(),
            projection: BTreeMap::new(),
            learning_catalog: BTreeMap::new(),
            gates: Vec::new(),
            connection: self.connection.clone(),
            loaded: None,
            #[cfg(any(test, feature = "test-support"))]
            fixture: None,
        }))
    }
}
impl ReminderAdmissionRead {
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn composition(&self) -> Result<ReminderCompositionRead, PlanError> {
        Ok(ReminderCompositionRead {
            id: self.id.clone(),
            mode: self.mode.clone(),
            planning_state: self.planning_state,
            owner_tab_id: self.owner_tab_id.clone(),
            team_id: self.team_id.clone(),
            connection: self.connection.clone(),
        })
    }
    pub fn with_gate_facts(self) -> Result<Self, PlanError> {
        self.load(None)
    }
    pub fn with_worker_gate_facts(self, task: &str) -> Result<Self, PlanError> {
        self.load(Some(task))
    }
    fn load(mut self, task: Option<&str>) -> Result<Self, PlanError> {
        if self
            .loaded
            .as_ref()
            .is_some_and(|loaded| loaded.as_deref() == task)
        {
            return Ok(self);
        }
        self.projection =
            queries::projection(&self.connection, &self.id, &self.mode, &self.status, task)?;
        self.tasks = match task {
            Some(task) => queries::worker(&self.connection, &self.id, task)?
                .into_iter()
                .collect(),
            None => Vec::new(),
        };
        self.gates.clear();
        self.learning_catalog.clear();
        if task.is_none() {
            queries::owner_catalog(
                &self.connection,
                &self.id,
                &mut self.gates,
                &mut self.learning_catalog,
            )?;
        }
        self.loaded = Some(task.map(str::to_string));
        Ok(self)
    }
    pub fn actionable_task_ids(&self) -> Result<Vec<String>, PlanError> {
        queries::actionable(&self.connection, &self.id)
    }
    pub fn blocker_facts(
        &self,
        task: Option<&str>,
    ) -> Result<Vec<super::ReminderBlockerRead>, PlanError> {
        super::blockers::read(&self.connection, &self.id, task)
    }
    pub fn delta_context(
        &self,
        delta: &BTreeMap<String, String>,
        limit: usize,
    ) -> Result<(BTreeMap<String, serde_json::Value>, bool), PlanError> {
        queries::context(&self.connection, &self.id, delta, limit)
    }
    pub fn owner_rows(
        &self,
        delta: &BTreeMap<String, String>,
    ) -> Result<Vec<ReminderOwnerRowRead>, PlanError> {
        queries::owner_rows(&self.connection, &self.id, delta)
    }
    pub fn cooldown_key(&self) -> Result<String, PlanError> {
        super::functions::state_key_fields(&self.connection, &self.id, &self.status, &self.mode)
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(plan: &crate::plan::Plan, scrubber: fn(&str) -> String) -> Self {
        let fixture = Rc::new(tempfile::tempdir().unwrap());
        let db = super::super::PlanDatabase::open(fixture.path().join("plans.db")).unwrap();
        db.persist_plan_rows(plan, None).unwrap();
        let snap = db.reminder_read_snapshot(scrubber).unwrap();
        let mut read = snap
            .admission(&plan.id)
            .unwrap()
            .unwrap()
            .with_gate_facts()
            .unwrap();
        read.fixture = Some(fixture);
        read
    }
}

impl std::fmt::Debug for ReminderAdmissionRead {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output
            .debug_struct("ReminderAdmissionRead")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
