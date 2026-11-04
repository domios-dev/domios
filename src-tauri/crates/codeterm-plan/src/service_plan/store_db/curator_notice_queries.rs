use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{PlanMode, PlanStatus, TaskStatus};
use crate::service_plan::curator_notice_projection::CuratorNoticeRead;
use crate::service_plan::reminders::composition::{
    self, ActionGroup, ReminderComposer, MAX_REMINDER_BYTES,
};
use crate::service_plan::state::TaskCounts;
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};

mod facts;
mod sections;

#[derive(serde::Deserialize)]
struct Header {
    id: String,
    role: String,
    owner_tab_id: Option<String>,
    mode: PlanMode,
    status: PlanStatus,
    orphaned_active: bool,
}

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    facts::register(connection)
}

impl PlanDatabase {
    pub(in crate::service_plan) fn curator_notice_projection(
        &self,
        id: &str,
        now: u64,
    ) -> Result<CuratorNoticeRead, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        record_query();
        let encoded:Option<String>=connection.query_row(
            "SELECT json_object('id',json_extract(header_json,'$.id'),
                'role',domios_notice_role(json_extract(header_json,'$.id'),CASE WHEN json_type(header_json,'$.title')='text' THEN json_extract(header_json,'$.title') ELSE json('invalid notice title') END,CASE WHEN json_type(header_json,'$.team_id') IS NULL OR json_type(header_json,'$.team_id')='null' THEN NULL WHEN json_type(header_json,'$.team_id')='text' THEN json_extract(header_json,'$.team_id') ELSE json('invalid notice team') END),
                'owner_tab_id',CASE WHEN json_type(header_json,'$.owner')='object' THEN json_extract(header_json,'$.owner.tab_id') ELSE json('invalid notice owner') END,
                'mode',CASE WHEN json_type(header_json,'$.mode') IS NULL THEN 'planning' ELSE json_extract(header_json,'$.mode') END,
                'status',json_extract(header_json,'$.status'),
                'orphaned_active',json(CASE WHEN json_extract(header_json,'$.status')='active' AND json_extract(header_json,'$.team_id') IS NULL
                    AND json_type(header_json,'$.orphaned') IS NOT NULL AND json_type(header_json,'$.orphaned')<>'null' THEN 'true' ELSE 'false' END))
             FROM plans WHERE id=?1",[id],|row|column(row,0),
        ).optional().map_err(db_error)?;
        let header: Header = decode(id, &encoded.ok_or(PlanError::NotFound)?)?;
        if header.id != id {
            return Err(PlanError::Quarantined {
                plan_id: id.into(),
                reason: "notice header id differs from row key".into(),
            });
        }
        let mut render = Render {
            connection: &connection,
            id,
            composer: ReminderComposer::with_header(id, header.role.clone()),
            labels: HashMap::new(),
            action_ids: Vec::new(),
            orphaned_active: header.orphaned_active,
        };
        let groups = action_groups(&connection, id)?;
        let (parked, pending) = decision_counts(&connection, id)?;
        let mut ready = render.append(&composition::render_action_groups(groups))?;
        if ready && parked > 0 {
            ready = render.append(&format!("{parked} tasks parked for later groups."))?;
        }
        if ready && !matches!(header.mode, PlanMode::Paused | PlanMode::Complete) {
            let counts = task_counts(&connection, id)?;
            if counts.total == 0 || counts.done + counts.cancelled != counts.total {
                ready = render.append(&crate::service_plan::heartbeat::view::state_header(
                    id, &counts, pending,
                ))?;
                if ready {
                    ready = render.actions(false)?;
                }
                if ready && !render.orphaned_active {
                    ready = sections::loops(&mut render)?;
                }
                if ready {
                    ready = render.actions(true)?;
                }
                if ready {
                    ready = sections::learnings(&mut render)?;
                }
                if ready {
                    ready = sections::regression(&mut render)?;
                }
                if ready {
                    ready = sections::competition(&mut render)?;
                }
                if ready {
                    ready = sections::gates(&mut render, now)?;
                }
                if ready {
                    ready = sections::control(&mut render)?;
                }
                if ready {
                    ready = sections::skills(&mut render, &header)?;
                }
                if ready {
                    render.append(&format!(
                        "{}\n</codeterm-orchestrator-heartbeat>",
                        crate::service_plan::heartbeat::HEARTBEAT_MORE
                    ))?;
                }
            }
        }
        let body = render.composer.finish();
        connection.commit().map_err(db_error)?;
        Ok(CuratorNoticeRead {
            owner_tab_id: header.owner_tab_id,
            body,
        })
    }
}

fn action_groups(connection: &Connection, id: &str) -> Result<[ActionGroup; 3], PlanError> {
    record_query();
    let label = facts::label_sql("t.row_json");
    let sql=format!("{}, grouped AS (
        SELECT task_id,position,json_extract(decision,'$.category') AS category,
            count(*) OVER(PARTITION BY json_extract(decision,'$.category')) AS total,
            row_number() OVER(PARTITION BY json_extract(decision,'$.category') ORDER BY position) AS rank
        FROM facts WHERE json_extract(decision,'$.category')>=0
    ) SELECT g.category,g.total,{label}
        FROM grouped g JOIN plan_tasks t ON t.plan_id=?1 AND t.task_id=g.task_id WHERE g.rank<=5 ORDER BY g.category,g.position",facts::task_facts());
    let mut groups = std::array::from_fn(|_| ActionGroup {
        labels: Vec::new(),
        total: 0,
    });
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map(params![id, super::status_queries::WHITESPACE], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, usize>(1)?,
                column::<String>(row, 2)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (category, total, label) = row.map_err(db_error)?;
        groups[category].total = total;
        groups[category].labels.push(label);
    }
    Ok(groups)
}

fn decision_counts(connection: &Connection, id: &str) -> Result<(usize, usize), PlanError> {
    record_query();
    connection.query_row(&format!("{} SELECT coalesce(sum(json_extract(decision,'$.parked')),0),coalesce(sum(json_extract(decision,'$.pending')),0) FROM facts",facts::task_facts()),
        params![id,super::status_queries::WHITESPACE],|row|Ok((row.get(0)?,row.get(1)?))).map_err(db_error)
}

fn task_counts(connection: &Connection, id: &str) -> Result<TaskCounts, PlanError> {
    record_query();
    let mut counts = TaskCounts::default();
    let mut statement = connection
        .prepare_cached(
            "SELECT json_quote(status),count(*) FROM plan_tasks WHERE plan_id=?1 GROUP BY status",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((column::<String>(row, 0)?, row.get::<_, usize>(1)?))
        })
        .map_err(db_error)?;
    for row in rows {
        let (status, count) = row.map_err(db_error)?;
        counts.total += count;
        match decode::<TaskStatus>(id, &status)? {
            TaskStatus::Pending => counts.pending = count,
            TaskStatus::InProgress => counts.in_progress = count,
            TaskStatus::InReview => counts.in_review = count,
            TaskStatus::Blocked => counts.blocked = count,
            TaskStatus::Paused => counts.paused = count,
            TaskStatus::Done => counts.done = count,
            TaskStatus::Cancelled => counts.cancelled = count,
        }
    }
    Ok(counts)
}

struct Render<'a> {
    connection: &'a Connection,
    id: &'a str,
    composer: ReminderComposer,
    labels: HashMap<String, Option<String>>,
    action_ids: Vec<String>,
    orphaned_active: bool,
}

impl Render<'_> {
    fn append(&mut self, fragment: &str) -> Result<bool, PlanError> {
        let tokens: HashSet<&str> = fragment
            .split(composition::reference_delimiter)
            .filter(|token| !token.is_empty() && !self.labels.contains_key(*token))
            .collect();
        if !tokens.is_empty() {
            record_query();
            let encoded =
                serde_json::to_string(&tokens).map_err(|error| PlanError::Io(error.to_string()))?;
            let label = facts::label_sql("row_json");
            let mut statement=self.connection.prepare_cached(&format!("SELECT task_id,{label} FROM plan_tasks WHERE plan_id=?1 AND task_id IN(SELECT value FROM json_each(?2))")).map_err(db_error)?;
            let rows = statement
                .query_map(params![self.id, encoded], |row| {
                    Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
                })
                .map_err(db_error)?;
            for token in tokens {
                self.labels.insert(token.into(), None);
            }
            for row in rows {
                let (id, label) = row.map_err(db_error)?;
                self.labels.insert(id, Some(label));
            }
        }
        let labeled = composition::label_references(fragment, |token| {
            self.labels.get(token).cloned().flatten()
        });
        Ok(self.composer.append(&labeled))
    }

    fn actions(&mut self, late: bool) -> Result<bool, PlanError> {
        if self.orphaned_active && !late {
            return Ok(true);
        }
        record_query();
        let ranks = if late { "(5,6)" } else { "(1,2,3)" };
        let code = if self.orphaned_active {
            "CASE WHEN json_extract(decision,'$.handoff') THEN 601 ELSE 0 END"
        } else {
            "json_extract(decision,'$.action')"
        };
        let sql = format!(
            "{} SELECT task_id,{code} AS code FROM facts
            WHERE CAST(({code})/100 AS INTEGER) IN {ranks}
            ORDER BY CAST(({code})/100 AS INTEGER),position LIMIT ?3",
            facts::task_facts()
        );
        let connection = self.connection;
        let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
        let mut rows = statement
            .query(params![
                self.id,
                super::status_queries::WHITESPACE,
                MAX_REMINDER_BYTES + 1
            ])
            .map_err(db_error)?;
        loop {
            let mut block = String::new();
            let mut count = 0;
            for _ in 0..16 {
                let Some(row) = rows.next().map_err(db_error)? else {
                    break;
                };
                let id: String = column(row, 0).map_err(db_error)?;
                let code: i64 = row.get(1).map_err(db_error)?;
                let verb = facts::action_verb(code)?;
                block.push_str(&format!("- {id}: {verb}\n"));
                self.action_ids.push(id);
                count += 1;
            }
            if count == 0 {
                return Ok(true);
            }
            if !self.append(&block)? {
                return Ok(false);
            }
        }
    }
}
