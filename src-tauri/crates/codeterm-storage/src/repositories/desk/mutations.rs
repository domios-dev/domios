use super::records::{DeskLayoutNode, DeskSnapshot, DeskTeam, DeskTeamLifecycle, DeskWorkspace};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{expect_matches, from_sql_u64, to_sql_u64, Expect, Revision};
use crate::schema::{self, WORKSPACE_ORDER_REVISION};
use crate::WorkspaceRecord;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};

#[path = "delta.rs"]
mod delta;

#[derive(Clone, Copy)]
enum OrderedTable {
    Teams,
    Workspaces,
    Tabs,
}

impl OrderedTable {
    fn max_ordinal_sql(self) -> &'static str {
        match self {
            Self::Teams => "SELECT COALESCE(MAX(ordinal), -1) FROM desk_teams",
            Self::Workspaces => "SELECT COALESCE(MAX(ordinal), -1) FROM workspaces",
            Self::Tabs => "SELECT COALESCE(MAX(ordinal), -1) FROM desk_tabs",
        }
    }

    fn update_ordinal_sql(self) -> &'static str {
        match self {
            Self::Teams => "UPDATE desk_teams SET ordinal = ?2 WHERE id = ?1",
            Self::Workspaces => "UPDATE workspaces SET ordinal = ?2 WHERE id = ?1",
            Self::Tabs => "UPDATE desk_tabs SET ordinal = ?2 WHERE id = ?1",
        }
    }

    fn ordered_rows_sql(self) -> &'static str {
        match self {
            Self::Teams => "SELECT id, ordinal FROM desk_teams",
            Self::Workspaces => "SELECT id, ordinal FROM workspaces",
            Self::Tabs => "SELECT id, ordinal FROM desk_tabs WHERE workspace_id = ?1",
        }
    }
}

fn park_ordered_rows(
    cx: &mut WriteCx<'_>,
    table: OrderedTable,
    ids: &[String],
) -> Result<(), StorageError> {
    if ids.is_empty() {
        return Ok(());
    }
    let maximum: i64 = cx
        .conn
        .query_row(table.max_ordinal_sql(), [], |row| row.get(0))
        .map_err(|error| sqlite_error("read ordered row range", error))?;
    let first = maximum.checked_add(1).ok_or_else(|| {
        StorageError::invalid("desk ordinal", "non-negative parking range is exhausted")
    })?;
    for (offset, id) in ids.iter().enumerate() {
        let offset = i64::try_from(offset).map_err(|_| {
            StorageError::invalid("desk ordinal", "too many rows to park in ordinal range")
        })?;
        let ordinal = first.checked_add(offset).ok_or_else(|| {
            StorageError::invalid("desk ordinal", "non-negative parking range is exhausted")
        })?;
        let changed = cx.write(table.update_ordinal_sql(), params![id, ordinal])?;
        if changed != 1 {
            return Err(StorageError::Corrupt {
                detail: format!("ordered row {id} disappeared while being parked"),
            });
        }
    }
    Ok(())
}

fn renumber_ordered_rows_if_needed(
    cx: &mut WriteCx<'_>,
    table: OrderedTable,
    workspace_id: Option<&str>,
    ids: &[String],
) -> Result<bool, StorageError> {
    if ids.is_empty() {
        return Ok(false);
    }
    let current_ordinals = {
        let mut statement = cx
            .conn
            .prepare(table.ordered_rows_sql())
            .map_err(|error| sqlite_error("prepare desk order", error))?;
        let rows = match workspace_id {
            Some(workspace_id) => statement.query_map([workspace_id], read_ordered_row),
            None => statement.query_map([], read_ordered_row),
        }
        .map_err(|error| sqlite_error("query desk order", error))?;
        rows.collect::<Result<HashMap<_, _>, _>>()
            .map_err(|error| sqlite_error("read desk order", error))?
    };
    let normalized = ids.iter().enumerate().all(|(ordinal, id)| {
        i64::try_from(ordinal)
            .ok()
            .is_some_and(|ordinal| current_ordinals.get(id) == Some(&ordinal))
    });
    if normalized {
        return Ok(false);
    }
    park_ordered_rows(cx, table, ids)?;
    for (ordinal, id) in ids.iter().enumerate() {
        let ordinal = i64::try_from(ordinal)
            .map_err(|_| StorageError::invalid("desk ordinal", "too many ordered rows"))?;
        let changed = cx.write(table.update_ordinal_sql(), params![id, ordinal])?;
        if changed != 1 {
            return Err(StorageError::Corrupt {
                detail: format!("ordered row {id} disappeared while being renumbered"),
            });
        }
    }
    Ok(true)
}

fn read_ordered_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, i64)> {
    Ok((row.get(0)?, row.get(1)?))
}

pub(super) fn import(cx: &mut WriteCx<'_>, snapshot: DeskSnapshot) -> Result<(), StorageError> {
    import_empty(cx, snapshot)
}

fn import_empty(cx: &mut WriteCx<'_>, snapshot: DeskSnapshot) -> Result<(), StorageError> {
    let existing: Option<i64> = cx
        .conn
        .query_row(
            "SELECT revision FROM desk_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error("read desk import marker", error))?;
    let current = existing
        .map(|revision| from_sql_u64("desk revision", revision).map(Revision))
        .transpose()?;
    expect_matches("desk import", Expect::Absent, current)?;
    let workspace_count: i64 = cx
        .conn
        .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
        .map_err(|error| sqlite_error("count workspace rows before desk import", error))?;
    if workspace_count != 0 {
        return Err(StorageError::invalid(
            "desk import",
            "workspace repository already contains rows",
        ));
    }
    let revision = sql_revision(cx)?;
    cx.write(
        "INSERT INTO desk_meta (singleton, version, timestamp, active_workspace_id, revision, workspace_order_revision) \
         VALUES (1, ?1, ?2, ?3, ?4, ?4)",
        params![
            snapshot.version,
            to_sql_u64("desk timestamp", snapshot.timestamp)?,
            snapshot.active_workspace_id,
            revision
        ],
    )?;
    for workspace in &snapshot.workspaces {
        insert_workspace(cx, workspace)?;
    }
    for (ordinal, team) in snapshot.teams.iter().enumerate() {
        insert_team(cx, team, ordinal as i64)?;
    }
    for identity in &snapshot.agent_session_identities {
        insert_identity(cx, identity)?;
    }
    schema::write_meta(cx.conn, WORKSPACE_ORDER_REVISION, cx.revision.0)?;
    Ok(())
}

pub(super) fn additive_restore(
    cx: &mut WriteCx<'_>,
    snapshot: DeskSnapshot,
) -> Result<(), StorageError> {
    let initialized: bool = cx
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM desk_meta WHERE singleton = 1)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error("read desk restore marker", error))?;
    if !initialized {
        return import_empty(cx, snapshot);
    }
    let mut changed = false;
    let mut workspace_added = false;
    for workspace in &snapshot.workspaces {
        let exists = workspace_exists(cx.conn, workspace.record.id.as_str())?;
        if !exists {
            insert_workspace(cx, workspace)?;
            changed = true;
            workspace_added = true;
        }
        for tab in &workspace.tabs {
            if !tab_exists(cx.conn, &tab.id)? {
                let ordinal = tab_ordinal(cx.conn, workspace.record.id.as_str())?;
                insert_tab(cx, workspace.record.id.as_str(), tab, ordinal)?;
                changed = true;
            }
        }
    }
    for team in &snapshot.teams {
        if !team_exists(cx.conn, &team.id)? {
            insert_team(cx, team, next_team_ordinal(cx.conn)?)?;
            changed = true;
        }
    }
    for identity in &snapshot.agent_session_identities {
        if !identity_exists(cx.conn, &identity.provider, &identity.session_id)? {
            insert_identity(cx, identity)?;
            changed = true;
        }
    }
    if changed {
        bump_desk(cx)?;
        if workspace_added {
            schema::write_meta(cx.conn, WORKSPACE_ORDER_REVISION, cx.revision.0)?;
            let revision = sql_revision(cx)?;
            cx.write(
                "UPDATE desk_meta SET workspace_order_revision = ?1 WHERE singleton = 1",
                [revision],
            )?;
        }
    }
    Ok(())
}

pub(super) fn rename_workspace(
    cx: &mut WriteCx<'_>,
    id: &str,
    expect: Revision,
    name: String,
) -> Result<(), StorageError> {
    if id.is_empty() || name.len() > 1024 {
        return Err(StorageError::invalid(
            "workspace rename",
            "invalid id or name",
        ));
    }
    let actual = workspace_revision(cx.conn, id)?;
    expect_matches(&format!("workspace {id}"), Expect::At(expect), actual)?;
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE workspaces SET name = ?2, revision = ?3 WHERE id = ?1",
        params![id, name, revision],
    )?;
    cx.write(
        "UPDATE desk_workspace_state SET revision = ?2 WHERE workspace_id = ?1",
        params![id, revision],
    )?;
    bump_desk(cx)
}

pub(super) fn update_workspace(
    cx: &mut WriteCx<'_>,
    record: WorkspaceRecord,
    expect: Revision,
) -> Result<(), StorageError> {
    record.validate()?;
    let id = record.id.as_str();
    let actual = workspace_revision(cx.conn, id)?;
    expect_matches(&format!("workspace {id}"), Expect::At(expect), actual)?;
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE workspaces SET name = ?2, cwd = ?3, provisioned_cwd = ?4, protected = ?5, \
         task_key_prefix = ?6, task_key_next = ?7, revision = ?8 WHERE id = ?1",
        params![
            id,
            record.name,
            record.cwd,
            record.provisioned_cwd,
            record.protected,
            record.task_key_prefix,
            to_sql_u64("workspace task key counter", record.task_key_next)?,
            revision
        ],
    )?;
    cx.write(
        "UPDATE desk_workspace_state SET revision = ?2 WHERE workspace_id = ?1",
        params![id, revision],
    )?;
    bump_desk(cx)
}

pub(super) fn rename_tab(
    cx: &mut WriteCx<'_>,
    id: &str,
    expect: Revision,
    title: String,
) -> Result<(), StorageError> {
    if id.is_empty() || title.len() > 4096 {
        return Err(StorageError::invalid("tab rename", "invalid id or title"));
    }
    let actual = tab_revision(cx.conn, id)?;
    expect_matches(&format!("tab {id}"), Expect::At(expect), actual)?;
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE desk_tabs SET title = ?2, revision = ?3 WHERE id = ?1",
        params![id, title, revision],
    )?;
    bump_desk(cx)
}

pub(super) fn rename_team(
    cx: &mut WriteCx<'_>,
    id: &str,
    expect: Revision,
    name: String,
) -> Result<(), StorageError> {
    if id.is_empty() || name.len() > 1024 {
        return Err(StorageError::invalid("Team rename", "invalid id or name"));
    }
    let actual = team_revision(cx.conn, id)?;
    expect_matches(&format!("Team {id}"), Expect::At(expect), actual)?;
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE desk_teams SET name = ?2, revision = ?3 WHERE id = ?1",
        params![id, name, revision],
    )?;
    bump_desk(cx)
}

pub(super) fn set_team_lifecycle(
    cx: &mut WriteCx<'_>,
    id: &str,
    expect: Revision,
    lifecycle: super::records::DeskTeamLifecycle,
) -> Result<(), StorageError> {
    let actual = team_revision(cx.conn, id)?;
    expect_matches(&format!("Team {id}"), Expect::At(expect), actual)?;
    let lifecycle = match lifecycle {
        super::records::DeskTeamLifecycle::Forming => "forming",
        super::records::DeskTeamLifecycle::Active => "active",
        super::records::DeskTeamLifecycle::Closing => "closing",
    };
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE desk_teams SET lifecycle = ?2, revision = ?3 WHERE id = ?1",
        params![id, lifecycle, revision],
    )?;
    bump_desk(cx)
}

pub(super) fn reorder_tabs(
    cx: &mut WriteCx<'_>,
    workspace_id: &str,
    expect_order: Revision,
    order: Vec<String>,
) -> Result<(), StorageError> {
    let actual = workspace_tab_order_revision(cx.conn, workspace_id)?;
    expect_matches(
        &format!("tab order in workspace {workspace_id}"),
        Expect::At(expect_order),
        actual,
    )?;
    let current = tab_order(cx.conn, workspace_id)?;
    exact_permutation("tab order", &current, &order)?;
    let order_ids = order.clone();
    let renumbered =
        renumber_ordered_rows_if_needed(cx, OrderedTable::Tabs, Some(workspace_id), &order_ids)?;
    if current == order {
        if renumbered {
            bump_desk(cx)?;
        }
        return Ok(());
    }
    let revision = sql_revision(cx)?;
    for (ordinal, id) in order.iter().enumerate() {
        if current.iter().position(|current_id| current_id == id) != Some(ordinal) {
            cx.write(
                "UPDATE desk_tabs SET ordinal = ?2, sort_order = ?2 WHERE id = ?1",
                params![id, ordinal as i64],
            )?;
        }
    }
    cx.write(
        "UPDATE desk_workspace_state SET revision = ?2, tab_order_revision = ?2 WHERE workspace_id = ?1",
        params![workspace_id, revision],
    )?;
    bump_desk(cx)
}

pub(super) fn reorder_workspaces(
    cx: &mut WriteCx<'_>,
    expect_order: Revision,
    order: Vec<String>,
) -> Result<(), StorageError> {
    let actual = desk_workspace_order_revision(cx.conn)?;
    expect_matches("desk workspace order", Expect::At(expect_order), actual)?;
    let current = workspace_order(cx.conn)?;
    exact_permutation("workspace order", &current, &order)?;
    let order_ids = order.clone();
    let renumbered =
        renumber_ordered_rows_if_needed(cx, OrderedTable::Workspaces, None, &order_ids)?;
    let moved: Vec<(String, i64)> = order
        .iter()
        .zip(0_i64..)
        .filter(|(id, target)| {
            current.iter().position(|current| current == *id) != Some(*target as usize)
        })
        .map(|(id, target)| (id.clone(), target))
        .collect();
    if moved.is_empty() {
        if renumbered {
            bump_desk(cx)?;
        }
        return Ok(());
    }
    let revision = sql_revision(cx)?;
    for (id, target) in &moved {
        cx.write(
            "UPDATE workspaces SET ordinal = ?2, revision = ?3 WHERE id = ?1",
            params![id, target, revision],
        )?;
        cx.write(
            "UPDATE desk_workspace_state SET sort_order = ?2, revision = ?3 WHERE workspace_id = ?1",
            params![id, target, revision],
        )?;
    }
    schema::write_meta(cx.conn, WORKSPACE_ORDER_REVISION, cx.revision.0)?;
    cx.write(
        "UPDATE desk_meta SET workspace_order_revision = ?1 WHERE singleton = 1",
        [revision],
    )?;
    bump_desk(cx)
}

pub(super) fn close_tab(
    cx: &mut WriteCx<'_>,
    id: &str,
    expect: Revision,
) -> Result<(), StorageError> {
    let actual = tab_revision(cx.conn, id)?;
    expect_matches(&format!("tab {id}"), Expect::At(expect), actual)?;
    let (workspace_id, ordinal, active_tab_id): (String, i64, Option<String>) = cx
        .conn
        .query_row(
            "SELECT t.workspace_id, t.ordinal, s.active_tab_id \
             FROM desk_tabs t JOIN desk_workspace_state s ON s.workspace_id = t.workspace_id \
             WHERE t.id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| sqlite_error("read tab workspace", error))?;
    let active_tab_after = if active_tab_id.as_deref() == Some(id) {
        adjacent_tab(cx.conn, &workspace_id, id, ordinal)?
    } else {
        active_tab_id
    };
    let revision = sql_revision(cx)?;
    cx.write("DELETE FROM desk_tabs WHERE id = ?1", [id])?;
    cx.write(
        "UPDATE desk_workspace_state SET active_tab_id = ?2, revision = ?3, \
         tab_order_revision = ?3 WHERE workspace_id = ?1",
        params![workspace_id, active_tab_after, revision],
    )?;
    bump_desk(cx)
}

pub(super) fn select_workspace(
    cx: &mut WriteCx<'_>,
    id: Option<String>,
    expect: Revision,
) -> Result<(), StorageError> {
    let actual = desk_revision(cx.conn)?;
    expect_matches("desk selection", Expect::At(expect), actual)?;
    if let Some(id) = id.as_deref() {
        if !workspace_exists(cx.conn, id)? {
            return Err(StorageError::NotFound {
                entity: format!("workspace {id}"),
            });
        }
    }
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE desk_meta SET active_workspace_id = ?1, revision = ?2 WHERE singleton = 1",
        params![id, revision],
    )?;
    Ok(())
}

pub(super) fn select_tab(
    cx: &mut WriteCx<'_>,
    workspace_id: &str,
    expect_workspace: Revision,
    tab_id: Option<String>,
) -> Result<(), StorageError> {
    let actual = workspace_revision(cx.conn, workspace_id)?;
    expect_matches(
        &format!("workspace {workspace_id}"),
        Expect::At(expect_workspace),
        actual,
    )?;
    if let Some(id) = tab_id.as_deref() {
        let belongs: bool = cx
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM desk_tabs WHERE id = ?1 AND workspace_id = ?2)",
                params![id, workspace_id],
                |row| row.get(0),
            )
            .map_err(|error| sqlite_error("validate selected tab", error))?;
        if !belongs {
            return Err(StorageError::NotFound {
                entity: format!("tab {id} in workspace {workspace_id}"),
            });
        }
    }
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE desk_workspace_state SET active_tab_id = ?2, revision = ?3 WHERE workspace_id = ?1",
        params![workspace_id, tab_id, revision],
    )?;
    bump_desk(cx)
}

pub(crate) fn remove_team(
    cx: &mut WriteCx<'_>,
    id: &str,
    expect: Revision,
) -> Result<(), StorageError> {
    let actual = team_revision(cx.conn, id)?;
    expect_matches(&format!("Team {id}"), Expect::At(expect), actual)?;
    cx.write("DELETE FROM desk_teams WHERE id = ?1", [id])?;
    bump_desk(cx)
}

pub(super) fn close_workspace(
    cx: &mut WriteCx<'_>,
    id: &str,
    expect: Revision,
) -> Result<(), StorageError> {
    let actual = workspace_revision(cx.conn, id)?;
    expect_matches(&format!("workspace {id}"), Expect::At(expect), actual)?;
    let active_workspace_id: Option<String> = cx
        .conn
        .query_row(
            "SELECT active_workspace_id FROM desk_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error("read active workspace before close", error))?;
    let active_workspace_after = if active_workspace_id.as_deref() == Some(id) {
        cx.conn
            .query_row(
                "SELECT id FROM workspaces WHERE id <> ?1 ORDER BY ordinal LIMIT 1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| sqlite_error("select active workspace after close", error))?
    } else {
        active_workspace_id
    };
    let revision = sql_revision(cx)?;
    cx.write("DELETE FROM workspaces WHERE id = ?1", [id])?;
    cx.write(
        "UPDATE desk_meta SET active_workspace_id = ?1, workspace_order_revision = ?2 \
         WHERE singleton = 1",
        params![active_workspace_after, revision],
    )?;
    schema::write_meta(cx.conn, WORKSPACE_ORDER_REVISION, cx.revision.0)?;
    bump_desk(cx)
}

fn insert_workspace(cx: &mut WriteCx<'_>, workspace: &DeskWorkspace) -> Result<(), StorageError> {
    let record = &workspace.record;
    let revision = sql_revision(cx)?;
    let ordinal: i64 = cx
        .conn
        .query_row(
            "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM workspaces",
            [],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error("allocate workspace ordinal", error))?;
    cx.write(
        "INSERT INTO workspaces (id, ordinal, name, cwd, provisioned_cwd, protected, task_key_prefix, task_key_next, revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            record.id.as_str(),
            ordinal,
            record.name,
            record.cwd,
            record.provisioned_cwd,
            record.protected,
            record.task_key_prefix,
            to_sql_u64("workspace task key counter", record.task_key_next)?,
            revision
        ],
    )?;
    cx.write(
        "INSERT INTO desk_workspace_state (workspace_id, sort_order, active_tab_id, revision, tab_order_revision) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![record.id.as_str(), workspace.sort_order, workspace.active_tab_id, revision],
    )?;
    for (tab_ordinal, tab) in workspace.tabs.iter().enumerate() {
        insert_tab(cx, record.id.as_str(), tab, tab_ordinal as i64)?;
    }
    schema::write_meta(cx.conn, WORKSPACE_ORDER_REVISION, cx.revision.0)?;
    Ok(())
}

fn insert_tab(
    cx: &mut WriteCx<'_>,
    workspace_id: &str,
    tab: &super::records::DeskTab,
    ordinal: i64,
) -> Result<(), StorageError> {
    let revision = sql_revision(cx)?;
    cx.write(
        "INSERT INTO desk_tabs (id, workspace_id, ordinal, title, description, sort_order, revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![tab.id, workspace_id, ordinal, tab.title, tab.description, tab.sort_order, revision],
    )?;
    insert_layout(cx, &tab.id, None, 0, &tab.layout)
}

fn insert_layout(
    cx: &mut WriteCx<'_>,
    group_id: &str,
    parent_id: Option<i64>,
    ordinal: i64,
    node: &DeskLayoutNode,
) -> Result<(), StorageError> {
    let id = match node {
        DeskLayoutNode::Leaf {
            tab_id,
            cwd,
            fields,
        } => {
            fields.validate()?;
            let fields_json = serde_json::to_string(&fields.values)
                .map_err(|error| StorageError::invalid("desk leaf fields", error.to_string()))?;
            cx.write(
                "INSERT INTO desk_layout_nodes (group_id, parent_id, ordinal, kind, tab_id, cwd, fields_json) \
                 VALUES (?1, ?2, ?3, 'leaf', ?4, ?5, ?6)",
                params![group_id, parent_id, ordinal, tab_id, cwd, fields_json],
            )?;
            cx.conn.last_insert_rowid()
        }
        DeskLayoutNode::Split {
            orientation,
            ratio,
            children,
            fields,
        } => {
            let fields_json = serde_json::to_string(fields)
                .map_err(|error| StorageError::invalid("desk split fields", error.to_string()))?;
            cx.write(
                "INSERT INTO desk_layout_nodes (group_id, parent_id, ordinal, kind, orientation, ratio, fields_json) \
                 VALUES (?1, ?2, ?3, 'split', ?4, ?5, ?6)",
                params![group_id, parent_id, ordinal, orientation, ratio, fields_json],
            )?;
            let id = cx.conn.last_insert_rowid();
            for (child_ordinal, child) in children.iter().enumerate() {
                insert_layout(cx, group_id, Some(id), child_ordinal as i64, child)?;
            }
            id
        }
    };
    let _ = id;
    Ok(())
}

fn insert_team(cx: &mut WriteCx<'_>, team: &DeskTeam, ordinal: i64) -> Result<(), StorageError> {
    let lifecycle = match team.lifecycle {
        DeskTeamLifecycle::Forming => "forming",
        DeskTeamLifecycle::Active => "active",
        DeskTeamLifecycle::Closing => "closing",
    };
    let created_at = to_sql_u64("Team timestamp", team.created_at)?;
    let revision = sql_revision(cx)?;
    cx.write(
        "INSERT INTO desk_teams (id, ordinal, name, lifecycle, created_at, workspace_id, revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![team.id, ordinal, team.name, lifecycle, created_at, team.workspace_id, revision],
    )?;
    Ok(())
}

fn insert_identity(
    cx: &mut WriteCx<'_>,
    identity: &super::records::DeskAgentSessionIdentity,
) -> Result<(), StorageError> {
    let revision = sql_revision(cx)?;
    cx.write(
        "INSERT INTO agent_session_identities (provider, session_id, record_provider, role, role_profile, parent_tab_id, can_spawn, revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![identity.provider, identity.session_id, identity.record_provider, identity.role, identity.role_profile, identity.parent_tab_id, identity.can_spawn, revision],
    )?;
    Ok(())
}

fn workspace_exists(conn: &Connection, id: &str) -> Result<bool, StorageError> {
    exists(
        conn,
        "SELECT EXISTS(SELECT 1 FROM desk_workspace_state WHERE workspace_id = ?1)",
        id,
    )
}

fn tab_exists(conn: &Connection, id: &str) -> Result<bool, StorageError> {
    exists(
        conn,
        "SELECT EXISTS(SELECT 1 FROM desk_tabs WHERE id = ?1)",
        id,
    )
}

fn team_exists(conn: &Connection, id: &str) -> Result<bool, StorageError> {
    exists(
        conn,
        "SELECT EXISTS(SELECT 1 FROM desk_teams WHERE id = ?1)",
        id,
    )
}

fn exists(conn: &Connection, sql: &str, id: &str) -> Result<bool, StorageError> {
    conn.query_row(sql, [id], |row| row.get(0))
        .map_err(|error| sqlite_error("check desk identity", error))
}

fn identity_exists(conn: &Connection, provider: &str, session: &str) -> Result<bool, StorageError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_session_identities WHERE provider = ?1 AND session_id = ?2)",
        params![provider, session],
        |row| row.get(0),
    )
    .map_err(|error| sqlite_error("check agent session identity", error))
}

fn team_revision(conn: &Connection, id: &str) -> Result<Option<Revision>, StorageError> {
    row_revision(
        conn,
        "SELECT revision FROM desk_teams WHERE id = ?1",
        id,
        "Team revision",
    )
}

fn workspace_revision(conn: &Connection, id: &str) -> Result<Option<Revision>, StorageError> {
    row_revision(
        conn,
        "SELECT revision FROM desk_workspace_state WHERE workspace_id = ?1",
        id,
        "workspace revision",
    )
}

fn tab_revision(conn: &Connection, id: &str) -> Result<Option<Revision>, StorageError> {
    row_revision(
        conn,
        "SELECT revision FROM desk_tabs WHERE id = ?1",
        id,
        "tab revision",
    )
}

fn row_revision(
    conn: &Connection,
    sql: &str,
    id: &str,
    field: &'static str,
) -> Result<Option<Revision>, StorageError> {
    let revision: Option<i64> = conn
        .query_row(sql, [id], |row| row.get(0))
        .optional()
        .map_err(|error| sqlite_error("read desk entity revision", error))?;
    revision
        .map(|value| from_sql_u64(field, value).map(Revision))
        .transpose()
}

fn workspace_tab_order_revision(
    conn: &Connection,
    id: &str,
) -> Result<Option<Revision>, StorageError> {
    row_revision(
        conn,
        "SELECT tab_order_revision FROM desk_workspace_state WHERE workspace_id = ?1",
        id,
        "tab order revision",
    )
}

fn desk_workspace_order_revision(conn: &Connection) -> Result<Option<Revision>, StorageError> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT workspace_order_revision FROM desk_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error("read desk order revision", error))?;
    value
        .map(|revision| from_sql_u64("desk order revision", revision).map(Revision))
        .transpose()
}

fn desk_revision(conn: &Connection) -> Result<Option<Revision>, StorageError> {
    row_revision(
        conn,
        "SELECT revision FROM desk_meta WHERE singleton = ?1",
        "1",
        "desk revision",
    )
}

fn tab_order(conn: &Connection, workspace_id: &str) -> Result<Vec<String>, StorageError> {
    ordered_ids(
        conn,
        "SELECT id FROM desk_tabs WHERE workspace_id = ?1 ORDER BY ordinal",
        workspace_id,
    )
}

fn adjacent_tab(
    conn: &Connection,
    workspace_id: &str,
    closed_tab_id: &str,
    closed_ordinal: i64,
) -> Result<Option<String>, StorageError> {
    conn.query_row(
        "SELECT id FROM desk_tabs WHERE workspace_id = ?1 AND id <> ?2 \
         ORDER BY CASE WHEN ordinal >= ?3 THEN 0 ELSE 1 END, \
         CASE WHEN ordinal >= ?3 THEN ordinal END ASC, \
         CASE WHEN ordinal < ?3 THEN ordinal END DESC LIMIT 1",
        params![workspace_id, closed_tab_id, closed_ordinal],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| sqlite_error("select active tab after close", error))
}

fn workspace_order(conn: &Connection) -> Result<Vec<String>, StorageError> {
    let mut statement = conn
        .prepare("SELECT w.id FROM workspaces w JOIN desk_workspace_state d ON d.workspace_id = w.id ORDER BY w.ordinal")
        .map_err(|error| sqlite_error("prepare workspace order", error))?;
    let ids = statement
        .query_map([], |row| row.get(0))
        .map_err(|error| sqlite_error("query workspace order", error))?
        .map(|row| row.map_err(|error| sqlite_error("read workspace order", error)))
        .collect();
    ids
}

fn ordered_ids(conn: &Connection, sql: &str, scope: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare desk order", error))?;
    let ids = statement
        .query_map([scope], |row| row.get(0))
        .map_err(|error| sqlite_error("query desk order", error))?
        .map(|row| row.map_err(|error| sqlite_error("read desk order", error)))
        .collect();
    ids
}

fn exact_permutation(
    field: &'static str,
    current: &[String],
    order: &[String],
) -> Result<(), StorageError> {
    let current_ids: HashSet<&str> = current.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    let exact = order.len() == current.len()
        && order
            .iter()
            .all(|id| current_ids.contains(id.as_str()) && seen.insert(id.as_str()));
    if exact {
        Ok(())
    } else {
        Err(StorageError::invalid(
            field,
            "must be an exact permutation of current identities",
        ))
    }
}

fn tab_ordinal(conn: &Connection, workspace_id: &str) -> Result<i64, StorageError> {
    conn.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM desk_tabs WHERE workspace_id = ?1",
        [workspace_id],
        |row| row.get(0),
    )
    .map_err(|error| sqlite_error("allocate tab ordinal", error))
}

fn next_team_ordinal(conn: &Connection) -> Result<i64, StorageError> {
    conn.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM desk_teams",
        [],
        |row| row.get(0),
    )
    .map_err(|error| sqlite_error("allocate Team ordinal", error))
}

fn bump_desk(cx: &mut WriteCx<'_>) -> Result<(), StorageError> {
    let revision = sql_revision(cx)?;
    cx.write(
        "UPDATE desk_meta SET revision = ?1 WHERE singleton = 1",
        [revision],
    )?;
    Ok(())
}

fn sql_revision(cx: &WriteCx<'_>) -> Result<i64, StorageError> {
    to_sql_u64("desk revision", cx.revision.0)
}
