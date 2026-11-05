use super::{column, decode, record_query, ReminderControlRead};
use crate::plan::{PlanMode, PlanStatus};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderCandidateRead {
    pub id: String,
    pub status: PlanStatus,
    pub mode: PlanMode,
    pub owner_tab_id: Option<String>,
    pub control_loop: ReminderControlRead,
    pub complete: bool,
    pub owner_open_work: bool,
    pub retained_scope_tabs: Option<Vec<String>>,
    pub worker_tabs: Vec<String>,
    pub popup: Option<ReminderPopupRead>,
    #[serde(skip_serializing)]
    popup_worker_progress: Option<bool>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderCandidateWorkerRead {
    pub task_id: String,
    pub tab_id: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ReminderPopupRead {
    pub worker: bool,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ReminderTargetRead {
    Owner { tab_id: String, plan_id: String },
    Worker { tab_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReminderWorkerBindingRead {
    pub plan_id: String,
    pub task_id: String,
}

impl ReminderCandidateRead {
    pub fn active_execution(&self) -> bool {
        self.control_loop
            .active_execution(&self.status, &self.mode, self.complete)
    }
    pub fn owner_eligible(&self) -> bool {
        self.control_loop.owner_eligible(
            &self.status,
            &self.mode,
            self.complete,
            self.owner_open_work,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(
        plans: &[crate::plan::Plan],
        tab: Option<&str>,
        scrubber: fn(&str) -> String,
    ) -> Vec<Self> {
        let reference = plans
            .iter()
            .map(|plan| super::ReminderPlanRead::for_test(plan, scrubber))
            .collect::<Vec<_>>();
        let mut result = reference
            .iter()
            .map(|plan| {
                read_plan(plan.connection().unwrap(), &plan.id, tab)
                    .unwrap()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        if let Some(tab) = tab {
            for (index, worker) in popup_choices(&result, tab) {
                let reader = &reference[index];
                let labels = labels(
                    reader.connection().unwrap(),
                    &reader.id,
                    if worker { Some(tab) } else { None },
                )
                .unwrap();
                result[index].popup = Some(ReminderPopupRead { worker, labels });
            }
        }
        result
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub fn reminder_targets(plans: &[ReminderCandidateRead]) -> Vec<ReminderTargetRead> {
    let mut targets = BTreeMap::<(String, String), ReminderTargetRead>::new();
    for plan in plans.iter().filter(|plan| plan.owner_eligible()) {
        if let Some(tab) = non_empty(plan.owner_tab_id.as_deref()) {
            targets.insert(
                (tab.into(), plan.id.clone()),
                ReminderTargetRead::Owner {
                    tab_id: tab.into(),
                    plan_id: plan.id.clone(),
                },
            );
        }
    }
    for plan in plans.iter().filter(|plan| plan.active_execution()) {
        for worker in &plan.worker_tabs {
            let Some(tab) = non_empty(Some(worker)) else {
                continue;
            };
            if !targets.keys().any(|(owner, _)| owner == tab) {
                targets
                    .entry((tab.into(), String::new()))
                    .or_insert_with(|| ReminderTargetRead::Worker { tab_id: tab.into() });
            }
        }
    }
    targets.into_values().collect()
}

fn popup_choices(plans: &[ReminderCandidateRead], tab: &str) -> Vec<(usize, bool)> {
    reminder_targets(plans)
        .into_iter()
        .filter_map(|target| match target {
            ReminderTargetRead::Owner { tab_id, plan_id } if tab_id == tab => plans
                .iter()
                .position(|plan| plan.id == plan_id)
                .map(|index| (index, false)),
            ReminderTargetRead::Worker { tab_id } if tab_id == tab => plans
                .iter()
                .position(|plan| plan.popup_worker_progress == Some(true))
                .map(|index| (index, true)),
            _ => None,
        })
        .collect()
}

pub(super) fn read(
    connection: &Connection,
    tab: Option<&str>,
) -> Result<Vec<ReminderCandidateRead>, PlanError> {
    if tab.is_some_and(|tab| tab.trim().is_empty()) {
        return Ok(Vec::new());
    }
    record_query();
    let mut statement=connection.prepare_cached("SELECT p.id FROM plans p WHERE p.status='active' AND json_extract(p.header_json,'$.mode')='execution'
        AND EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id AND t.status IN ('pending','in_progress','in_review','blocked'))
        AND (?1 IS NULL OR p.owner_tab_id=?1 OR EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id AND t.assigned_tab_id=?1 AND t.status NOT IN ('done','cancelled')))
        ORDER BY p.updated_at DESC,p.id").map_err(super::super::super::db_error)?;
    let ids = statement
        .query_map([tab], |row| column::<String>(row, 0))
        .map_err(super::super::super::db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(super::super::super::db_error)?;
    let mut result = ids
        .iter()
        .map(|id| read_plan(connection, id, tab)?.ok_or(PlanError::NotFound))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(tab) = tab {
        for (index, worker) in popup_choices(&result, tab) {
            let labels = labels(
                connection,
                &result[index].id,
                if worker { Some(tab) } else { None },
            )?;
            result[index].popup = Some(ReminderPopupRead { worker, labels });
        }
    }
    Ok(result)
}

fn read_plan(
    connection: &Connection,
    id: &str,
    tab: Option<&str>,
) -> Result<Option<ReminderCandidateRead>, PlanError> {
    record_query();
    let raw:Option<String>=connection.query_row("SELECT json_object('id',p.id,'status',p.status,'mode',coalesce(json_extract(p.header_json,'$.mode'),'planning'),'owner_tab_id',p.owner_tab_id,
        'control_loop',json_object('mode',coalesce(json_extract(p.header_json,'$.control_loop.mode'),'curator'),'heartbeat_interval_secs',json_extract(p.header_json,'$.control_loop.heartbeat_interval_secs'),
            'has_fallback',json(CASE WHEN json_type(p.header_json,'$.control_loop.fallback_reason') IS NULL OR json_type(p.header_json,'$.control_loop.fallback_reason')='null' THEN 'false' ELSE 'true' END)),
        'complete',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=p.id) AND NOT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=p.id AND status NOT IN ('done','cancelled')) THEN 'true' ELSE 'false' END),
        'owner_open_work',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=p.id AND status IN ('pending','in_progress','in_review','blocked')) THEN 'true' ELSE 'false' END),
        'retained_scope_tabs',CASE WHEN ?2 IS NULL THEN json((SELECT json_group_array(tab_id) FROM (SELECT DISTINCT trim(assigned_tab_id,?3) AS tab_id FROM plan_tasks WHERE plan_id=p.id AND assigned_tab_id IS NOT NULL AND trim(assigned_tab_id,?3)<>'' ORDER BY tab_id))) ELSE NULL END,
        'worker_tabs',json('[]'),'popup',NULL,
        'popup_worker_progress',CASE WHEN ?2 IS NULL THEN NULL ELSE json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=p.id AND status='in_progress' AND assigned_tab_id=?2) THEN 'true' ELSE 'false' END) END)
        FROM plans p WHERE p.id=?1",params![id,tab,super::super::super::status_queries::WHITESPACE],|row|column(row,0)).optional().map_err(super::super::super::db_error)?;
    let Some(raw) = raw else { return Ok(None) };
    let mut read: ReminderCandidateRead = decode(id, &raw)?;
    read.worker_tabs = if read.active_execution() {
        recipient_tabs(connection, id, tab)?
    } else {
        Vec::new()
    };
    Ok(Some(read))
}

fn worker_rows(
    connection: &Connection,
    id: &str,
    tab: Option<&str>,
    limit: usize,
) -> Result<Vec<ReminderCandidateWorkerRead>, PlanError> {
    record_query();
    let facts = super::super::super::curator_notice_queries::reminder_worker_task_facts();
    let sql=format!("{facts} SELECT t.task_id,t.assigned_tab_id FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 AND json_extract(f.decision,'$.reminder_worker')=1 ORDER BY t.position LIMIT ?4");
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(super::super::super::db_error)?;
    let limit = if limit == usize::MAX {
        -1_i64
    } else {
        limit as i64
    };
    let rows = statement
        .query_map(
            params![
                id,
                super::super::super::status_queries::WHITESPACE,
                tab,
                limit
            ],
            |row| {
                Ok(ReminderCandidateWorkerRead {
                    task_id: column(row, 0)?,
                    tab_id: column(row, 1)?,
                })
            },
        )
        .map_err(super::super::super::db_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(super::super::super::db_error)
}

fn labels(connection: &Connection, id: &str, tab: Option<&str>) -> Result<Vec<String>, PlanError> {
    record_query();
    let mut statement=connection.prepare_cached("SELECT domios_notice_label(json_extract(row_json,'$.name'),json_extract(row_json,'$.key'),json_extract(row_json,'$.handle')) FROM plan_tasks WHERE plan_id=?1 AND status='in_progress' AND (?2 IS NULL OR assigned_tab_id=?2) ORDER BY position LIMIT 5").map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(params![id, tab], |row| column(row, 0))
        .map_err(super::super::super::db_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(super::super::super::db_error)
}

pub(super) fn worker_bindings(
    connection: &Connection,
    tab: &str,
    known_plan: Option<&str>,
) -> Result<Vec<ReminderWorkerBindingRead>, PlanError> {
    record_query();
    let mut statement=connection.prepare_cached("SELECT p.id FROM plans p WHERE (?2 IS NULL OR p.id=?2) AND p.status='active' AND json_extract(p.header_json,'$.mode')='execution'
        AND domios_reminder_active(p.status,json_extract(p.header_json,'$.mode'),coalesce(json_extract(p.header_json,'$.control_loop.mode'),'curator'),
            EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=p.id) AND NOT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=p.id AND status NOT IN ('done','cancelled')))
        AND EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=p.id AND trim(assigned_tab_id,?3)=?1 AND status IN ('pending','in_progress','blocked')) ORDER BY p.updated_at DESC,p.id").map_err(super::super::super::db_error)?;
    let mut rows = statement
        .query(params![
            tab,
            known_plan,
            super::super::super::status_queries::WHITESPACE
        ])
        .map_err(super::super::super::db_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(super::super::super::db_error)? {
        let id: String = column(row, 0).map_err(super::super::super::db_error)?;
        for worker in worker_rows(connection, &id, Some(tab), 2 - result.len())? {
            result.push(ReminderWorkerBindingRead {
                plan_id: id.clone(),
                task_id: worker.task_id,
            });
        }
        if result.len() == 2 {
            break;
        }
    }
    Ok(result)
}

fn recipient_tabs(
    connection: &Connection,
    id: &str,
    tab: Option<&str>,
) -> Result<Vec<String>, PlanError> {
    record_query();
    let facts = super::super::super::curator_notice_queries::reminder_worker_task_facts();
    let sql=format!("{facts} SELECT DISTINCT trim(t.assigned_tab_id,?2) AS tab_id FROM plan_tasks t JOIN facts f ON f.task_id=t.task_id WHERE t.plan_id=?1 AND json_extract(f.decision,'$.reminder_worker')=1 AND trim(t.assigned_tab_id,?2)<>'' ORDER BY tab_id");
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(super::super::super::db_error)?;
    let rows = statement
        .query_map(
            params![id, super::super::super::status_queries::WHITESPACE, tab],
            |row| column(row, 0),
        )
        .map_err(super::super::super::db_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(super::super::super::db_error)
}
