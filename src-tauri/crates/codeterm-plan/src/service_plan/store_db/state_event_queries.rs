use super::db_error;
use super::read_projections::{column, record_query};
use crate::service_plan::state::mission_control::{
    EventLog, LinkedEventTask, PlanEvent, EVENT_LOG_LIMIT,
};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection};

const EVENT_SQL: &str = r#"
WITH titles AS (
    SELECT a.executor_tab_id AS tab, json_extract(a.row_json,'$.executor_title') AS title,
           row_number() OVER (PARTITION BY a.executor_tab_id ORDER BY t.position DESC,a.position DESC) AS rank
    FROM plan_task_attempts a JOIN plan_tasks t USING(plan_id,task_id)
    WHERE a.plan_id=?1 AND json_extract(a.row_json,'$.executor_title') IS NOT NULL
),
sources AS (
    SELECT coalesce(json_extract(l.value,'$.at'),0) AS ts,0 AS phase,CAST(l.key AS INTEGER) AS position,0 AS subposition,
           'planning' AS kind,
           CASE json_extract(l.value,'$.by.kind') WHEN 'user' THEN 'user'
                ELSE 'planner ' || coalesce(json_extract(l.value,'$.by.tab_id'),'') END AS explicit_author,
           json_extract(l.value,'$.by.tab_id') AS author_tab,NULL AS author_title,NULL AS task_id,NULL AS task_name,
           json_extract(l.value,'$.from') || ' to ' || json_extract(l.value,'$.to') AS text
    FROM plans p,json_each(p.header_json,'$.planning_log') l WHERE p.id=?1
    UNION ALL
    SELECT coalesce(json_extract(row_json,'$.created_at'),0),1,position,-1,
           'task_created','',NULL,NULL,task_id,json_extract(row_json,'$.name'),json_extract(row_json,'$.name')
    FROM plan_tasks WHERE plan_id=?1
    UNION ALL
    SELECT coalesce(json_extract(m.row_json,'$.ts'),0),1,t.position,m.seq,
           json_extract(m.row_json,'$.kind'),NULL,json_extract(m.row_json,'$.from.tab_id'),json_extract(m.row_json,'$.from.title'),
           t.task_id,json_extract(t.row_json,'$.name'),coalesce(json_extract(m.row_json,'$.text'),'')
    FROM plan_task_milestones m JOIN plan_tasks t USING(plan_id,task_id) WHERE m.plan_id=?1
    UNION ALL
    SELECT coalesce(json_extract(row_json,'$.ts'),0),2,seq,0,json_extract(row_json,'$.kind'),NULL,
           json_extract(row_json,'$.from.tab_id'),json_extract(row_json,'$.from.title'),NULL,NULL,
           coalesce(json_extract(row_json,'$.text'),'') FROM plan_strides WHERE plan_id=?1
    UNION ALL
    SELECT coalesce(json_extract(g.value,'$.asked_at'),0),3,CAST(g.key AS INTEGER),0,'gate',NULL,
           json_extract(g.value,'$.asked_by_tab_id'),NULL,NULL,NULL,coalesce(json_extract(g.value,'$.question'),'')
    FROM plans p,json_each(p.header_json,'$.gates') g WHERE p.id=?1
),
resolved AS (
    SELECT *, coalesce(explicit_author,
        (SELECT title FROM titles WHERE tab=author_tab AND rank=1),
        (SELECT CASE WHEN json_extract(header_json,'$.planner.tab_id')=author_tab THEN json_extract(header_json,'$.planner.title') END FROM plans WHERE id=?1),
        (SELECT CASE WHEN owner_tab_id=author_tab THEN json_extract(header_json,'$.owner.title') END FROM plans WHERE id=?1),
        author_title,author_tab,'') AS author,
        CASE WHEN kind IN ('task_created','planning') THEN text ELSE
          trim(substr(trim(text,?2),1,coalesce(nullif(instr(trim(text,?2),char(10)),0)-1,length(trim(text,?2)))),?2) END AS line
    FROM sources
),
ordered AS (
    SELECT *,row_number() OVER (ORDER BY ts,phase,position,subposition) AS ordinal,
        lag(kind) OVER sequence AS prior_kind,lag(author) OVER sequence AS prior_author,
        lag(task_id) OVER sequence AS prior_task,lag(line) OVER sequence AS prior_line
    FROM resolved WINDOW sequence AS (ORDER BY ts,phase,position,subposition)
),
grouped AS (
    SELECT *,sum(CASE WHEN (kind='worker_report' AND prior_kind=kind AND prior_author=author AND prior_task IS task_id)
                          OR (kind='tab_todo_linked' AND prior_kind=kind AND prior_line=line)
                     THEN 0 ELSE 1 END) OVER (ORDER BY ordinal) AS group_id FROM ordered
),
collapsed AS (
    SELECT group_id,min(ordinal) AS first_ordinal,max(ts) AS last_ts,count(*) AS count
    FROM grouped GROUP BY group_id
),
tail AS (SELECT * FROM collapsed ORDER BY first_ordinal DESC LIMIT ?3 OFFSET ?4)
SELECT (SELECT count(*) FROM sources),(SELECT count(*) FROM collapsed),g.ts,t.last_ts,t.count,
       g.kind,g.author,g.author_tab,g.task_id,g.task_name,g.line,
       CASE WHEN g.kind='tab_todo_linked' THEN (SELECT json_group_array(json_object('id',task_id,'name',task_name)) FROM (
           SELECT task_id,task_name,min(ordinal) AS first_link FROM grouped
           WHERE group_id=g.group_id AND task_id IS NOT NULL GROUP BY task_id ORDER BY first_link)) ELSE '[]' END
FROM tail t JOIN grouped g ON g.ordinal=t.first_ordinal ORDER BY t.first_ordinal
"#;

pub(super) fn event_log(connection: &Connection, plan_id: &str) -> Result<EventLog, PlanError> {
    event_window(connection, plan_id, EVENT_LOG_LIMIT, 0)
}

pub(super) fn event_window(
    connection: &Connection,
    plan_id: &str,
    limit: usize,
    offset: usize,
) -> Result<EventLog, PlanError> {
    let whitespace: String = [
        '\t', '\n', '\u{b}', '\u{c}', '\r', ' ', '\u{85}', '\u{a0}', '\u{1680}', '\u{2000}',
        '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}',
        '\u{2008}', '\u{2009}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}',
        '\u{3000}',
    ]
    .into_iter()
    .collect();
    record_query();
    let mut statement = connection.prepare_cached(EVENT_SQL).map_err(db_error)?;
    let rows = statement
        .query_map(params![plan_id, whitespace, limit, offset], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, usize>(1)?,
                PlanEvent {
                    ts: row.get(2)?,
                    last_ts: row.get(3)?,
                    count: row.get(4)?,
                    kind: column(row, 5)?,
                    author: column(row, 6)?,
                    author_tab_id: column(row, 7)?,
                    task_id: column(row, 8)?,
                    task_name: column(row, 9)?,
                    text: column(row, 10)?,
                    linked_tasks: Vec::new(),
                },
                column::<String>(row, 11)?,
            ))
        })
        .map_err(db_error)?;
    let mut result = EventLog::default();
    for row in rows {
        let (recorded, groups, mut event, links) = row.map_err(db_error)?;
        let value =
            super::super::store_sync::parse(&links).map_err(|reason| PlanError::Quarantined {
                plan_id: plan_id.into(),
                reason,
            })?;
        let links = value
            .as_array()
            .ok_or_else(|| PlanError::Io("event task links are not an array".into()))?;
        for link in links {
            event.linked_tasks.push(LinkedEventTask {
                id: link["id"].as_str().unwrap_or_default().into(),
                name: link["name"].as_str().unwrap_or_default().into(),
            });
        }
        result.recorded_events = recorded;
        result.truncated = groups > limit.saturating_add(offset);
        result.entries.push(event);
    }
    Ok(result)
}
