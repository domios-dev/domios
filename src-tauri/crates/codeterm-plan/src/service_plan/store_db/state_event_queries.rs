use super::db_error;
use super::read_projections::{column, record_query};
use crate::service_plan::state::mission_control::{
    EventLog, LinkedEventTask, PlanEvent, EVENT_LOG_LIMIT,
};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection};

const EVENT_SQL: &str = r#"
WITH task_facts AS MATERIALIZED (
    SELECT task_id,position,coalesce(json_extract(row_json,'$.created_at'),0) AS created_at,
        json_extract(row_json,'$.name') AS name FROM plan_tasks WHERE plan_id=?1
), header_facts AS MATERIALIZED (
    SELECT json_extract(header_json,'$.planner.tab_id') AS planner_tab,
        json_extract(header_json,'$.planner.title') AS planner_title,owner_tab_id,
        json_extract(header_json,'$.owner.title') AS owner_title FROM plans WHERE id=?1
), titles AS MATERIALIZED (
    SELECT a.executor_tab_id AS tab, json_extract(a.row_json,'$.executor_title') AS title,
           row_number() OVER (PARTITION BY a.executor_tab_id ORDER BY t.position DESC,a.position DESC) AS rank
    FROM plan_task_attempts a JOIN task_facts t USING(task_id)
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
    SELECT created_at,1,position,-1,'task_created','',NULL,NULL,task_id,name,name FROM task_facts
    UNION ALL
    SELECT coalesce(json_extract(m.row_json,'$.ts'),0),1,t.position,m.seq,
           json_extract(m.row_json,'$.kind'),NULL,json_extract(m.row_json,'$.from.tab_id'),json_extract(m.row_json,'$.from.title'),
           t.task_id,t.name,coalesce(json_extract(m.row_json,'$.text'),'')
    FROM plan_task_milestones m JOIN task_facts t USING(task_id) WHERE m.plan_id=?1
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
        (SELECT CASE WHEN planner_tab=author_tab THEN planner_title END FROM header_facts),
        (SELECT CASE WHEN owner_tab_id=author_tab THEN owner_title END FROM header_facts),
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
           WHERE group_id=g.group_id AND task_id IS NOT NULL GROUP BY task_id ORDER BY first_link)) ELSE '[]' END,t.first_ordinal
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
    super::state_event_window::event_window(connection, plan_id, limit, offset)
}

#[cfg(test)]
pub(super) fn legacy_event_window(
    connection: &Connection,
    plan_id: &str,
    limit: usize,
    offset: usize,
) -> Result<EventLog, PlanError> {
    let whitespace = event_whitespace();
    record_query();
    let mut statement = connection.prepare_cached(EVENT_SQL).map_err(db_error)?;
    let rows = statement
        .query_map(params![plan_id, whitespace, limit, offset], |row| {
            Ok((
                row.get::<_, usize>(0)?,
                row.get::<_, usize>(1)?,
                event_from_row(row)?,
            ))
        })
        .map_err(db_error)?;
    let mut result = EventLog::default();
    for row in rows {
        let (recorded, groups, (event, links)) = row.map_err(db_error)?;
        let event = decode_event_links(plan_id, event, &links)?;
        result.recorded_events = recorded;
        result.truncated = groups > limit.saturating_add(offset);
        result.entries.push(event);
    }
    Ok(result)
}

fn event_whitespace() -> String {
    [
        '\t', '\n', '\u{b}', '\u{c}', '\r', ' ', '\u{85}', '\u{a0}', '\u{1680}', '\u{2000}',
        '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}',
        '\u{2008}', '\u{2009}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}',
        '\u{3000}',
    ]
    .into_iter()
    .collect()
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(PlanEvent, String)> {
    Ok((
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
        column(row, 11)?,
    ))
}

fn decode_event_links(
    plan_id: &str,
    mut event: PlanEvent,
    links: &str,
) -> Result<PlanEvent, PlanError> {
    let value =
        super::super::store_sync::parse(links).map_err(|reason| PlanError::Quarantined {
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
    Ok(event)
}

pub(super) fn event_page(
    connection: &Connection,
    plan_id: &str,
    limit: usize,
    cursor: Option<(u64, &str, &str)>,
) -> Result<Vec<(usize, PlanEvent)>, PlanError> {
    let sql=format!("WITH projection(recorded,groups,ts,last_ts,count,kind,author,author_tab,task_id,task_name,line,links,first_ordinal) AS ({EVENT_SQL}),
        numbered AS (SELECT *,row_number() OVER(ORDER BY first_ordinal)-1 AS ordinal FROM projection),
        page AS (SELECT *, 'stored-'||ordinal||'-'||kind||'-'||ts||'-'||coalesce(task_id,'') AS event_id FROM numbered)
        SELECT recorded,groups,ts,last_ts,count,kind,author,author_tab,task_id,task_name,line,links,ordinal
        FROM page WHERE ?5 IS NULL OR printf('%20s',ts)<?5 OR (printf('%20s',ts)=?5 AND (?1>?6 OR (?1=?6 AND event_id>?7)))
        ORDER BY ts DESC,event_id LIMIT ?8");
    let (at, plan, event) = match cursor {
        Some((at, plan, event)) => (Some(format!("{at:>20}")), Some(plan), Some(event)),
        None => (None, None, None),
    };
    record_query();
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map(
            params![
                plan_id,
                event_whitespace(),
                EVENT_LOG_LIMIT,
                0,
                at,
                plan,
                event,
                limit.min(101)
            ],
            |row| Ok((row.get::<_, usize>(12)?, event_from_row(row)?)),
        )
        .map_err(db_error)?;
    rows.map(|row| {
        let (ordinal, (event, links)) = row.map_err(db_error)?;
        Ok((ordinal, decode_event_links(plan_id, event, &links)?))
    })
    .collect()
}
