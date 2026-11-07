use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{Milestone, TaskStatus};
use crate::service_plan::state::mission_control::BounceSummary;
use crate::service_plan::status_projection::PlanStatusRead;
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

mod state_facts;

const RECENT: &str = "WITH history AS (
    SELECT m.task_id,t.position AS task_position,m.seq,0 AS phase,json_extract(m.row_json,'$.ts') AS ts,m.row_json
    FROM plan_task_milestones m JOIN plan_tasks t USING(plan_id,task_id) WHERE m.plan_id=?1
    UNION ALL SELECT NULL,0,seq,1,json_extract(row_json,'$.ts'),row_json FROM plan_strides WHERE plan_id=?1
    ) SELECT task_id,json_remove(row_json,'$.attachments','$.canvas') FROM history
    ORDER BY ts DESC,phase,task_position,seq LIMIT 10";

const REGRESSION: &str = "WITH strides AS MATERIALIZED (
    SELECT json_remove(row_json,'$.text','$.attachments','$.canvas') AS row_json,seq,
        json_extract(row_json,'$.kind') AS kind,json_extract(row_json,'$.ts') AS at,
        row_json->'$.links' AS links FROM plan_strides
    WHERE plan_id=?1 AND json_extract(row_json,'$.kind') IN ('regression','group_close')
    ), records AS (
    SELECT row_json,seq,kind,row_number() OVER (PARTITION BY kind,link.value ORDER BY at DESC,seq DESC) AS rank
    FROM strides,json_each(links) link WHERE link.value LIKE 'group:%'
    ) SELECT DISTINCT row_json FROM records WHERE rank=1";

pub(super) const WHITESPACE: &str = "\t\n\u{b}\u{c}\r \u{85}\u{a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}";

impl PlanDatabase {
    pub(in crate::service_plan) fn status_projection(
        &self,
        plan_id: &str,
        now_ms: u64,
        _selected_learnings: &[String],
    ) -> Result<PlanStatusRead, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let read = state_facts::load_status_projection(&transaction, plan_id, now_ms)?;
        transaction.commit().map_err(db_error)?;
        Ok(read)
    }
}

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    connection
        .create_scalar_function(
            "domios_skill_refs",
            4,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let encoded: String = context.get(3)?;
                let slugs: Vec<String> = serde_json::from_str(&encoded)
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                let mut matches = Vec::new();
                for slug in slugs {
                    let mut matched = false;
                    for index in 0..3 {
                        if let rusqlite::types::ValueRef::Text(bytes) = context.get_raw(index) {
                            let source = std::str::from_utf8(bytes).map_err(|error| {
                                rusqlite::Error::UserFunctionError(Box::new(error))
                            })?;
                            matched |= source
                                .split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                                .any(|token| token == slug);
                        }
                    }
                    if matched {
                        matches.push(slug);
                    }
                }
                Ok(matches.join(" "))
            },
        )
        .map_err(db_error)?;
    connection
        .create_scalar_function(
            "domios_readiness_hint",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let source = match context.get_raw(0) {
                    rusqlite::types::ValueRef::Text(bytes) => std::str::from_utf8(bytes)
                        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?,
                    _ => "",
                };
                let approval = ["approval", "self-review", "review gate"]
                    .iter()
                    .any(|needle| lowercase_contains(source, needle));
                let verification = [
                    "verification",
                    "test plan",
                    "build.sh",
                    "cargo test",
                    "playwright",
                ]
                .iter()
                .any(|needle| lowercase_contains(source, needle));
                Ok(i32::from(approval) | (i32::from(verification) << 1))
            },
        )
        .map_err(db_error)
}

fn lowercase_contains(source: &str, needle: &str) -> bool {
    let mut window = std::collections::VecDeque::with_capacity(needle.len());
    for ch in source.chars().flat_map(char::to_lowercase) {
        window.push_back(ch);
        if window.len() > needle.len() {
            window.pop_front();
        }
        if window.iter().copied().eq(needle.chars()) {
            return true;
        }
    }
    false
}

type HistoryFacts = (BounceSummary, HashMap<String, (usize, Option<u64>)>, usize);

fn history_facts(
    connection: &Connection,
    plan: &crate::service_plan::state::input::StateInput,
) -> Result<HistoryFacts, PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(
        "SELECT t.task_id,(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id),
           (SELECT count(*) FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id AND json_extract(m.row_json,'$.kind')='review'),
           (SELECT max(json_extract(m.row_json,'$.ts')) FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id AND json_extract(m.row_json,'$.kind')='review')
         FROM plan_tasks t WHERE t.plan_id=?1 ORDER BY t.position"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([&plan.id], |row| {
            Ok((
                column::<String>(row, 0)?,
                row.get::<_, usize>(1)?,
                row.get::<_, usize>(2)?,
                row.get::<_, Option<u64>>(3)?,
            ))
        })
        .map_err(db_error)?;
    let mut bounces = BounceSummary::default();
    let mut held = HashMap::new();
    let mut lane_history = 0;
    for (row, task) in rows.zip(&plan.tasks) {
        let (id, attempts, reviews, last_review) = row.map_err(db_error)?;
        bounces.total_attempts += attempts;
        bounces.tasks_on_second_attempt += usize::from(attempts >= 2);
        bounces.first_try_done += usize::from(task.status == TaskStatus::Done && attempts == 1);
        bounces.labelled_bounced +=
            usize::from(task.status_label.as_deref() == Some("review bounced"));
        lane_history += usize::from(
            attempts > 0
                || task.assigned_tab_id.is_some()
                || task.lane.tab_id.is_some()
                || task.lane.branch.is_some()
                || task.lane.worktree_path.is_some(),
        );
        if task.status_label.as_deref() == Some("review held") {
            held.insert(id, (reviews, last_review));
        }
    }
    Ok((bounces, held, lane_history))
}

pub(super) fn control_counts(
    connection: &Connection,
    plan_id: &str,
    now_ms: u64,
) -> Result<crate::plan::CuratorCounts, PlanError> {
    record_query();
    connection.query_row(
        "SELECT coalesce(sum(json_extract(r,'$.outcome') NOT IN ('quiet','nudged','fallback')),0),
                coalesce(sum(json_extract(r,'$.outcome')='quiet'),0),
                coalesce(sum(json_extract(r,'$.outcome')='nudged'),0),
                coalesce(sum(json_extract(r,'$.learnings')),0),
                coalesce(sum(json_extract(r,'$.save_id') IS NOT NULL),0),
                coalesce(sum(json_extract(r,'$.outcome')='fallback'),0)
         FROM (SELECT json_extract(row_json,'$.text') AS r FROM plan_strides
            WHERE plan_id=?1 AND coalesce(json_extract(row_json,'$.ts'),0)>=?2
                AND json_extract(row_json,'$.kind')='curator_round')
         WHERE json_valid(r)",
        params![plan_id,now_ms.saturating_sub(7*24*60*60*1000)], |row| Ok(crate::plan::CuratorCounts {
            rounds:row.get(0)?,quiet:row.get(1)?,nudged:row.get(2)?,learnings:row.get(3)?,saves:row.get(4)?,fallbacks:row.get(5)?,
        })
    ).map_err(db_error)
}
