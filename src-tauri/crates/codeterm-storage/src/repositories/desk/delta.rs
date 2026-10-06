use super::super::records::{
    DeskAgentSessionIdentity, DeskSnapshot, DeskTab, DeskTeam, DeskWorkspace,
};
use super::super::{queries, retain_orphan_teams, DeskRepository};
use super::{
    insert_identity, insert_layout, insert_tab, insert_team, insert_workspace,
    renumber_ordered_rows_if_needed, OrderedTable,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::{expect_matches, to_sql_u64, Committed, Completion, Expect, Revision};
use crate::schema::{self, WORKSPACE_ORDER_REVISION};
use rusqlite::params;
use std::collections::{HashMap, HashSet};

impl DeskRepository {
    pub fn save_preserving(
        &self,
        snapshot: DeskSnapshot,
        expected: Revision,
    ) -> Completion<Committed<()>> {
        self.submit_delta(snapshot, expected, false)
    }

    pub fn save_authorized_reduction(
        &self,
        snapshot: DeskSnapshot,
        expected: Revision,
    ) -> Completion<Committed<()>> {
        self.submit_delta(snapshot, expected, true)
    }

    fn submit_delta(
        &self,
        mut snapshot: DeskSnapshot,
        expected: Revision,
        reduce: bool,
    ) -> Completion<Committed<()>> {
        retain_orphan_teams(&mut snapshot);
        let cost = match snapshot.validate() {
            Ok(cost) => cost,
            Err(error) => return Completion::settled(Err(error)),
        };
        self.storage
            .submit_mutation(cost, move |cx| apply_delta(cx, snapshot, expected, reduce))
    }
}

fn apply_delta(
    cx: &mut WriteCx<'_>,
    submitted: DeskSnapshot,
    expected: Revision,
    reduce: bool,
) -> Result<(), StorageError> {
    let current = queries::load(cx.conn)?;
    expect_matches(
        "desk snapshot",
        Expect::At(expected),
        Some(current.revision),
    )?;
    let mut target = if reduce {
        submitted
    } else {
        preserve_missing(&current.snapshot, submitted)
    };
    retain_orphan_teams(&mut target);
    target.validate()?;
    validate_references(&target)?;

    let initial_rows = cx.rows_written;
    let workspace_record_revisions = queries::load_workspace_record_revisions(cx.conn)?;
    let identity_revisions = queries::load_identity_revisions(cx.conn)?;
    insert_workspace_stubs(cx, &current.snapshot, &target)?;
    apply_tabs(cx, &current.snapshot, &current.tab_revisions, &target)?;
    delete_missing(
        cx,
        &current,
        &workspace_record_revisions,
        &identity_revisions,
        &target,
        reduce,
    )?;
    apply_teams(cx, &current.snapshot, &current.team_revisions, &target)?;
    apply_identities(cx, &current.snapshot, &identity_revisions, &target)?;
    let workspace_order_changed =
        apply_workspaces(cx, &current, &workspace_record_revisions, &target)?;

    let metadata_changed = current.snapshot.version != target.version
        || current.snapshot.timestamp != target.timestamp
        || current.snapshot.active_workspace_id != target.active_workspace_id;
    if cx.rows_written != initial_rows || metadata_changed {
        let order_revision = if workspace_order_changed {
            to_sql_u64("desk workspace order revision", cx.revision.0)?
        } else {
            to_sql_u64(
                "desk workspace order revision",
                current.workspace_order_revision.0,
            )?
        };
        let changed = cx.write(
            "UPDATE desk_meta SET version = ?1, timestamp = ?2, active_workspace_id = ?3, \
             revision = ?4, workspace_order_revision = ?5 WHERE singleton = 1 AND revision = ?6",
            params![
                target.version,
                to_sql_u64("desk timestamp", target.timestamp)?,
                target.active_workspace_id,
                to_sql_u64("desk revision", cx.revision.0)?,
                order_revision,
                to_sql_u64("desk revision", expected.0)?
            ],
        )?;
        require_one(changed, "desk snapshot", expected)?;
    }
    if workspace_order_changed {
        schema::write_meta(cx.conn, WORKSPACE_ORDER_REVISION, cx.revision.0)?;
    }
    Ok(())
}

fn preserve_missing(current: &DeskSnapshot, submitted: DeskSnapshot) -> DeskSnapshot {
    let incoming_tab_owners: HashMap<String, String> = submitted
        .workspaces
        .iter()
        .flat_map(|workspace| {
            workspace
                .tabs
                .iter()
                .map(move |tab| (tab.id.clone(), workspace.record.id.as_str().to_owned()))
        })
        .collect();
    let current_workspaces: HashMap<&str, &DeskWorkspace> = current
        .workspaces
        .iter()
        .map(|workspace| (workspace.record.id.as_str(), workspace))
        .collect();
    let mut incoming_workspaces = Vec::with_capacity(submitted.workspaces.len());
    for mut workspace in submitted.workspaces {
        if let Some(previous) = current_workspaces.get(workspace.record.id.as_str()) {
            let retained_tabs: Vec<_> = previous
                .tabs
                .iter()
                .filter(|tab| {
                    incoming_tab_owners
                        .get(&tab.id)
                        .is_none_or(|owner| owner.as_str() == workspace.record.id.as_str())
                })
                .cloned()
                .collect();
            workspace.tabs = preserve_order(&retained_tabs, &workspace.tabs, |tab| &tab.id);
        }
        incoming_workspaces.push(workspace);
    }
    let submitted_workspace_ids: HashSet<String> = incoming_workspaces
        .iter()
        .map(|workspace| workspace.record.id.as_str().to_owned())
        .collect();
    let incoming_workspaces =
        preserve_order(&current.workspaces, &incoming_workspaces, |workspace| {
            workspace.record.id.as_str()
        });
    let mut workspaces = Vec::with_capacity(incoming_workspaces.len());
    for mut workspace in incoming_workspaces {
        if !submitted_workspace_ids.contains(workspace.record.id.as_str()) {
            workspace
                .tabs
                .retain(|tab| !incoming_tab_owners.contains_key(&tab.id));
        }
        workspaces.push(workspace);
    }

    let teams = preserve_order(&current.teams, &submitted.teams, |team| &team.id);
    let mut identities = submitted.agent_session_identities;
    let incoming_identities: HashSet<(String, String)> = identities
        .iter()
        .map(|identity| (identity.provider.clone(), identity.session_id.clone()))
        .collect();
    identities.extend(
        current
            .agent_session_identities
            .iter()
            .filter(|identity| {
                !incoming_identities
                    .contains(&(identity.provider.clone(), identity.session_id.clone()))
            })
            .cloned(),
    );
    DeskSnapshot {
        version: submitted.version,
        timestamp: submitted.timestamp,
        active_workspace_id: submitted.active_workspace_id,
        workspaces,
        teams,
        agent_session_identities: identities,
    }
}

fn preserve_order<T: Clone, F>(current: &[T], incoming: &[T], id: F) -> Vec<T>
where
    F: for<'a> Fn(&'a T) -> &'a str,
{
    let current_ids: HashSet<&str> = current.iter().map(|item| id(item)).collect();
    let incoming_by_id: HashMap<&str, &T> = incoming.iter().map(|item| (id(item), item)).collect();
    let slots: Vec<usize> = current
        .iter()
        .enumerate()
        .filter(|(_, item)| incoming_by_id.contains_key(id(item)))
        .map(|(index, _)| index)
        .collect();
    let replacements: Vec<&T> = incoming
        .iter()
        .filter(|item| current_ids.contains(id(item)))
        .collect();
    let mut merged = current.to_vec();
    for (slot, replacement) in slots.into_iter().zip(replacements) {
        merged[slot] = (*replacement).clone();
    }
    merged.extend(
        incoming
            .iter()
            .filter(|item| !current_ids.contains(id(item)))
            .cloned(),
    );
    merged
}

fn validate_references(snapshot: &DeskSnapshot) -> Result<(), StorageError> {
    let workspace_ids: HashSet<&str> = snapshot
        .workspaces
        .iter()
        .map(|workspace| workspace.record.id.as_str())
        .collect();
    if snapshot
        .active_workspace_id
        .as_deref()
        .is_some_and(|id| !workspace_ids.contains(id))
    {
        return Err(StorageError::invalid(
            "active workspace",
            "must name a desk workspace",
        ));
    }
    for workspace in &snapshot.workspaces {
        if workspace
            .active_tab_id
            .as_deref()
            .is_some_and(|id| !workspace.tabs.iter().any(|tab| tab.id == id))
        {
            return Err(StorageError::invalid(
                "active tab",
                "must name a tab in its workspace",
            ));
        }
    }
    Ok(())
}

fn insert_workspace_stubs(
    cx: &mut WriteCx<'_>,
    current: &DeskSnapshot,
    target: &DeskSnapshot,
) -> Result<(), StorageError> {
    let current_ids: HashSet<&str> = current
        .workspaces
        .iter()
        .map(|workspace| workspace.record.id.as_str())
        .collect();
    for workspace in &target.workspaces {
        if !current_ids.contains(workspace.record.id.as_str()) {
            let mut stub = workspace.clone();
            stub.tabs.clear();
            insert_workspace(cx, &stub)?;
        }
    }
    Ok(())
}

fn apply_tabs(
    cx: &mut WriteCx<'_>,
    current: &DeskSnapshot,
    revisions: &HashMap<String, Revision>,
    target: &DeskSnapshot,
) -> Result<(), StorageError> {
    let current_tabs = indexed_tabs(current);
    for workspace in &target.workspaces {
        for (ordinal, tab) in workspace.tabs.iter().enumerate() {
            if !current_tabs.contains_key(tab.id.as_str()) {
                insert_tab(cx, workspace.record.id.as_str(), tab, ordinal as i64)?;
            }
        }
    }
    for workspace in &target.workspaces {
        let owner = workspace.record.id.as_str();
        let target_ids: Vec<String> = workspace.tabs.iter().map(|tab| tab.id.clone()).collect();
        renumber_ordered_rows_if_needed(cx, OrderedTable::Tabs, Some(owner), &target_ids)?;
        for (ordinal, tab) in workspace.tabs.iter().enumerate() {
            let Some((old_workspace, old_ordinal, old_tab)) = current_tabs.get(tab.id.as_str())
            else {
                continue;
            };
            let changed = *old_workspace != owner
                || *old_ordinal != ordinal
                || old_tab.title != tab.title
                || old_tab.description != tab.description
                || old_tab.sort_order != tab.sort_order
                || old_tab.layout != tab.layout;
            if !changed {
                continue;
            }
            let expected = *revisions
                .get(&tab.id)
                .ok_or_else(|| corrupt(format!("tab {} has no revision", tab.id)))?;
            let revision = to_sql_u64("desk revision", cx.revision.0)?;
            let old_revision = to_sql_u64("desk tab revision", expected.0)?;
            let changed_rows = cx.write(
                "UPDATE desk_tabs SET workspace_id = ?2, ordinal = ?3, title = ?4, \
                 description = ?5, sort_order = ?6, revision = ?7 \
                 WHERE id = ?1 AND revision = ?8",
                params![
                    tab.id,
                    owner,
                    ordinal as i64,
                    tab.title,
                    tab.description,
                    tab.sort_order,
                    revision,
                    old_revision
                ],
            )?;
            require_one(changed_rows, &format!("tab {}", tab.id), expected)?;
            if old_tab.layout != tab.layout {
                cx.write(
                    "DELETE FROM desk_layout_nodes WHERE group_id = ?1",
                    [&tab.id],
                )?;
                insert_layout(cx, &tab.id, None, 0, &tab.layout)?;
            }
        }
    }
    Ok(())
}

fn delete_missing(
    cx: &mut WriteCx<'_>,
    current: &crate::repositories::desk::DeskState,
    workspace_revisions: &HashMap<String, Revision>,
    identity_revisions: &HashMap<(String, String), Revision>,
    target: &DeskSnapshot,
    reduce: bool,
) -> Result<(), StorageError> {
    let target_workspaces: HashSet<&str> = target
        .workspaces
        .iter()
        .map(|workspace| workspace.record.id.as_str())
        .collect();
    if reduce {
        let target_tabs: HashSet<&str> = target
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs.iter().map(|tab| tab.id.as_str()))
            .collect();
        for tab in current
            .snapshot
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs.iter())
            .filter(|tab| !target_tabs.contains(tab.id.as_str()))
        {
            let expected = current.tab_revisions[&tab.id];
            delete_tab(cx, &tab.id, expected)?;
        }

        for workspace in &current.snapshot.workspaces {
            let id = workspace.record.id.as_str();
            if target_workspaces.contains(id) {
                continue;
            }
            let state_revision = current.workspace_revisions[id];
            let record_revision = workspace_revisions[id];
            let deleted_state = cx.write(
                "DELETE FROM desk_workspace_state WHERE workspace_id = ?1 AND revision = ?2",
                params![id, to_sql_u64("desk workspace revision", state_revision.0)?],
            )?;
            require_one(deleted_state, &format!("workspace {id}"), state_revision)?;
            let deleted_record = cx.write(
                "DELETE FROM workspaces WHERE id = ?1 AND revision = ?2",
                params![
                    id,
                    to_sql_u64("workspace record revision", record_revision.0)?
                ],
            )?;
            require_one(deleted_record, &format!("workspace {id}"), record_revision)?;
        }
    }

    let target_teams: HashSet<&str> = target.teams.iter().map(|team| team.id.as_str()).collect();
    for team in &current.snapshot.teams {
        if target_teams.contains(team.id.as_str())
            || (!reduce && target_workspaces.contains(team.workspace_id.as_str()))
        {
            continue;
        }
        let expected = current.team_revisions[&team.id];
        let deleted = cx.write(
            "DELETE FROM desk_teams WHERE id = ?1 AND revision = ?2",
            params![team.id, to_sql_u64("desk Team revision", expected.0)?],
        )?;
        require_one(deleted, &format!("Team {}", team.id), expected)?;
    }

    if !reduce {
        return Ok(());
    }

    let target_identities: HashSet<(&str, &str)> = target
        .agent_session_identities
        .iter()
        .map(|identity| (identity.provider.as_str(), identity.session_id.as_str()))
        .collect();
    for identity in &current.snapshot.agent_session_identities {
        let key = (identity.provider.as_str(), identity.session_id.as_str());
        if target_identities.contains(&key) {
            continue;
        }
        let expected = identity_revisions
            .get(&(identity.provider.clone(), identity.session_id.clone()))
            .copied()
            .ok_or_else(|| corrupt("agent identity has no revision"))?;
        let deleted = cx.write(
            "DELETE FROM agent_session_identities WHERE provider = ?1 AND session_id = ?2 AND revision = ?3",
            params![
                identity.provider,
                identity.session_id,
                to_sql_u64("agent identity revision", expected.0)?
            ],
        )?;
        require_one(deleted, "agent session identity", expected)?;
    }
    Ok(())
}

fn delete_tab(cx: &mut WriteCx<'_>, id: &str, expected: Revision) -> Result<(), StorageError> {
    cx.write("DELETE FROM desk_layout_nodes WHERE group_id = ?1", [id])?;
    let deleted = cx.write(
        "DELETE FROM desk_tabs WHERE id = ?1 AND revision = ?2",
        params![id, to_sql_u64("desk tab revision", expected.0)?],
    )?;
    require_one(deleted, &format!("tab {id}"), expected)
}

fn apply_teams(
    cx: &mut WriteCx<'_>,
    current: &DeskSnapshot,
    revisions: &HashMap<String, Revision>,
    target: &DeskSnapshot,
) -> Result<(), StorageError> {
    let current_ids: HashSet<&str> = current.teams.iter().map(|team| team.id.as_str()).collect();
    for team in &target.teams {
        if !current_ids.contains(team.id.as_str()) {
            insert_team(cx, team, next_team_ordinal(cx.conn)?)?;
        }
    }
    let current_ordinals = team_ordinals(cx)?;
    let current_order: Vec<&str> = current_ordinals.iter().map(|(id, _)| id.as_str()).collect();
    let target_order: Vec<&str> = target.teams.iter().map(|team| team.id.as_str()).collect();
    let moved = moved_ids(&current_order, &target_order);
    let target_ids: Vec<String> = target_order.iter().map(|id| (*id).to_owned()).collect();
    renumber_ordered_rows_if_needed(cx, OrderedTable::Teams, None, &target_ids)?;
    for (ordinal, team) in target.teams.iter().enumerate() {
        let was_present = current_ids.contains(team.id.as_str());
        let record_changed = current
            .teams
            .iter()
            .find(|previous| previous.id == team.id)
            .is_some_and(|previous| previous != team);
        if !moved.contains(&team.id) && !record_changed {
            continue;
        }
        let expected = if was_present {
            revisions[&team.id]
        } else {
            Revision(cx.revision.0)
        };
        update_team(cx, team, ordinal as i64, expected, was_present)?;
    }
    Ok(())
}

fn update_team(
    cx: &mut WriteCx<'_>,
    team: &DeskTeam,
    ordinal: i64,
    expected: Revision,
    was_present: bool,
) -> Result<(), StorageError> {
    let lifecycle = team_lifecycle(team);
    let changed = cx.write(
        "UPDATE desk_teams SET ordinal = ?2, name = ?3, lifecycle = ?4, created_at = ?5, \
         workspace_id = ?6, revision = ?7 WHERE id = ?1 AND revision = ?8",
        params![
            team.id,
            ordinal,
            team.name,
            lifecycle,
            to_sql_u64("Team timestamp", team.created_at)?,
            team.workspace_id,
            to_sql_u64("desk revision", cx.revision.0)?,
            to_sql_u64("desk Team revision", expected.0)?
        ],
    )?;
    require_one(
        changed,
        &format!("Team {}", team.id),
        if was_present {
            expected
        } else {
            Revision(cx.revision.0)
        },
    )
}

fn apply_identities(
    cx: &mut WriteCx<'_>,
    current: &DeskSnapshot,
    revisions: &HashMap<(String, String), Revision>,
    target: &DeskSnapshot,
) -> Result<(), StorageError> {
    let current_identities: HashMap<(&str, &str), &DeskAgentSessionIdentity> = current
        .agent_session_identities
        .iter()
        .map(|identity| {
            (
                (identity.provider.as_str(), identity.session_id.as_str()),
                identity,
            )
        })
        .collect();
    for identity in &target.agent_session_identities {
        let key = (identity.provider.as_str(), identity.session_id.as_str());
        let Some(previous) = current_identities.get(&key) else {
            insert_identity(cx, identity)?;
            continue;
        };
        if *previous == identity {
            continue;
        }
        let expected = revisions
            .get(&(identity.provider.clone(), identity.session_id.clone()))
            .copied()
            .ok_or_else(|| corrupt("agent identity has no revision"))?;
        let changed = cx.write(
            "UPDATE agent_session_identities SET record_provider = ?3, role = ?4, \
             role_profile = ?5, parent_tab_id = ?6, can_spawn = ?7, revision = ?8 \
             WHERE provider = ?1 AND session_id = ?2 AND revision = ?9",
            params![
                identity.provider,
                identity.session_id,
                identity.record_provider,
                identity.role,
                identity.role_profile,
                identity.parent_tab_id,
                identity.can_spawn,
                to_sql_u64("desk revision", cx.revision.0)?,
                to_sql_u64("agent identity revision", expected.0)?
            ],
        )?;
        require_one(changed, "agent session identity", expected)?;
    }
    Ok(())
}

fn apply_workspaces(
    cx: &mut WriteCx<'_>,
    current: &crate::repositories::desk::DeskState,
    record_revisions: &HashMap<String, Revision>,
    target: &DeskSnapshot,
) -> Result<bool, StorageError> {
    let current_ordinals = workspace_ordinals(cx)?;
    let current_order: Vec<&str> = current_ordinals.iter().map(|(id, _)| id.as_str()).collect();
    let target_order: Vec<&str> = target
        .workspaces
        .iter()
        .map(|workspace| workspace.record.id.as_str())
        .collect();
    let order_changed = current_order != target_order;
    let moved = moved_ids(&current_order, &target_order);
    let target_ids: Vec<String> = target_order.iter().map(|id| (*id).to_owned()).collect();
    renumber_ordered_rows_if_needed(cx, OrderedTable::Workspaces, None, &target_ids)?;

    let current_workspaces: HashMap<&str, &DeskWorkspace> = current
        .snapshot
        .workspaces
        .iter()
        .map(|workspace| (workspace.record.id.as_str(), workspace))
        .collect();
    for (ordinal, workspace) in target.workspaces.iter().enumerate() {
        let id = workspace.record.id.as_str();
        let Some(previous) = current_workspaces.get(id) else {
            continue;
        };
        let record_changed = previous.record != workspace.record;
        let moved_workspace = moved.contains(workspace.record.id.as_str());
        let tab_order_changed = tab_ids(previous) != tab_ids(workspace);
        let state_changed = record_changed
            || previous.sort_order != workspace.sort_order
            || previous.active_tab_id != workspace.active_tab_id
            || moved_workspace
            || tab_order_changed;
        if record_changed || moved_workspace {
            let expected = record_revisions[id];
            let changed = cx.write(
                "UPDATE workspaces SET ordinal = ?2, name = ?3, cwd = ?4, provisioned_cwd = ?5, \
                 protected = ?6, task_key_prefix = ?7, task_key_next = ?8, revision = ?9 \
                 WHERE id = ?1 AND revision = ?10",
                params![
                    id,
                    ordinal as i64,
                    workspace.record.name,
                    workspace.record.cwd,
                    workspace.record.provisioned_cwd,
                    workspace.record.protected,
                    workspace.record.task_key_prefix,
                    to_sql_u64("workspace task key counter", workspace.record.task_key_next)?,
                    to_sql_u64("desk revision", cx.revision.0)?,
                    to_sql_u64("workspace record revision", expected.0)?
                ],
            )?;
            require_one(changed, &format!("workspace {id}"), expected)?;
        }
        if state_changed {
            let expected = current.workspace_revisions[id];
            let tab_order_revision = if tab_order_changed {
                to_sql_u64("desk tab order revision", cx.revision.0)?
            } else {
                to_sql_u64(
                    "desk tab order revision",
                    current.workspace_order_revisions[id].0,
                )?
            };
            let changed = cx.write(
                "UPDATE desk_workspace_state SET sort_order = ?2, active_tab_id = ?3, \
                 revision = ?4, tab_order_revision = ?5 \
                 WHERE workspace_id = ?1 AND revision = ?6",
                params![
                    id,
                    workspace.sort_order,
                    workspace.active_tab_id,
                    to_sql_u64("desk revision", cx.revision.0)?,
                    tab_order_revision,
                    to_sql_u64("desk workspace revision", expected.0)?
                ],
            )?;
            require_one(changed, &format!("workspace {id}"), expected)?;
        }
    }
    Ok(order_changed)
}

fn workspace_ordinals(cx: &WriteCx<'_>) -> Result<Vec<(String, i64)>, StorageError> {
    load_ordinals(
        cx,
        "SELECT id, ordinal FROM workspaces ORDER BY ordinal",
        "workspace ordinals",
    )
}

fn team_ordinals(cx: &WriteCx<'_>) -> Result<Vec<(String, i64)>, StorageError> {
    load_ordinals(
        cx,
        "SELECT id, ordinal FROM desk_teams ORDER BY ordinal",
        "Team ordinals",
    )
}

fn load_ordinals(
    cx: &WriteCx<'_>,
    sql: &str,
    field: &'static str,
) -> Result<Vec<(String, i64)>, StorageError> {
    let mut statement = cx
        .conn
        .prepare(sql)
        .map_err(|error| sqlite_error("prepare desk order", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| sqlite_error("query desk order", error))?;
    rows.map(|row| row.map_err(|error| sqlite_error(field, error)))
        .collect()
}

fn next_team_ordinal(conn: &rusqlite::Connection) -> Result<i64, StorageError> {
    conn.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM desk_teams",
        [],
        |row| row.get(0),
    )
    .map_err(|error| sqlite_error("allocate Team ordinal", error))
}

fn moved_ids(current: &[&str], target: &[&str]) -> HashSet<String> {
    target
        .iter()
        .enumerate()
        .filter(|(index, id)| current.get(*index).copied() != Some(*id))
        .map(|(_, id)| (*id).to_owned())
        .collect()
}

fn indexed_tabs(snapshot: &DeskSnapshot) -> HashMap<&str, (&str, usize, &DeskTab)> {
    snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| {
            workspace
                .tabs
                .iter()
                .enumerate()
                .map(move |(ordinal, tab)| {
                    (
                        tab.id.as_str(),
                        (workspace.record.id.as_str(), ordinal, tab),
                    )
                })
        })
        .collect()
}

fn tab_ids(workspace: &DeskWorkspace) -> Vec<&str> {
    workspace.tabs.iter().map(|tab| tab.id.as_str()).collect()
}

fn team_lifecycle(team: &DeskTeam) -> &'static str {
    match team.lifecycle {
        super::super::records::DeskTeamLifecycle::Forming => "forming",
        super::super::records::DeskTeamLifecycle::Active => "active",
        super::super::records::DeskTeamLifecycle::Closing => "closing",
    }
}

fn require_one(changed: usize, entity: &str, expected: Revision) -> Result<(), StorageError> {
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::StaleRevision {
        entity: entity.to_owned(),
        expected: Some(expected),
        actual: None,
    })
}

fn corrupt(detail: impl Into<String>) -> StorageError {
    StorageError::Corrupt {
        detail: detail.into(),
    }
}
