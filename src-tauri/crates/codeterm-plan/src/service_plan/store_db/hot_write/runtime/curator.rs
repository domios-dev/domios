use super::*;
use crate::plan::{ControlLoop, CuratorRound};
use rusqlite::Transaction;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CuratorStateRows {
    pub control_loop: ControlLoop,
}
impl RuntimeRows for CuratorStateRows {
    const FIELDS: &'static [&'static str] = &["control_loop"];
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CuratorRoundRows {
    pub control_loop: ControlLoop,
    pub heartbeat: OrchestratorHeartbeat,
    #[serde(skip_serializing)]
    task_states: BTreeMap<String, String>,
    #[serde(skip)]
    strides: Vec<Milestone>,
}
impl RuntimeRows for CuratorRoundRows {
    const FIELDS: &'static [&'static str] = &["control_loop"];
    const HEARTBEAT: bool = true;
    const TASK_STATES: bool = true;
    fn strides(&mut self) -> Vec<Milestone> {
        std::mem::take(&mut self.strides)
    }
}

impl CuratorRoundRows {
    pub fn append_round(&mut self, round: CuratorRound) {
        self.control_loop.task_states = self.task_states.clone();
        self.control_loop
            .defer(round.ended_at, Some(round.next_delay_secs));
        self.strides.push(Milestone {
            id: crate::plan::ids::short_id("ms"),
            from: Default::default(),
            kind: "curator_round".into(),
            text: serde_json::to_string(&round).unwrap_or_default(),
            ts: round.ended_at,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        });
        self.control_loop.record(round);
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CuratorSaveRows {
    pub control_loop: ControlLoop,
    #[serde(skip_serializing)]
    pub completed_skill_episodes: BTreeMap<String, BTreeSet<String>>,
}
impl RuntimeRows for CuratorSaveRows {
    const FIELDS: &'static [&'static str] = &["control_loop"];
    const SKILL_EPISODES: bool = true;
}

pub(super) fn load_context<R: RuntimeRows>(
    tx: &Transaction<'_>,
    id: &str,
    fields: &mut serde_json::Map<String, serde_json::Value>,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    if R::TASK_STATES {
        let mut statement = tx
            .prepare_cached(
                "SELECT task_id, status FROM plan_tasks WHERE plan_id=?1 ORDER BY position",
            )
            .map_err(db_error)?;
        let states = statement
            .query_map([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error)?
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(db_error)?;
        receipt.projected_text_bytes += states
            .iter()
            .map(|(id, status)| id.len() + status.len())
            .sum::<usize>();
        fields.insert(
            "task_states".into(),
            serde_json::to_value(states).map_err(json_error)?,
        );
    }
    if R::SKILL_EPISODES {
        let mut statement = tx
            .prepare_cached(
                "WITH completed_tasks AS MATERIALIZED (
                SELECT plan_id,task_id FROM plan_tasks WHERE plan_id=?1 AND status='done'
             ) SELECT t.task_id, json_extract(s.value,'$.slug') FROM completed_tasks t
            JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id,
            json_each(a.row_json,'$.report.loaded_plan_skills') s
            WHERE json_type(s.value,'$.slug')='text'",
            )
            .map_err(db_error)?;
        let mut episodes = BTreeMap::<String, BTreeSet<String>>::new();
        let pairs = statement
            .query_map([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error)?;
        for pair in pairs {
            let (task, slug) = pair.map_err(db_error)?;
            receipt.projected_text_bytes += task.len() + slug.len();
            episodes
                .entry(format!("plan-skill:{slug}"))
                .or_default()
                .insert(task);
        }
        fields.insert(
            "completed_skill_episodes".into(),
            serde_json::to_value(episodes).map_err(json_error)?,
        );
    }
    Ok(())
}

impl PlanStore {
    pub fn edit_curator_state<T>(
        &self,
        id: &str,
        now: u64,
        change: impl FnOnce(&mut CuratorStateRows) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        self.with_runtime_rows(id, now, change)
    }
    pub fn edit_curator_round<T>(
        &self,
        id: &str,
        now: u64,
        change: impl FnOnce(&mut CuratorRoundRows) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        self.with_runtime_rows(id, now, change)
    }
    pub fn edit_curator_save<T>(
        &self,
        id: &str,
        now: u64,
        change: impl FnOnce(&mut CuratorSaveRows) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        self.with_runtime_rows(id, now, change)
    }
}
