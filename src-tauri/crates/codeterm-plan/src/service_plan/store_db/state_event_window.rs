use super::db_error;
use super::read_projections::{column, record_query};
use crate::service_plan::state::mission_control::{EventLog, LinkedEventTask, PlanEvent};
use crate::service_plan::PlanError;
use rusqlite::{Connection, OptionalExtension};
use std::collections::{BTreeSet, HashMap};

pub(super) const TASKS: &str = "SELECT coalesce(json_extract(row_json,'$.created_at'),0),1,position,-1,
    'task_created','',NULL,NULL,task_id,json_extract(row_json,'$.name'),json_extract(row_json,'$.name')
    FROM plan_tasks INDEXED BY plan_tasks_event_order WHERE plan_id=?1
    ORDER BY coalesce(json_extract(row_json,'$.created_at'),0) DESC,position DESC";

pub(super) const STRIDES: &str = "SELECT coalesce(json_extract(row_json,'$.ts'),0),2,seq,0,
    json_extract(row_json,'$.kind'),NULL,json_extract(row_json,'$.from.tab_id'),json_extract(row_json,'$.from.title'),
    NULL,NULL,domios_event_text(json_extract(row_json,'$.kind'),coalesce(json_extract(row_json,'$.text'),''))
    FROM plan_strides INDEXED BY plan_strides_event_order WHERE plan_id=?1
    ORDER BY coalesce(json_extract(row_json,'$.ts'),0) DESC,seq DESC";

pub(super) const MILESTONES: &str = "SELECT coalesce(json_extract(m.row_json,'$.ts'),0),1,t.position,m.seq,
    json_extract(m.row_json,'$.kind'),NULL,json_extract(m.row_json,'$.from.tab_id'),json_extract(m.row_json,'$.from.title'),
    t.task_id,'',domios_event_text(json_extract(m.row_json,'$.kind'),coalesce(json_extract(m.row_json,'$.text'),''))
    FROM plan_task_milestones m INDEXED BY plan_milestones_event_order
    JOIN plan_tasks t ON t.plan_id=m.plan_id AND t.task_id=m.task_id WHERE m.plan_id=?1
    ORDER BY coalesce(json_extract(m.row_json,'$.ts'),0) DESC,t.position DESC,m.seq DESC";

const HEADER_EVENTS: &str = "SELECT coalesce(json_extract(l.value,'$.at'),0),0,CAST(l.key AS INTEGER),0,'planning',
    CASE json_extract(l.value,'$.by.kind') WHEN 'user' THEN 'user'
    ELSE 'planner ' || coalesce(json_extract(l.value,'$.by.tab_id'),'') END,
    json_extract(l.value,'$.by.tab_id'),NULL,NULL,NULL,
    json_extract(l.value,'$.from') || ' to ' || json_extract(l.value,'$.to')
    FROM plans p,json_each(p.header_json,'$.planning_log') l WHERE p.id=?1
    UNION ALL
    SELECT coalesce(json_extract(g.value,'$.asked_at'),0),3,CAST(g.key AS INTEGER),0,'gate',NULL,
    json_extract(g.value,'$.asked_by_tab_id'),NULL,NULL,NULL,domios_event_text('gate',coalesce(json_extract(g.value,'$.question'),''))
    FROM plans p,json_each(p.header_json,'$.gates') g WHERE p.id=?1
    ORDER BY 1 DESC,2 DESC,3 DESC,4 DESC";

pub(super) const AUTHOR: &str = "SELECT (
    SELECT json_extract(a.row_json,'$.executor_title') FROM plan_task_attempts a INDEXED BY plan_attempts_event_author
    WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND a.executor_tab_id=?2
      AND json_extract(a.row_json,'$.executor_title') IS NOT NULL ORDER BY a.position DESC LIMIT 1
    ) AS title FROM plan_tasks t INDEXED BY plan_tasks_handle_order
    WHERE t.plan_id=?1 AND title IS NOT NULL ORDER BY t.position DESC LIMIT 1";

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    connection
        .create_scalar_function(
            "domios_event_text",
            2,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let kind = context
                    .get_raw(0)
                    .as_str()
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                let text = context
                    .get_raw(1)
                    .as_str()
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                Ok(if matches!(kind, "task_created" | "planning") {
                    text.to_string()
                } else {
                    crate::service_plan::state::mission_control::first_line(text)
                })
            },
        )
        .map_err(db_error)
}

type Order = (u64, i64, i64, i64);

struct SourceEvent {
    order: Order,
    explicit_author: Option<String>,
    author_title: Option<String>,
    event: PlanEvent,
}

impl SourceEvent {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let kind: String = column(row, 4)?;
        let line: String = column(row, 10)?;
        let ts = row.get(0)?;
        Ok(Self {
            order: (ts, row.get(1)?, row.get(2)?, row.get(3)?),
            explicit_author: column(row, 5)?,
            author_title: column(row, 7)?,
            event: PlanEvent {
                ts,
                last_ts: ts,
                count: 1,
                kind,
                author: String::new(),
                author_tab_id: column(row, 6)?,
                task_id: column(row, 8)?,
                task_name: column(row, 9)?,
                text: line,
                linked_tasks: Vec::new(),
            },
        })
    }
}

struct Authors<'a> {
    connection: &'a Connection,
    plan_id: &'a str,
    names: HashMap<String, Option<String>>,
}

impl Authors<'_> {
    fn resolve(&mut self, source: &SourceEvent) -> Result<String, PlanError> {
        if let Some(author) = &source.explicit_author {
            return Ok(author.clone());
        }
        if let Some(tab) = &source.event.author_tab_id {
            if !self.names.contains_key(tab) {
                record_query();
                let attempt: Option<String> = self
                    .connection
                    .query_row(AUTHOR, rusqlite::params![self.plan_id, tab], |row| {
                        column(row, 0)
                    })
                    .optional()
                    .map_err(db_error)?;
                let name = if attempt.is_some() {
                    attempt
                } else {
                    record_query();
                    self.connection.query_row(
                        "SELECT coalesce(
                            CASE WHEN json_extract(header_json,'$.planner.tab_id')=?2 THEN json_extract(header_json,'$.planner.title') END,
                            CASE WHEN owner_tab_id=?2 THEN json_extract(header_json,'$.owner.title') END)
                         FROM plans WHERE id=?1",
                        rusqlite::params![self.plan_id,tab], |row| column(row,0),
                    ).map_err(db_error)?
                };
                self.names.insert(tab.clone(), name);
            }
            if let Some(Some(name)) = self.names.get(tab) {
                return Ok(name.clone());
            }
        }
        Ok(source
            .author_title
            .clone()
            .or_else(|| source.event.author_tab_id.clone())
            .unwrap_or_default())
    }
}

fn resolve_task_names(
    connection: &Connection,
    plan_id: &str,
    entries: &mut [PlanEvent],
) -> Result<(), PlanError> {
    let ids = entries
        .iter()
        .filter_map(|entry| {
            entry
                .task_id
                .as_ref()
                .filter(|_| entry.task_name.as_deref() == Some(""))
        })
        .chain(
            entries
                .iter()
                .flat_map(|entry| entry.linked_tasks.iter().map(|task| &task.id)),
        )
        .cloned()
        .collect::<BTreeSet<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let selected = serde_json::to_string(&ids).map_err(super::json_error)?;
    record_query();
    let mut statement=connection.prepare_cached("SELECT task_id,json_extract(row_json,'$.name') FROM plan_tasks WHERE plan_id=?1 AND task_id IN(SELECT value FROM json_each(?2))").map_err(db_error)?;
    let names = statement
        .query_map(rusqlite::params![plan_id, selected], |row| {
            Ok((column::<String>(row, 0)?, column::<Option<String>>(row, 1)?))
        })
        .map_err(db_error)?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(db_error)?;
    if ids.iter().any(|id| !names.contains_key(id)) {
        return Err(PlanError::Io(
            "event task disappeared during its committed read".into(),
        ));
    }
    for entry in entries {
        if entry.task_name.as_deref() == Some("") {
            if let Some(name) = entry.task_id.as_ref().and_then(|id| names.get(id)) {
                entry.task_name = name.clone();
            }
        }
        entry
            .linked_tasks
            .retain_mut(|task| match names.get(&task.id) {
                Some(Some(name)) => {
                    task.name = name.clone();
                    true
                }
                Some(None) => false,
                None => true,
            });
    }
    Ok(())
}

fn collapses(newer: &PlanEvent, older: &PlanEvent) -> bool {
    (newer.kind == "worker_report"
        && older.kind == newer.kind
        && newer.author == older.author
        && newer.task_id == older.task_id)
        || (newer.kind == "tab_todo_linked" && older.kind == newer.kind && newer.text == older.text)
}

fn merge(newer: PlanEvent, mut older: PlanEvent) -> PlanEvent {
    older.count += newer.count;
    older.last_ts = newer.last_ts.max(older.last_ts);
    if older.kind == "tab_todo_linked" {
        let mut links = newer.linked_tasks;
        if let (Some(id), Some(name)) = (&older.task_id, &older.task_name) {
            links.retain(|link| &link.id != id);
            links.insert(
                0,
                LinkedEventTask {
                    id: id.clone(),
                    name: name.clone(),
                },
            );
        }
        older.linked_tasks = links;
    }
    older
}

pub(super) fn event_window(
    connection: &Connection,
    plan_id: &str,
    limit: usize,
    offset: usize,
) -> Result<EventLog, PlanError> {
    if limit == 0 {
        return Ok(EventLog::default());
    }
    let mut statements = [TASKS, MILESTONES, STRIDES, HEADER_EVENTS]
        .into_iter()
        .map(|sql| {
            record_query();
            connection.prepare_cached(sql).map_err(db_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut cursors = statements
        .iter_mut()
        .map(|statement| statement.query([plan_id]).map_err(db_error))
        .collect::<Result<Vec<_>, _>>()?;
    let mut heads = cursors
        .iter_mut()
        .map(|cursor| {
            cursor
                .next()
                .map_err(db_error)?
                .map(SourceEvent::read)
                .transpose()
                .map_err(db_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut authors = Authors {
        connection,
        plan_id,
        names: HashMap::new(),
    };
    let mut result = EventLog::default();
    let mut group: Option<PlanEvent> = None;
    let mut skipped = 0;
    while let Some(index) = heads
        .iter()
        .enumerate()
        .filter_map(|(i, head)| head.as_ref().map(|h| (i, h.order)))
        .max_by_key(|(_, order)| *order)
        .map(|(i, _)| i)
    {
        let Some(mut source) = heads[index].take() else {
            break;
        };
        record_visited();
        if source.event.kind == "worker_report" {
            source.event.author = authors.resolve(&source)?;
        }
        if group
            .as_ref()
            .is_some_and(|newer| !collapses(newer, &source.event))
        {
            if let Some(finished) = group.take() {
                if skipped < offset {
                    skipped += 1;
                } else {
                    result.entries.push(finished);
                }
            }
            if result.entries.len() >= limit {
                result.truncated = true;
                break;
            }
        }
        source.event.author = authors.resolve(&source)?;
        if source.event.kind == "tab_todo_linked" {
            if let (Some(id), Some(name)) = (&source.event.task_id, &source.event.task_name) {
                source.event.linked_tasks.push(LinkedEventTask {
                    id: id.clone(),
                    name: name.clone(),
                });
            }
        }
        group = Some(match group.take() {
            Some(newer) => merge(newer, source.event),
            None => source.event,
        });
        heads[index] = cursors[index]
            .next()
            .map_err(db_error)?
            .map(SourceEvent::read)
            .transpose()
            .map_err(db_error)?;
    }
    if let Some(finished) = group {
        if skipped >= offset && result.entries.len() < limit {
            result.entries.push(finished);
        }
    }
    resolve_task_names(connection, plan_id, &mut result.entries)?;
    if !result.entries.is_empty() {
        record_query();
        result.recorded_events = connection
            .query_row(
                "SELECT (SELECT count(*) FROM plan_tasks WHERE plan_id=?1)
                +(SELECT count(*) FROM plan_task_milestones WHERE plan_id=?1)
                +(SELECT count(*) FROM plan_strides WHERE plan_id=?1)
                +coalesce(json_array_length(header_json,'$.planning_log'),0)
                +coalesce(json_array_length(header_json,'$.gates'),0) FROM plans WHERE id=?1",
                [plan_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
    }
    result.entries.reverse();
    Ok(result)
}

#[cfg(test)]
thread_local! { static VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn record_visited() {
    #[cfg(test)]
    VISITED.with(|visited| visited.set(visited.get() + 1));
}

#[cfg(test)]
mod tests;
