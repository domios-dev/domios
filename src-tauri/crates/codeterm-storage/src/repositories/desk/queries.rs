use super::records::{
    DeskAgentSessionIdentity, DeskLayoutNode, DeskLeafFields, DeskSnapshot, DeskState, DeskTab,
    DeskTeam, DeskTeamLifecycle, DeskWorkspace, MAX_DESK_LAYOUT_DEPTH, MAX_DESK_LAYOUT_NODES,
};
use crate::error::{sqlite_error, StorageError};
use crate::repositories::workspaces::WorkspaceRecord;
use crate::repositories::{from_sql_u64, Revision};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::collections::HashMap;

struct WorkspaceRow {
    record: WorkspaceRecord,
    sort_order: i32,
    active_tab_id: Option<String>,
}

struct TabRow {
    id: String,
    workspace_id: String,
    title: String,
    description: Option<String>,
    sort_order: i32,
    revision: Revision,
}

struct LayoutRow {
    id: i64,
    group_id: String,
    parent_id: Option<i64>,
    ordinal: i64,
    kind: String,
    orientation: Option<String>,
    ratio: Option<f64>,
    tab_id: Option<String>,
    cwd: Option<String>,
    fields_json: Option<String>,
}

pub(super) fn load(conn: &Connection) -> Result<DeskState, StorageError> {
    let meta: Option<(String, i64, Option<String>, i64, i64)> = conn
        .query_row(
            "SELECT version, timestamp, active_workspace_id, revision, workspace_order_revision \
             FROM desk_meta WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| sqlite_error("read desk metadata", error))?;
    let Some((version, timestamp, active_workspace_id, desk_revision, workspace_order_revision)) =
        meta
    else {
        return Err(StorageError::NotFound {
            entity: "desk snapshot".into(),
        });
    };

    let all_workspace_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
        .map_err(|error| sqlite_error("count desk workspaces", error))?;
    let mut workspace_rows = Vec::new();
    let mut workspace_revisions = HashMap::new();
    let mut workspace_order_revisions = HashMap::new();
    let mut statement = conn
        .prepare(
            "SELECT w.id, w.name, w.cwd, w.provisioned_cwd, w.protected, \
             w.task_key_prefix, w.task_key_next, d.sort_order, d.active_tab_id, \
             d.revision, d.tab_order_revision \
             FROM workspaces w JOIN desk_workspace_state d ON d.workspace_id = w.id \
             ORDER BY w.ordinal",
        )
        .map_err(|error| sqlite_error("prepare desk workspaces", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i32>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })
        .map_err(|error| sqlite_error("query desk workspaces", error))?;
    for row in rows {
        let (
            id,
            name,
            cwd,
            provisioned_cwd,
            protected,
            task_key_prefix,
            task_key_next,
            sort_order,
            active_tab_id,
            revision,
            tab_order_revision,
        ) = row.map_err(|error| sqlite_error("read desk workspace", error))?;
        let id = crate::repositories::workspaces::WorkspaceId::new(id)?;
        let record = WorkspaceRecord {
            id: id.clone(),
            name,
            cwd,
            provisioned_cwd,
            protected,
            task_key_prefix,
            task_key_next: from_sql_u64("workspace task key counter", task_key_next)?,
        };
        let revision = Revision(from_sql_u64("desk workspace revision", revision)?);
        let tab_order_revision =
            Revision(from_sql_u64("desk tab order revision", tab_order_revision)?);
        workspace_revisions.insert(id.as_str().to_owned(), revision);
        workspace_order_revisions.insert(id.as_str().to_owned(), tab_order_revision);
        workspace_rows.push(WorkspaceRow {
            record,
            sort_order,
            active_tab_id,
        });
    }
    if workspace_rows.len() as i64 != all_workspace_count {
        return Err(corrupt("workspace rows are missing desk state"));
    }

    let tabs = load_tabs(conn)?;
    let layouts = load_layouts(conn)?;
    let layout_count: usize = layouts.values().map(Vec::len).sum();
    if layout_count > MAX_DESK_LAYOUT_NODES {
        return Err(corrupt("layout exceeds node bound"));
    }
    let mut tabs_by_workspace: HashMap<String, Vec<TabRow>> = HashMap::new();
    for tab in tabs {
        tabs_by_workspace
            .entry(tab.workspace_id.clone())
            .or_default()
            .push(tab);
    }
    let mut tab_revisions = HashMap::new();
    let mut visited_layouts = std::collections::HashSet::new();
    let mut workspaces = Vec::with_capacity(workspace_rows.len());
    for workspace in workspace_rows {
        let mut workspace_tabs = Vec::new();
        let tabs = tabs_by_workspace
            .remove(workspace.record.id.as_str())
            .unwrap_or_default();
        for tab in tabs {
            tab_revisions.insert(tab.id.clone(), tab.revision);
            let roots = layouts
                .get(&(tab.id.clone(), None))
                .ok_or_else(|| corrupt(format!("tab {} has no layout root", tab.id)))?;
            if roots.len() != 1 {
                return Err(corrupt(format!(
                    "tab {} has {} layout roots",
                    tab.id,
                    roots.len()
                )));
            }
            let layout = build_layout(&roots[0], &layouts, &mut visited_layouts, 0)?;
            workspace_tabs.push(DeskTab {
                id: tab.id.clone(),
                title: tab.title.clone(),
                description: tab.description.clone(),
                sort_order: tab.sort_order,
                layout,
            });
        }
        workspaces.push(DeskWorkspace {
            record: workspace.record,
            sort_order: workspace.sort_order,
            active_tab_id: workspace.active_tab_id,
            tabs: workspace_tabs,
        });
    }
    if !tabs_by_workspace.is_empty() || visited_layouts.len() != layout_count {
        return Err(corrupt("desk tabs or layout nodes are orphaned"));
    }

    let mut team_revisions = HashMap::new();
    let teams = load_teams(conn, &mut team_revisions)?;
    let agent_session_identities = load_identities(conn)?;
    let snapshot = DeskSnapshot {
        version,
        timestamp: from_sql_u64("desk timestamp", timestamp)?,
        active_workspace_id,
        workspaces,
        teams,
        agent_session_identities,
    };
    Ok(DeskState {
        snapshot,
        revision: Revision(from_sql_u64("desk revision", desk_revision)?),
        workspace_order_revision: Revision(from_sql_u64(
            "desk workspace order revision",
            workspace_order_revision,
        )?),
        workspace_revisions,
        workspace_order_revisions,
        tab_revisions,
        team_revisions,
    })
}

pub(super) fn load_workspace_record_revisions(
    conn: &Connection,
) -> Result<HashMap<String, Revision>, StorageError> {
    load_revisions(
        conn,
        "SELECT id, revision FROM workspaces",
        "workspace record revision",
    )
}

pub(super) fn load_identity_revisions(
    conn: &Connection,
) -> Result<HashMap<(String, String), Revision>, StorageError> {
    let mut statement = conn
        .prepare("SELECT provider, session_id, revision FROM agent_session_identities")
        .map_err(|error| sqlite_error("prepare agent identity revisions", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| sqlite_error("query agent identity revisions", error))?;
    let mut revisions = HashMap::new();
    for row in rows {
        let (provider, session_id, revision) =
            row.map_err(|error| sqlite_error("read agent identity revision", error))?;
        revisions.insert(
            (provider, session_id),
            Revision(from_sql_u64("agent identity revision", revision)?),
        );
    }
    Ok(revisions)
}

fn load_revisions(
    conn: &Connection,
    sql: &str,
    field: &'static str,
) -> Result<HashMap<String, Revision>, StorageError> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare desk revisions", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| sqlite_error("query desk revisions", error))?;
    let mut revisions = HashMap::new();
    for row in rows {
        let (id, revision) = row.map_err(|error| sqlite_error("read desk revision", error))?;
        revisions.insert(id, Revision(from_sql_u64(field, revision)?));
    }
    Ok(revisions)
}

fn load_tabs(conn: &Connection) -> Result<Vec<TabRow>, StorageError> {
    let mut statement = conn
        .prepare(
            "SELECT id, workspace_id, title, description, sort_order, revision \
             FROM desk_tabs ORDER BY workspace_id, ordinal",
        )
        .map_err(|error| sqlite_error("prepare desk tabs", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|error| sqlite_error("query desk tabs", error))?;
    rows.map(|row| {
        let (id, workspace_id, title, description, sort_order, revision) =
            row.map_err(|error| sqlite_error("read desk tab", error))?;
        Ok(TabRow {
            id,
            workspace_id,
            title,
            description,
            sort_order,
            revision: Revision(from_sql_u64("desk tab revision", revision)?),
        })
    })
    .collect()
}

fn load_layouts(
    conn: &Connection,
) -> Result<HashMap<(String, Option<i64>), Vec<LayoutRow>>, StorageError> {
    let mut statement = conn
        .prepare(
            "SELECT id, group_id, parent_id, ordinal, kind, orientation, ratio, tab_id, cwd, fields_json \
             FROM desk_layout_nodes ORDER BY group_id, parent_id, ordinal",
        )
        .map_err(|error| sqlite_error("prepare desk layout", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok(LayoutRow {
                id: row.get(0)?,
                group_id: row.get(1)?,
                parent_id: row.get(2)?,
                ordinal: row.get(3)?,
                kind: row.get(4)?,
                orientation: row.get(5)?,
                ratio: row.get(6)?,
                tab_id: row.get(7)?,
                cwd: row.get(8)?,
                fields_json: row.get(9)?,
            })
        })
        .map_err(|error| sqlite_error("query desk layout", error))?;
    let mut by_parent: HashMap<(String, Option<i64>), Vec<LayoutRow>> = HashMap::new();
    for row in rows {
        let row = row.map_err(|error| sqlite_error("read desk layout", error))?;
        by_parent
            .entry((row.group_id.clone(), row.parent_id))
            .or_default()
            .push(row);
    }
    Ok(by_parent)
}

fn build_layout(
    row: &LayoutRow,
    by_parent: &HashMap<(String, Option<i64>), Vec<LayoutRow>>,
    visited: &mut std::collections::HashSet<i64>,
    depth: usize,
) -> Result<DeskLayoutNode, StorageError> {
    if depth > MAX_DESK_LAYOUT_DEPTH || !visited.insert(row.id) {
        return Err(corrupt("layout is cyclic or exceeds depth bound"));
    }
    match row.kind.as_str() {
        "leaf" => {
            let tab_id = row
                .tab_id
                .clone()
                .ok_or_else(|| corrupt("leaf has no tab identity"))?;
            let json = row
                .fields_json
                .as_deref()
                .ok_or_else(|| corrupt("leaf has no typed fields"))?;
            let fields: Value = serde_json::from_str(json)
                .map_err(|error| corrupt(format!("invalid leaf fields: {error}")))?;
            let fields = DeskLeafFields { values: fields };
            fields.validate()?;
            Ok(DeskLayoutNode::Leaf {
                tab_id,
                cwd: row.cwd.clone(),
                fields,
            })
        }
        "split" => {
            let fields_json = row
                .fields_json
                .as_deref()
                .ok_or_else(|| corrupt("split has no fields"))?;
            let fields: Value = serde_json::from_str(fields_json)
                .map_err(|error| corrupt(format!("invalid split fields: {error}")))?;
            let fields = fields
                .as_object()
                .cloned()
                .ok_or_else(|| corrupt("split fields are not an object"))?;
            let orientation = row
                .orientation
                .clone()
                .ok_or_else(|| corrupt("split has no orientation"))?;
            let ratio = row.ratio.ok_or_else(|| corrupt("split has no ratio"))?;
            let mut children: Vec<&LayoutRow> = by_parent
                .get(&(row.group_id.clone(), Some(row.id)))
                .map(|rows| rows.iter().collect())
                .unwrap_or_default();
            children.sort_by_key(|child| child.ordinal);
            let children = children
                .iter()
                .map(|child| build_layout(child, by_parent, visited, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(DeskLayoutNode::Split {
                orientation,
                ratio,
                children,
                fields,
            })
        }
        other => Err(corrupt(format!("unknown layout kind {other}"))),
    }
}

fn load_teams(
    conn: &Connection,
    revisions: &mut HashMap<String, Revision>,
) -> Result<Vec<DeskTeam>, StorageError> {
    let mut statement = conn
        .prepare("SELECT id, ordinal, name, lifecycle, created_at, workspace_id, revision FROM desk_teams ORDER BY ordinal")
        .map_err(|error| sqlite_error("prepare desk teams", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|error| sqlite_error("query desk teams", error))?;
    rows.map(|row| {
        let (id, _ordinal, name, lifecycle, created_at, workspace_id, revision) =
            row.map_err(|error| sqlite_error("read desk team", error))?;
        let lifecycle = match lifecycle.as_str() {
            "forming" => DeskTeamLifecycle::Forming,
            "active" => DeskTeamLifecycle::Active,
            "closing" => DeskTeamLifecycle::Closing,
            other => return Err(corrupt(format!("unknown Team lifecycle {other}"))),
        };
        revisions.insert(
            id.clone(),
            Revision(from_sql_u64("desk Team revision", revision)?),
        );
        Ok(DeskTeam {
            id,
            name,
            lifecycle,
            created_at: from_sql_u64("Team timestamp", created_at)?,
            workspace_id,
        })
    })
    .collect()
}

fn load_identities(conn: &Connection) -> Result<Vec<DeskAgentSessionIdentity>, StorageError> {
    let mut statement = conn
        .prepare("SELECT provider, session_id, record_provider, role, role_profile, parent_tab_id, can_spawn FROM agent_session_identities ORDER BY provider, session_id")
        .map_err(|error| sqlite_error("prepare agent session identities", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok(DeskAgentSessionIdentity {
                provider: row.get(0)?,
                session_id: row.get(1)?,
                record_provider: row.get(2)?,
                role: row.get(3)?,
                role_profile: row.get(4)?,
                parent_tab_id: row.get(5)?,
                can_spawn: row.get(6)?,
            })
        })
        .map_err(|error| sqlite_error("query agent session identities", error))?;
    rows.map(|row| row.map_err(|error| sqlite_error("read agent session identity", error)))
        .collect()
}

fn corrupt(detail: impl Into<String>) -> StorageError {
    StorageError::Corrupt {
        detail: detail.into(),
    }
}
