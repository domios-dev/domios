//! SQL predicates over the stored plan rows. A consumer names the plans it needs,
//! the store answers with their ids from SQL and loads only those.

use crate::plan::{PlanMode, PlanStatus, TaskStatus};
use rusqlite::types::Value;

/// One predicate over a plan row; task, attempt and element predicates hold when
/// at least one of that plan's rows matches every condition they carry.
#[derive(Debug, Clone)]
pub enum PlanMatch {
    Ids(Vec<String>),
    Status(PlanStatus),
    Mode(PlanMode),
    /// The stored `project_path` equals this path.
    Project(String),
    Teams(Vec<String>),
    HasTeam,
    Owners(Vec<String>),
    OwnerSession(String),
    /// The plan's Planner is bound to this tab.
    Planner(String),
    /// A header field (a JSON path such as `$.control_loop.running`) holds a value.
    HeaderSet(&'static str),
    Task(TaskMatch),
    Attempt(AttemptMatch),
    /// A plan section, at any depth, carries this id.
    SectionId(String),
    /// A canvas element of this kind whose content JSON paths equal the values.
    Element {
        kind: String,
        fields: Vec<(&'static str, String)>,
    },
    LoopId(String),
    SpecRef(String),
    SpecLink(String),
    Any(Vec<PlanMatch>),
    All(Vec<PlanMatch>),
}

#[derive(Debug, Clone, Default)]
pub struct TaskMatch {
    pub statuses: Option<Vec<TaskStatus>>,
    pub assigned_tabs: Option<Vec<String>>,
    pub lane_tabs: Option<Vec<String>>,
    pub lane_worktree: Option<String>,
    /// Task id, key, handle or pre-key fallback key; names compare ASCII case-insensitively.
    pub reference: Option<String>,
    /// JSON path of the task row that must equal the value.
    pub fields: Vec<(&'static str, String)>,
    pub set: Vec<&'static str>,
    pub unset: Vec<&'static str>,
}

/// `Unclosed` has no `closed_at`; `Open` has neither `closed_at` nor `retired_at`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AttemptState {
    #[default]
    Any,
    Unclosed,
    Open,
}

#[derive(Debug, Clone, Default)]
pub struct AttemptMatch {
    pub executor_tabs: Option<Vec<String>>,
    pub state: AttemptState,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
    pub task_statuses: Option<Vec<TaskStatus>>,
    /// JSON path of the attempt row that must equal the value.
    pub fields: Vec<(&'static str, String)>,
}

pub const NON_TERMINAL_TASK_STATUSES: [TaskStatus; 5] = [
    TaskStatus::Pending,
    TaskStatus::InProgress,
    TaskStatus::InReview,
    TaskStatus::Blocked,
    TaskStatus::Paused,
];

impl PlanMatch {
    pub fn active() -> Self {
        Self::Status(PlanStatus::Active)
    }

    pub fn owner(tab_id: &str) -> Self {
        Self::Owners(vec![tab_id.to_string()])
    }

    pub fn team(team_id: &str) -> Self {
        Self::Teams(vec![team_id.to_string()])
    }

    pub fn and(self, other: PlanMatch) -> Self {
        match self {
            Self::All(mut all) => {
                all.push(other);
                Self::All(all)
            }
            first => Self::All(vec![first, other]),
        }
    }
}

impl TaskMatch {
    /// Tasks assigned to the tab, or whose lane names it, in an unfinished status.
    pub fn bound_to(tab_id: &str) -> PlanMatch {
        PlanMatch::Any(vec![
            PlanMatch::Task(Self {
                statuses: Some(NON_TERMINAL_TASK_STATUSES.to_vec()),
                assigned_tabs: Some(vec![tab_id.to_string()]),
                ..Default::default()
            }),
            PlanMatch::Task(Self {
                statuses: Some(NON_TERMINAL_TASK_STATUSES.to_vec()),
                lane_tabs: Some(vec![tab_id.to_string()]),
                ..Default::default()
            }),
        ])
    }

    /// Tasks in one of these statuses the tab is assigned to, names in its lane, or
    /// executes through an unclosed attempt.
    pub fn held_by(tab_id: &str, statuses: &[TaskStatus]) -> PlanMatch {
        let tabs = vec![tab_id.to_string()];
        PlanMatch::Any(vec![
            PlanMatch::Task(Self {
                statuses: Some(statuses.to_vec()),
                assigned_tabs: Some(tabs.clone()),
                ..Default::default()
            }),
            PlanMatch::Task(Self {
                statuses: Some(statuses.to_vec()),
                lane_tabs: Some(tabs.clone()),
                ..Default::default()
            }),
            PlanMatch::Attempt(AttemptMatch {
                executor_tabs: Some(tabs),
                state: AttemptState::Unclosed,
                task_statuses: Some(statuses.to_vec()),
                ..Default::default()
            }),
        ])
    }

    pub fn assigned(tab_id: &str, statuses: &[TaskStatus]) -> PlanMatch {
        PlanMatch::Task(Self {
            statuses: Some(statuses.to_vec()),
            assigned_tabs: Some(vec![tab_id.to_string()]),
            ..Default::default()
        })
    }

    pub fn with_status(statuses: &[TaskStatus]) -> PlanMatch {
        PlanMatch::Task(Self {
            statuses: Some(statuses.to_vec()),
            ..Default::default()
        })
    }

    pub fn reference(reference: &str) -> PlanMatch {
        PlanMatch::Task(Self {
            reference: Some(reference.to_string()),
            ..Default::default()
        })
    }
}

impl AttemptMatch {
    pub fn executor(tab_id: &str, state: AttemptState) -> Self {
        Self {
            executor_tabs: Some(vec![tab_id.to_string()]),
            state,
            ..Default::default()
        }
    }

    pub fn into_match(self) -> PlanMatch {
        PlanMatch::Attempt(self)
    }
}

pub(super) fn plan_status_text(status: &PlanStatus) -> &'static str {
    match status {
        PlanStatus::Active => "active",
        PlanStatus::Archived => "archived",
    }
}

fn plan_mode_text(mode: &PlanMode) -> &'static str {
    match mode {
        PlanMode::Planning => "planning",
        PlanMode::Approved => "approved",
        PlanMode::Execution => "execution",
        PlanMode::Paused => "paused",
        PlanMode::Complete => "complete",
    }
}

fn task_status_text(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::InReview => "in_review",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Paused => "paused",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
    }
}

/// A damaged header must not fail the query: the guard keeps the JSON functions off it.
fn header(expression: &str) -> String {
    format!("(CASE WHEN json_valid(p.header_json) THEN {expression} END)")
}

/// Compiles a predicate into a `WHERE` clause over `plans p` and its bound parameters.
pub(super) fn compile(predicate: &PlanMatch) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut params = Vec::new();
    plan_clause(predicate, &mut sql, &mut params);
    (sql, params)
}

fn push_in(sql: &mut String, params: &mut Vec<Value>, column: &str, values: &[String]) {
    if values.is_empty() {
        sql.push('0');
        return;
    }
    sql.push_str(column);
    sql.push_str(" IN (");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        sql.push('?');
        params.push(Value::Text(value.clone()));
    }
    sql.push(')');
}

fn push_statuses(sql: &mut String, params: &mut Vec<Value>, column: &str, statuses: &[TaskStatus]) {
    let values: Vec<String> = statuses
        .iter()
        .map(|status| task_status_text(status).to_string())
        .collect();
    push_in(sql, params, column, &values);
}

fn push_text(params: &mut Vec<Value>, value: &str) {
    params.push(Value::Text(value.to_string()));
}

fn plan_clause(predicate: &PlanMatch, sql: &mut String, params: &mut Vec<Value>) {
    match predicate {
        PlanMatch::Ids(ids) => push_in(sql, params, "p.id", ids),
        PlanMatch::Status(status) => {
            sql.push_str("p.status = ?");
            push_text(params, plan_status_text(status));
        }
        PlanMatch::Mode(mode) => {
            sql.push_str(&header("json_extract(p.header_json, '$.mode')"));
            sql.push_str(" = ?");
            push_text(params, plan_mode_text(mode));
        }
        PlanMatch::Project(path) => {
            sql.push_str("p.project_path = ?");
            push_text(params, path);
        }
        PlanMatch::Teams(teams) => push_in(sql, params, "p.team_id", teams),
        PlanMatch::HasTeam => sql.push_str("p.team_id IS NOT NULL"),
        PlanMatch::Owners(tabs) => push_in(sql, params, &header("p.owner_tab_id"), tabs),
        PlanMatch::OwnerSession(session_id) => {
            sql.push_str(&header("json_extract(p.header_json, '$.owner.session_id')"));
            sql.push_str(" = ?");
            push_text(params, session_id);
        }
        PlanMatch::Planner(tab_id) => {
            sql.push_str(&header(
                "(json_extract(p.header_json, '$.planner.kind') = 'tab' AND json_extract(p.header_json, '$.planner.tab_id') = ?)",
            ));
            push_text(params, tab_id);
        }
        PlanMatch::HeaderSet(path) => {
            sql.push_str(&header("json_extract(p.header_json, ?) IS NOT NULL"));
            push_text(params, path);
        }
        PlanMatch::Task(task) => task_clause(task, sql, params),
        PlanMatch::Attempt(attempt) => attempt_clause(attempt, sql, params),
        PlanMatch::SectionId(id) => {
            sql.push_str(
                "EXISTS (SELECT 1 FROM plan_sections s, json_tree(s.row_json) j
                 WHERE s.plan_id = p.id AND j.key = 'id' AND j.value = ?)",
            );
            push_text(params, id);
        }
        PlanMatch::Element { kind, fields } => {
            sql.push_str(
                "EXISTS (SELECT 1 FROM plan_elements e
                 WHERE e.plan_id = p.id AND json_extract(e.row_json, '$.kind') = ?",
            );
            push_text(params, kind);
            push_fields(sql, params, "e.row_json", fields);
            sql.push(')');
        }
        PlanMatch::LoopId(id) => {
            sql.push_str(&header(
                "EXISTS (SELECT 1 FROM json_each(p.header_json, '$.loops') l WHERE json_extract(l.value, '$.id') = ?)",
            ));
            push_text(params, id);
        }
        PlanMatch::SpecRef(id) => {
            sql.push_str(&header(
                "EXISTS (SELECT 1 FROM json_each(p.header_json, '$.spec_refs') r WHERE r.value = ?)",
            ));
            push_text(params, id);
        }
        PlanMatch::SpecLink(id) => {
            sql.push_str(&header(
                "EXISTS (SELECT 1 FROM json_each(p.header_json, '$.spec_links') r WHERE json_extract(r.value, '$.spec_id') = ?)",
            ));
            push_text(params, id);
        }
        PlanMatch::Any(parts) => join(parts, " OR ", "0", sql, params),
        PlanMatch::All(parts) => join(parts, " AND ", "1", sql, params),
    }
}

fn join(
    parts: &[PlanMatch],
    separator: &str,
    empty: &str,
    sql: &mut String,
    params: &mut Vec<Value>,
) {
    if parts.is_empty() {
        sql.push_str(empty);
        return;
    }
    sql.push('(');
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            sql.push_str(separator);
        }
        plan_clause(part, sql, params);
    }
    sql.push(')');
}

fn push_fields(
    sql: &mut String,
    params: &mut Vec<Value>,
    row: &str,
    fields: &[(&'static str, String)],
) {
    for (path, value) in fields {
        sql.push_str(&format!(" AND json_extract({row}, ?) = ?"));
        push_text(params, path);
        push_text(params, value);
    }
}

const FALLBACK_KEY: &str =
    "upper(CASE WHEN substr(t.task_id, 1, 5) = 'task_' THEN substr(t.task_id, 6) ELSE t.task_id END)";

fn task_clause(task: &TaskMatch, sql: &mut String, params: &mut Vec<Value>) {
    sql.push_str("EXISTS (SELECT 1 FROM plan_tasks t WHERE t.plan_id = p.id");
    if let Some(statuses) = &task.statuses {
        sql.push_str(" AND ");
        push_statuses(sql, params, "t.status", statuses);
    }
    if let Some(tabs) = &task.assigned_tabs {
        sql.push_str(" AND ");
        push_in(sql, params, "t.assigned_tab_id", tabs);
    }
    if let Some(tabs) = &task.lane_tabs {
        sql.push_str(" AND ");
        push_in(
            sql,
            params,
            "json_extract(t.row_json, '$.lane.tab_id')",
            tabs,
        );
    }
    if let Some(path) = &task.lane_worktree {
        sql.push_str(" AND t.lane_worktree_path = ?");
        push_text(params, path);
    }
    if let Some(reference) = &task.reference {
        sql.push_str(&format!(
            " AND (t.task_id = ?
                OR upper(json_extract(t.row_json, '$.key')) = upper(?)
                OR upper(json_extract(t.row_json, '$.handle')) = upper(?)
                OR (json_extract(t.row_json, '$.key') IS NULL AND {FALLBACK_KEY} = upper(?)))"
        ));
        for _ in 0..4 {
            push_text(params, reference);
        }
    }
    push_fields(sql, params, "t.row_json", &task.fields);
    for path in &task.set {
        sql.push_str(" AND json_extract(t.row_json, ?) IS NOT NULL");
        push_text(params, path);
    }
    for path in &task.unset {
        sql.push_str(" AND json_extract(t.row_json, ?) IS NULL");
        push_text(params, path);
    }
    sql.push(')');
}

fn attempt_clause(attempt: &AttemptMatch, sql: &mut String, params: &mut Vec<Value>) {
    sql.push_str(
        "EXISTS (SELECT 1 FROM plan_task_attempts a JOIN plan_tasks t
            ON t.plan_id = a.plan_id AND t.task_id = a.task_id
         WHERE a.plan_id = p.id",
    );
    if let Some(tabs) = &attempt.executor_tabs {
        sql.push_str(" AND ");
        push_in(sql, params, "a.executor_tab_id", tabs);
    }
    match attempt.state {
        AttemptState::Any => {}
        AttemptState::Unclosed => {
            sql.push_str(" AND json_extract(a.row_json, '$.closed_at') IS NULL")
        }
        AttemptState::Open => sql.push_str(" AND a.open"),
    }
    if let Some(session_id) = &attempt.session_id {
        sql.push_str(" AND a.session_id = ?");
        push_text(params, session_id);
    }
    if let Some(nonce) = &attempt.binding_nonce {
        sql.push_str(" AND a.session_binding_nonce = ?");
        push_text(params, nonce);
    }
    if let Some(statuses) = &attempt.task_statuses {
        sql.push_str(" AND ");
        push_statuses(sql, params, "t.status", statuses);
    }
    push_fields(sql, params, "a.row_json", &attempt.fields);
    sql.push(')');
}
